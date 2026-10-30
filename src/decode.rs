//! W1/W2: pixel-layer decode — embedded MNF8/MNF9 files decode
//! byte-identically to their standalone decode (the W1 exit gate); the
//! W2 plane formats (RGB8/RGBA8/Gray8/GrayA8) decode raw and zstd
//! planes against the format-derived stride bound, hidden RGB
//! preserved (grammar rule 8 — the plane's alpha and color channels
//! all come back byte-exact).

use crate::error::{Error, Result};
use crate::parser::Chunk;
use crate::*;

/// The decoded pixel plane of an MNFX container: dims + interleaved
/// row-major samples in the header's pixel format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    /// Image dimensions (must equal the container header's).
    pub dims: manifold::codec::Dims,
    /// The container's pixel format (echoed for the caller).
    pub format: PixelFormat,
    /// Interleaved row-major samples, `width * height * stride` bytes
    /// (stride = 3 RGB8, 4 RGBA8/GrayA8, 1 Gray8).
    pub samples: Vec<u8>,
}

/// Decode a container's PIXEL chunk into pixels.
///
/// Dispatch by payload magic: `MNF8` -> `decode_mnf8`, `MNF9` ->
/// `decode_mnf9`. Raw/Zstd payloads are plain byte planes (v1 RGB8: the
/// raw_len bound checks the exact size); zstd itself lands at W2 with the
/// alpha layer (ponytail: raw/zstd cover no shipped producer yet).
pub fn decode_pixels(hdr: &ContainerHeader, chunks: &[Chunk]) -> Result<Pixels> {
    // 16-bit hi/lo plane formats: the pixels are the PIX16-HI/LO pair
    // (the parser enforces exactly one each, no Pixel chunk).
    if hdr.pixel_format.is_planar16() {
        return decode_planar16(hdr, chunks);
    }
    let pixel = chunks
        .iter()
        .find(|c| c.header.chunk_type == ChunkType::Pixel)
        .ok_or_else(|| Error::fmt_msg("mnfx: no pixel chunk"))?;
    if pixel.header.codec == Codec::MnF8 || pixel.header.codec == Codec::MnF9 {
        // Embed payloads are the engines' native RGB8; other formats ride
        // the plane codecs (grammar rule 9).
        if hdr.pixel_format != PixelFormat::Rgb8 {
            return Err(Error::fmt_msg(format!(
                "mnfx: embed codec {} requires pixel_format 1 (RGB8), got {}",
                pixel.header.codec as u8, hdr.pixel_format as u8
            )));
        }
        // An embed payload is a whole standalone file: dispatch by magic,
        // and the magic must AGREE with the chunk's codec field (a codec-5
        // payload whose magic says MNF8 is corrupt, not a silent remap).
        let magic: &[u8] = pixel
            .stored
            .get(0..4)
            .ok_or_else(|| Error::fmt_msg("mnfx: pixel embed shorter than a magic (4 B)"))?;
        let want: &[u8] = if pixel.header.codec == Codec::MnF8 {
            b"MNF8"
        } else {
            manifold::mnf9::MAGIC.as_slice()
        };
        if magic != want {
            return Err(Error::fmt_msg(format!(
                "mnfx: pixel embed codec {} wants magic {} got {magic:02x?}",
                pixel.header.codec as u8,
                String::from_utf8_lossy(want)
            )));
        }
        // The codec field routed here; its payload is exactly that engine's
        // whole standalone file (magic already agrees above).
        let (dims, rgb) = if pixel.header.codec == Codec::MnF8 {
            let (d, px, _) = manifold::mnf8::decode_mnf8(&pixel.stored)
                .map_err(|e| Error::fmt_msg(format!("mnfx: MNF8 embed decode: {e}")))?;
            (d, px)
        } else {
            manifold::mnf9::decode_mnf9(&pixel.stored)
                .map_err(|e| Error::fmt_msg(format!("mnfx: MNF9 embed decode: {e}")))?
        };
        check_dims(hdr, dims.width, dims.height)?;
        return Ok(Pixels {
            dims,
            format: hdr.pixel_format,
            samples: plane_bytes(hdr, rgb)?,
        });
    }
    // Raw / Zstd: an interleaved byte plane, bound = stride × pixels
    // (grammar rule 4: raw_len == stored_len == plane length).
    let bound = plane_bound(hdr)?;
    let plane: Vec<u8> = match pixel.header.codec {
        Codec::Raw => {
            if pixel.stored.len() != bound || pixel.header.raw_len as usize != bound {
                return Err(Error::fmt_msg(format!(
                    "mnfx: raw pixel layer {} B != header-derived {} B",
                    pixel.stored.len(),
                    bound
                )));
            }
            pixel.stored.clone()
        }
        Codec::Zstd => decompress_bounded(&pixel.stored, bound)?,
        _ => {
            return Err(Error::fmt_msg(
                "mnfx: non-embed pixel codec must be raw or zstd",
            ))
        }
    };
    Ok(Pixels {
        dims: manifold::codec::Dims {
            width: hdr.width,
            height: hdr.height,
        },
        format: hdr.pixel_format,
        samples: plane,
    })
}

/// The exact byte length of the pixel plane: width × height × stride(format).
fn plane_bound(hdr: &ContainerHeader) -> Result<usize> {
    let w =
        usize::try_from(hdr.width).map_err(|_| Error::fmt_msg("mnfx: width overflows usize"))?;
    let h =
        usize::try_from(hdr.height).map_err(|_| Error::fmt_msg("mnfx: height overflows usize"))?;
    w.checked_mul(h)
        .and_then(|a| a.checked_mul(hdr.pixel_format.stride_mult()))
        .ok_or_else(|| Error::fmt_msg("mnfx: pixel area overflows usize"))
}

/// Decode a 16-bit container: join PIX16-HI + PIX16-LO into the
/// interleaved big-endian sample plane (PNG wire order: hi plane = each
/// sample's first byte). Each plane chunk carries exactly half the
/// plane bound, per its own codec (raw/zstd; embed codecs stay RGB8-only
/// by rule 9, so plane chunks must not carry them).
fn decode_planar16(hdr: &ContainerHeader, chunks: &[Chunk]) -> Result<Pixels> {
    let dims = manifold::codec::Dims {
        width: hdr.width,
        height: hdr.height,
    };
    let half = plane_bound(hdr)? / 2;
    let mut planes: [Option<Vec<u8>>; 2] = [None, None];
    for c in chunks {
        let slot = match c.header.chunk_type {
            ChunkType::Pix16Hi => 0usize,
            ChunkType::Pix16Lo => 1usize,
            _ => continue,
        };
        if c.header.codec == Codec::MnF8 || c.header.codec == Codec::MnF9 {
            return Err(Error::fmt_msg(format!(
                "mnfx: 16-bit plane chunk {slot} must not carry an embed codec"
            )));
        }
        let raw = match c.header.codec {
            Codec::Raw => {
                if c.stored.len() != half || c.header.raw_len as usize != half {
                    return Err(Error::fmt_msg(format!(
                        "mnfx: 16-bit plane {slot} {} B != half-bound {} B",
                        c.stored.len(),
                        half
                    )));
                }
                c.stored.clone()
            }
            Codec::Zstd => decompress_bounded(&c.stored, half)?,
            _ => {
                return Err(Error::fmt_msg(
                    "mnfx: 16-bit plane codec must be raw or zstd",
                ))
            }
        };
        planes[slot] = Some(raw);
    }
    let (hi, lo) = match planes {
        [Some(h), Some(l)] => (h, l),
        _ => return Err(Error::fmt_msg("mnfx: no pixel chunk")),
    };
    // Interleave: samples[n] = (hi[n], lo[n]) big-endian.
    let mut samples = Vec::with_capacity(hi.len() * 2);
    for i in 0..hi.len() {
        samples.push(hi[i]);
        samples.push(lo[i]);
    }
    Ok(Pixels {
        dims,
        format: hdr.pixel_format,
        samples,
    })
}

/// A raw (uncompressed) plane re-bounded by the header-derived length.
fn plane_bytes(hdr: &ContainerHeader, plane: Vec<u8>) -> Result<Vec<u8>> {
    let want = plane_bound(hdr)?;
    if plane.len() != want {
        return Err(Error::fmt_msg(format!(
            "mnfx: pixel plane {} B != header-derived {} B",
            plane.len(),
            want
        )));
    }
    Ok(plane)
}

/// Zstd decode with the manifold-repo hardening: mandatory frame
/// checksum (the library accepts a checksumless frame whose interior
/// corruption goes silent — the MNF7-precedent rule), window capped at
/// 8 MiB (2^23, matching the level-19 encoder ceiling; a hostile frame
/// must not claim the 128 MiB library default), and the decompressed
/// size bounded by `bound + 1` with the take-limit read as the bomb check.
fn decompress_bounded(stored: &[u8], bound: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut d =
        zstd::stream::read::Decoder::new(stored).map_err(|_| Error::fmt_msg("mnfx: zstd init"))?;
    d.window_log_max(23)
        .map_err(|_| Error::fmt_msg("mnfx: zstd window param"))?;
    let mut v = Vec::new();
    d.take(bound as u64 + 1)
        .read_to_end(&mut v)
        .map_err(|_| Error::fmt_msg("mnfx: corrupt zstd stream"))?;
    if v.len() != bound {
        return Err(Error::fmt_msg(format!(
            "mnfx: zstd pixel plane {} B != bound {}",
            v.len(),
            bound
        )));
    }
    Ok(v)
}

fn check_dims(hdr: &ContainerHeader, w: u32, h: u32) -> Result<()> {
    if (w, h) != (hdr.width, hdr.height) {
        return Err(Error::fmt_msg(format!(
            "mnfx: embed dims {w}x{h} != container {}x{}",
            hdr.width, hdr.height
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! W1: the embedded==standalone gate, per ADR-009's exit criteria.
    //!
    //! Fixtures are REAL engine files: MNF8 via `encode_mnf8` (both
    //! profiles), MNF9 via `encode_exact` (the solid fixture discovers an
    //! exact rule). Each embed test asserts the container decode equals the
    //! STANDALONE decode of the same file through the engine directly —
    //! the differential pattern, not a self-roundtrip.

    use super::*;
    use crate::parser::{parse, serialize};

    fn hdr_for(w: u32, h: u32) -> ContainerHeader {
        ContainerHeader {
            pixel_format: PixelFormat::Rgb8,
            orientation: 1,
            chunk_count: 1,
            width: w,
            height: h,
        }
    }

    /// Wrap a standalone engine file as the sole Pixel chunk of an MNFX.
    fn wrap(file: &[u8], codec: Codec, w: u32, h: u32) -> Vec<u8> {
        serialize(&hdr_for(w, h), &[(ChunkType::Pixel, codec, file)]).unwrap()
    }

    /// Wrap chunks under an EXPLICIT header (non-RGB8 formats).
    fn wrap_with(hdr: &ContainerHeader, chunks: &[(ChunkType, Codec, &[u8])]) -> Vec<u8> {
        serialize(hdr, chunks).unwrap()
    }

    fn solid_8x8() -> (manifold::codec::Dims, Vec<u8>) {
        let dims = manifold::codec::Dims {
            width: 8,
            height: 8,
        };
        (dims, vec![7u8; 8 * 8 * 3])
    }

    fn noisy_16x16() -> (manifold::codec::Dims, Vec<u8>) {
        // Deterministic pseudo-noise: forces literal tiles (no exact rule
        // fits, so MNF9's arm is absent and MNF8 carries the file).
        let dims = manifold::codec::Dims {
            width: 16,
            height: 16,
        };
        let mut px = Vec::with_capacity(16 * 16 * 3);
        let mut s: u32 = 0x2545_F491;
        for _ in 0..16 * 16 {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            px.extend_from_slice(&[(s >> 24) as u8, (s >> 16) as u8, (s >> 8) as u8]);
        }
        (dims, px)
    }

    #[test]
    fn embedded_mnf8_equals_standalone_balanced() {
        let (dims, px) = noisy_16x16();
        let (file, _st, _b) =
            manifold::mnf8::encode_mnf8(&px, dims, false, manifold::mnf8::Profile::Balanced);
        let buf = wrap(&file, Codec::MnF8, dims.width, dims.height);
        let (h, chunks) = parse(&buf).unwrap();
        let got = decode_pixels(&h, &chunks).unwrap();
        let (sdims, spx, _b) = manifold::mnf8::decode_mnf8(&file).unwrap();
        assert_eq!(got.dims, sdims);
        assert_eq!(got.samples, spx, "embedded MNF8 decode != standalone");
        assert_eq!(got.samples, px, "MNFX round-trip lost pixels");
    }

    #[test]
    fn embedded_mnf8_equals_standalone_maximum() {
        let (dims, px) = solid_8x8();
        let (file, _st, _b) =
            manifold::mnf8::encode_mnf8(&px, dims, false, manifold::mnf8::Profile::Maximum);
        let buf = wrap(&file, Codec::MnF8, dims.width, dims.height);
        let (h, chunks) = parse(&buf).unwrap();
        let got = decode_pixels(&h, &chunks).unwrap();
        let (sdims, spx, _b) = manifold::mnf8::decode_mnf8(&file).unwrap();
        assert_eq!(got.dims, sdims);
        assert_eq!(got.samples, spx, "embedded MNF8 decode != standalone");
        assert_eq!(got.samples, px);
    }

    #[test]
    fn embedded_mnf9_equals_standalone() {
        let (dims, px) = solid_8x8();
        let file = manifold::mnf9::encode_exact(&px, dims)
            .expect("solid fixture must discover an exact rule");
        assert_eq!(&file[0..4], b"MNF9");
        let buf = wrap(&file, Codec::MnF9, dims.width, dims.height);
        let (h, chunks) = parse(&buf).unwrap();
        let got = decode_pixels(&h, &chunks).unwrap();
        let (sdims, spx) = manifold::mnf9::decode_mnf9(&file).unwrap();
        assert_eq!(got.dims, sdims);
        assert_eq!(got.samples, spx, "embedded MNF9 decode != standalone");
        assert_eq!(got.samples, px);
    }

    #[test]
    fn embed_magic_must_agree_with_codec_field() {
        let (dims, px) = solid_8x8();
        // MNF9 file declared as codec 4 (MNF8): corrupt, not a silent remap.
        let file = manifold::mnf9::encode_exact(&px, dims).unwrap();
        let buf = wrap(&file, Codec::MnF8, dims.width, dims.height);
        let (h, chunks) = parse(&buf).unwrap();
        let err = decode_pixels(&h, &chunks).unwrap_err();
        assert!(
            format!("{err}").contains("wants magic MNF8"),
            "mismatched magic must error with the codec context, got: {err}"
        );
    }

    #[test]
    fn embed_shorter_than_magic_rejects() {
        let buf = wrap(b"MNF", Codec::MnF8, 8, 8);
        let (h, chunks) = parse(&buf).unwrap();
        assert!(decode_pixels(&h, &chunks).is_err());
    }

    #[test]
    fn embed_dims_mismatch_container_rejects() {
        let (dims, px) = solid_8x8();
        let file = manifold::mnf9::encode_exact(&px, dims).unwrap();
        // Container header claims 9x9; the embed decodes 8x8.
        let buf = wrap(&file, Codec::MnF9, 9, 9);
        let (h, chunks) = parse(&buf).unwrap();
        let err = decode_pixels(&h, &chunks).unwrap_err();
        assert!(format!("{err}").contains("dims"), "got: {err}");
    }

    #[test]
    fn raw_pixel_layer_size_must_match_header_derived_bound() {
        let (w, h) = (8u32, 8u32);
        let mut payload = vec![1u8; (w * h * 3) as usize];
        payload[0] = 9; // not solid: can't be mistaken for an MNF magic
        let buf = wrap(&payload, Codec::Raw, w, h);
        let (hc, chunks) = parse(&buf).unwrap();
        let got = decode_pixels(&hc, &chunks).unwrap();
        assert_eq!(got.samples, payload);
        // One byte short: wrong plane size must reject.
        let buf2 = wrap(&payload[..payload.len() - 1], Codec::Raw, w, h);
        let (hc2, ch2) = parse(&buf2).unwrap();
        assert!(decode_pixels(&hc2, &ch2).is_err());
    }

    #[test]
    fn zstd_pixel_plane_roundtrips() {
        // A real zstd frame (checksum on, the mandatory-production shape)
        // carrying an RGBA plane: hidden RGB preserved = all 4 channels
        // come back byte-exact.
        let (w, h) = (8u32, 8u32);
        let hdr = ContainerHeader {
            pixel_format: PixelFormat::Rgba8,
            ..hdr_for(w, h)
        };
        let mut plane = vec![0u8; (w * h) as usize * 4];
        for (i, b) in plane.iter_mut().enumerate() {
            *b = (i % 251) as u8; // noisy: no accidental structure
        }
        use std::io::Write;
        let mut e = zstd::stream::Encoder::new(Vec::new(), 19).unwrap();
        e.include_checksum(true).unwrap();
        e.write_all(&plane).unwrap();
        let frame = e.finish().unwrap();
        let buf = wrap_with(&hdr, &[(ChunkType::Pixel, Codec::Zstd, &frame)]);
        let (hc, chunks) = parse(&buf).unwrap();
        let got = decode_pixels(&hc, &chunks).unwrap();
        assert_eq!(got.format, PixelFormat::Rgba8);
        assert_eq!(got.samples, plane, "zstd RGBA plane roundtrip lost bytes");
    }

    #[test]
    fn zstd_over_bound_rejects() {
        // A frame decompressing to MORE than the header-derived plane
        // bound is a bomb: reject (also exercises the raw_len-mismatch arm).
        let hdr = hdr_for(8, 8);
        let bound = plane_bound(&hdr).unwrap();
        let big = vec![0u8; bound + 1];
        use std::io::Write;
        let mut e = zstd::stream::Encoder::new(Vec::new(), 19).unwrap();
        e.include_checksum(true).unwrap();
        e.write_all(&big).unwrap();
        let frame = e.finish().unwrap();
        let buf = wrap_with(&hdr, &[(ChunkType::Pixel, Codec::Zstd, &frame)]);
        let (hc, chunks) = parse(&buf).unwrap();
        assert!(decode_pixels(&hc, &chunks).is_err());
    }

    #[test]
    fn gray8_and_graya8_planes_bound_by_their_stride() {
        let (w, h) = (8u32, 8u32);
        assert_eq!(PixelFormat::Gray8.stride_mult(), 1);
        assert_eq!(PixelFormat::GrayA8.stride_mult(), 2);
        // Gray8: a plane of exactly w*h*1 decodes.
        let gray_hdr = ContainerHeader {
            pixel_format: PixelFormat::Gray8,
            ..hdr_for(w, h)
        };
        let plane = vec![7u8; (w * h) as usize];
        let buf = wrap_with(&gray_hdr, &[(ChunkType::Pixel, Codec::Raw, &plane)]);
        let (hc, chunks) = parse(&buf).unwrap();
        let got = decode_pixels(&hc, &chunks).unwrap();
        assert_eq!(got.samples, plane);
        // Gray8 one byte over the bound: reject.
        let mut over = plane.clone();
        over.push(7);
        let buf2 = wrap_with(&gray_hdr, &[(ChunkType::Pixel, Codec::Raw, &over)]);
        let (hc2, ch2) = parse(&buf2).unwrap();
        assert!(decode_pixels(&hc2, &ch2).is_err());
        // GrayA8: w*h*2 decodes.
        let ga_hdr = ContainerHeader {
            pixel_format: PixelFormat::GrayA8,
            ..hdr_for(w, h)
        };
        let ga_plane = vec![9u8; (w * h) as usize * 2];
        let buf3 = wrap_with(&ga_hdr, &[(ChunkType::Pixel, Codec::Raw, &ga_plane)]);
        let (hc3, ch3) = parse(&buf3).unwrap();
        let got3 = decode_pixels(&hc3, &ch3).unwrap();
        assert_eq!(got3.format, PixelFormat::GrayA8);
        assert_eq!(got3.samples, ga_plane);
    }

    #[test]
    fn embed_codec_rejected_outside_pixel_layer() {
        let hdr = ContainerHeader {
            chunk_count: 2,
            ..hdr_for(8, 8)
        };
        let (dims, px) = solid_8x8();
        let file = manifold::mnf9::encode_exact(&px, dims).unwrap();
        let buf = serialize(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &px),
                (ChunkType::Exif, Codec::MnF9, &file),
            ],
        )
        .unwrap();
        // serialize emits it; parse must reject (codec 5 is pixel-only).
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn mnf8_codec_allowed_on_nonpixel_chunk() {
        // W3's plane chunks will carry codec 4; the parser must not
        // reject the frozen vocabulary before that phase wires it.
        let hdr = ContainerHeader {
            chunk_count: 2,
            ..hdr_for(8, 8)
        };
        let (dims, px) = solid_8x8();
        let file =
            manifold::mnf8::encode_mnf8(&px, dims, false, manifold::mnf8::Profile::Balanced).0;
        let buf = serialize(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &px),
                (ChunkType::Unknown(0x0050), Codec::MnF8, &file),
            ],
        )
        .unwrap();
        let (_h, chunks) = parse(&buf).unwrap();
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn embed_codec_requires_rgb8_pixel_format() {
        let (dims, px) = solid_8x8();
        let file = manifold::mnf9::encode_exact(&px, dims).unwrap();
        // Codec 5 under an RGBA8 header: reject (grammar rule 9).
        let hdr = ContainerHeader {
            pixel_format: PixelFormat::Rgba8,
            ..hdr_for(dims.width, dims.height)
        };
        let buf = wrap_with(&hdr, &[(ChunkType::Pixel, Codec::MnF9, &file)]);
        let (h, chunks) = parse(&buf).unwrap();
        let err = decode_pixels(&h, &chunks).unwrap_err();
        assert!(format!("{err}").contains("RGB8"), "got: {err}");
    }

    #[test]
    fn mnfx_framing_overhead_is_34_bytes_exact() {
        // The +34-B rule: MNFX(solid 8x8) - the standalone file = 34.
        let (dims, px) = solid_8x8();
        let file = manifold::mnf9::encode_exact(&px, dims).unwrap();
        let buf = wrap(&file, Codec::MnF9, dims.width, dims.height);
        assert_eq!(buf.len() - file.len(), 34);
    }

    // ---- W3: 16-bit hi/lo plane layers ----

    fn gray16_hdr(w: u32, h: u32) -> ContainerHeader {
        planar16_hdr(PixelFormat::Gray16, w, h, 0)
    }

    fn planar16_hdr(fmt: PixelFormat, w: u32, h: u32, extra: usize) -> ContainerHeader {
        ContainerHeader {
            pixel_format: fmt,
            chunk_count: 2 + extra as u16, // the HI/LO pair (+ extras)
            ..hdr_for(w, h)
        }
    }

    #[test]
    fn gray16_hilo_planes_interleave_bigendian() {
        // The interleave-order pin: sample n = (hi[n], lo[n]). Build
        // planes whose bytes differ so a swapped join fails loudly.
        let (w, h) = (8u32, 8u32);
        let mut hi = Vec::new();
        let mut lo = Vec::new();
        for i in 0..(w * h) as usize {
            hi.push((i % 200) as u8 + 1);
            lo.push(((i * 7) % 200) as u8 + 2);
        }
        let hdr = gray16_hdr(w, h);
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pix16Hi, Codec::Raw, &hi),
                (ChunkType::Pix16Lo, Codec::Raw, &lo),
            ],
        );
        let (hc, chunks) = parse(&buf).unwrap();
        let got = decode_pixels(&hc, &chunks).unwrap();
        assert_eq!(got.format, PixelFormat::Gray16);
        let mut want = Vec::with_capacity(hi.len() * 2);
        for i in 0..hi.len() {
            want.push(hi[i]);
            want.push(lo[i]);
        }
        assert_eq!(got.samples, want);
        // And the u16 read-back is big-endian.
        let first = u16::from_be_bytes([got.samples[0], got.samples[1]]);
        assert_eq!(first, u16::from(hi[0]) * 256 + u16::from(lo[0]));
    }

    #[test]
    fn rgb16_planes_roundtrip() {
        let (w, h) = (8u32, 8u32);
        let hdr = planar16_hdr(PixelFormat::Rgb16, w, h, 0);
        let mut hi = Vec::new();
        let mut lo = Vec::new();
        for i in 0..(w * h * 3) as usize {
            hi.push((i % 251) as u8);
            lo.push(((i * 13) % 251) as u8);
        }
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pix16Hi, Codec::Raw, &hi),
                (ChunkType::Pix16Lo, Codec::Raw, &lo),
            ],
        );
        let (hc, chunks) = parse(&buf).unwrap();
        let got = decode_pixels(&hc, &chunks).unwrap();
        assert_eq!(got.format, PixelFormat::Rgb16);
        assert_eq!(got.samples.len(), (w * h * 6) as usize);
        let mut want = Vec::with_capacity(hi.len() * 2);
        for i in 0..hi.len() {
            want.push(hi[i]);
            want.push(lo[i]);
        }
        assert_eq!(got.samples, want);
    }

    #[test]
    fn gray16_zstd_plane_roundtrip() {
        let (w, h) = (8u32, 8u32);
        let hdr = gray16_hdr(w, h);
        let mut hi = Vec::new();
        let mut lo = Vec::new();
        for i in 0..(w * h) as usize {
            hi.push((i * 3 % 251) as u8);
            lo.push((i * 5 % 251) as u8);
        }
        use std::io::Write;
        let zstd_plane = |p: &[u8]| {
            let mut e = zstd::stream::Encoder::new(Vec::new(), 19).unwrap();
            e.include_checksum(true).unwrap();
            e.write_all(p).unwrap();
            e.finish().unwrap()
        };
        let hi_frame = zstd_plane(&hi);
        let lo_frame = zstd_plane(&lo);
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pix16Hi, Codec::Zstd, &hi_frame),
                (ChunkType::Pix16Lo, Codec::Zstd, &lo_frame),
            ],
        );
        let (hc, chunks) = parse(&buf).unwrap();
        let got = decode_pixels(&hc, &chunks).unwrap();
        let mut want = Vec::with_capacity(hi.len() * 2);
        for i in 0..hi.len() {
            want.push(hi[i]);
            want.push(lo[i]);
        }
        assert_eq!(got.samples, want);
    }

    #[test]
    fn plane_size_mismatch_rejects() {
        let (w, h) = (8u32, 8u32);
        let hdr = gray16_hdr(w, h);
        let hi = vec![1u8; (w * h) as usize];
        let lo_short = vec![2u8; (w * h) as usize - 1];
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pix16Hi, Codec::Raw, &hi),
                (ChunkType::Pix16Lo, Codec::Raw, &lo_short),
            ],
        );
        let (hc, chunks) = parse(&buf).unwrap();
        let err = decode_pixels(&hc, &chunks).unwrap_err();
        assert!(format!("{err}").contains("half-bound"), "got: {err}");
    }

    #[test]
    fn parser_rejects_16bit_shape_errors() {
        let (w, h) = (8u32, 8u32);
        let hi = vec![1u8; (w * h) as usize];
        let lo = vec![2u8; (w * h) as usize];
        let px8 = vec![7u8; (w * h * 3) as usize];
        // Plane chunks under an RGB8 header: reject.
        let rgb8_hdr = hdr_for(w, h);
        let buf = wrap_with(&rgb8_hdr, &[(ChunkType::Pix16Hi, Codec::Raw, &hi)]);
        assert!(parse(&buf).is_err());
        // Duplicate HI plane: reject.
        let dup_hdr = planar16_hdr(PixelFormat::Gray16, w, h, 1);
        let buf = wrap_with(
            &dup_hdr,
            &[
                (ChunkType::Pix16Hi, Codec::Raw, &hi),
                (ChunkType::Pix16Hi, Codec::Raw, &hi),
                (ChunkType::Pix16Lo, Codec::Raw, &lo),
            ],
        );
        assert!(parse(&buf).is_err());
        // Missing LO plane: reject. Header must claim ONE chunk (serialize
        // asserts chunk_count == chunks.len(); the missing-plane reject
        // fires in parse's post-check). Build that header directly.
        let miss_hdr = ContainerHeader {
            pixel_format: PixelFormat::Gray16,
            chunk_count: 1,
            ..hdr_for(w, h)
        };
        let buf = wrap_with(&miss_hdr, &[(ChunkType::Pix16Hi, Codec::Raw, &hi)]);
        assert!(parse(&buf).is_err());
        // Pixel chunk INSIDE a 16-bit container: reject (no double-booking).
        let both_hdr = planar16_hdr(PixelFormat::Gray16, w, h, 1);
        let buf = wrap_with(
            &both_hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &px8),
                (ChunkType::Pix16Hi, Codec::Raw, &hi),
                (ChunkType::Pix16Lo, Codec::Raw, &lo),
            ],
        );
        assert!(parse(&buf).is_err());
    }

    #[test]
    fn embed_codec_rejected_on_plane_chunk() {
        let (w, h) = (8u32, 8u32);
        let hi = vec![1u8; (w * h) as usize];
        let lo = vec![2u8; (w * h) as usize];
        let hdr = gray16_hdr(w, h);
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pix16Hi, Codec::MnF8, &hi),
                (ChunkType::Pix16Lo, Codec::Raw, &lo),
            ],
        );
        let (hc, chunks) = parse(&buf).unwrap();
        assert!(decode_pixels(&hc, &chunks).is_err());
    }

    #[test]
    fn parse_rejects_plane_chunk_under_rgb8_post_state_too() {
        // A 16-bit container whose ONLY chunk is a Pixel chunk (the
        // 8-bit shape): the post-check must reject the missing planes.
        let (w, h) = (8u32, 8u32);
        // Build raw bytes by hand: Gray16 header, single Pixel chunk.
        // serialize can't express it, so patch a serialized RGB8 file.
        let px8 = vec![7u8; (w * h * 3) as usize];
        let mut buf = wrap(&px8, Codec::Raw, w, h);
        buf[6] = PixelFormat::Gray16 as u8; // fmt field from 1 → 6
        assert!(parse(&buf).is_err());
    }
}
