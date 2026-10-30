//! W1: pixel-layer decode — embedded MNF8/MNF9 files decode byte-identically
//! to their standalone decode (the ADR-009 W1 exit gate).

use crate::error::{Error, Result};
use crate::parser::Chunk;
use crate::*;

/// The decoded pixels of an MNFX container: dims + RGB8 row-major bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    /// Image dimensions (must equal the container header's).
    pub dims: manifold::codec::Dims,
    /// RGB8 row-major samples, `width * height * 3` bytes.
    pub rgb: Vec<u8>,
}

/// Decode a container's PIXEL chunk into pixels.
///
/// Dispatch by payload magic: `MNF8` -> `decode_mnf8`, `MNF9` ->
/// `decode_mnf9`. Raw/Zstd payloads are plain byte planes (v1 RGB8: the
/// raw_len bound checks the exact size); zstd itself lands at W2 with the
/// alpha layer (ponytail: raw/zstd cover no shipped producer yet).
pub fn decode_pixels(hdr: &ContainerHeader, chunks: &[Chunk]) -> Result<Pixels> {
    let pixel = chunks
        .iter()
        .find(|c| c.header.chunk_type == ChunkType::Pixel)
        .ok_or_else(|| Error::fmt_msg("mnfx: no pixel chunk"))?;
    if pixel.header.codec == Codec::MnF8 || pixel.header.codec == Codec::MnF9 {
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
        return Ok(Pixels { dims, rgb });
    }
    // Raw / Zstd: a plain byte plane bounded by raw_len (the grammar's
    // decode bound). Zstd's decompression lands with W2's alpha layer;
    // until a producer exists, admit only Raw here.
    if pixel.header.codec != Codec::Raw {
        return Err(Error::fmt_msg(
            "mnfx: zstd pixel layer decode lands at W2 (no producer yet)",
        ));
    }
    let w =
        usize::try_from(hdr.width).map_err(|_| Error::fmt_msg("mnfx: width overflows usize"))?;
    let h =
        usize::try_from(hdr.height).map_err(|_| Error::fmt_msg("mnfx: height overflows usize"))?;
    let want = w
        .checked_mul(h)
        .and_then(|a| a.checked_mul(3))
        .ok_or_else(|| Error::fmt_msg("mnfx: pixel area overflows usize"))?;
    if pixel.stored.len() != want || pixel.header.raw_len as usize != want {
        return Err(Error::fmt_msg(format!(
            "mnfx: raw pixel layer {} B != header-derived {} B",
            pixel.stored.len(),
            want
        )));
    }
    Ok(Pixels {
        dims: manifold::codec::Dims {
            width: hdr.width,
            height: hdr.height,
        },
        rgb: pixel.stored.clone(),
    })
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
        assert_eq!(got.rgb, spx, "embedded MNF8 decode != standalone");
        assert_eq!(got.rgb, px, "MNFX round-trip lost pixels");
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
        assert_eq!(got.rgb, spx, "embedded MNF8 decode != standalone");
        assert_eq!(got.rgb, px);
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
        assert_eq!(got.rgb, spx, "embedded MNF9 decode != standalone");
        assert_eq!(got.rgb, px);
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
        assert_eq!(got.rgb, payload);
        // One byte short: wrong plane size must reject.
        let buf2 = wrap(&payload[..payload.len() - 1], Codec::Raw, w, h);
        let (hc2, ch2) = parse(&buf2).unwrap();
        assert!(decode_pixels(&hc2, &ch2).is_err());
    }

    #[test]
    fn zstd_pixel_layer_rejected_until_w2() {
        let buf = wrap(b"zstd-payload", Codec::Zstd, 8, 8);
        let (h, chunks) = parse(&buf).unwrap();
        let err = decode_pixels(&h, &chunks).unwrap_err();
        assert!(format!("{err}").contains("W2"), "got: {err}");
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
    fn mnfx_framing_overhead_is_34_bytes_exact() {
        // The +34-B rule: MNFX(solid 8x8) - the standalone file = 34.
        let (dims, px) = solid_8x8();
        let file = manifold::mnf9::encode_exact(&px, dims).unwrap();
        let buf = wrap(&file, Codec::MnF9, dims.width, dims.height);
        assert_eq!(buf.len() - file.len(), 34);
    }
}
