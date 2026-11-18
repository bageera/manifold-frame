//! W1–W4: pixel-layer decode — embedded MNF8/MNF9 files decode
//! byte-identically to their standalone decode (the W1 exit gate); the
//! plane formats (RGB8/RGBA8/Gray8/GrayA8, W2) decode raw and zstd
//! planes against the format-derived stride bound, hidden RGB
//! preserved (grammar rule 8 — the plane's alpha and color channels
//! all come back byte-exact); W3 adds the 16-bit PIX16-HI/LO pairs
//! (big-endian interleave); W4 adds the metadata surface (rule 12).

use crate::error::{Error, Result};
use crate::parser::{BorrowedChunk, Chunk};
use crate::*;
use std::borrow::Cow;

// ---- W8-D2: the shared decode surface over owned AND borrowed chunks ----

/// The decoded-relevant surface of one chunk, shared by the owning and
/// borrowed paths (one interpretation implementation; D2.2). `stored`
/// borrows from the chunk — whose own borrow chain for
/// [`BorrowedChunk`] reaches back to the ORIGINAL input buffer, so a
/// raw plane can stay zero-copy end to end.
pub(crate) struct ChunkView<'a> {
    pub chunk_type: ChunkType,
    pub codec: Codec,
    pub raw_len: u32,
    pub stored: &'a [u8],
}

pub(crate) trait AsChunkView<'a> {
    fn as_view(&'a self) -> ChunkView<'a>;
}

impl<'a> AsChunkView<'a> for Chunk {
    fn as_view(&'a self) -> ChunkView<'a> {
        ChunkView {
            chunk_type: self.header.chunk_type,
            codec: self.header.codec,
            raw_len: self.header.raw_len,
            stored: &self.stored,
        }
    }
}

impl<'a> AsChunkView<'a> for BorrowedChunk<'a> {
    fn as_view(&'a self) -> ChunkView<'a> {
        ChunkView {
            chunk_type: self.header.chunk_type,
            codec: self.header.codec,
            raw_len: self.header.raw_len,
            stored: self.stored,
        }
    }
}

pub(crate) fn views_of<'a, T: AsChunkView<'a>>(chunks: &'a [T]) -> Vec<ChunkView<'a>> {
    chunks.iter().map(|c| c.as_view()).collect()
}

/// The decoded pixel plane of an MNFX container: dims + interleaved
/// row-major samples in the header's pixel format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    /// Image dimensions (must equal the container header's).
    pub dims: manifold::codec::Dims,
    /// The container's pixel format (echoed for the caller).
    pub format: PixelFormat,
    /// Interleaved row-major samples, `width * height * stride` bytes
    /// (stride = 3 RGB8, 4 RGBA8, 1 Gray8, 2 GrayA8, 6 RGB16,
    /// 2 Gray16 — `PixelFormat::stride_mult`).
    pub samples: Vec<u8>,
}

/// W8-D2: the borrowed plane — `samples` is a `Cow`: a RAW payload
/// stays a reference INSIDE the input buffer (the D2.4 zero-copy gate,
/// asserted in `tests/w8d2_parity.rs`); decompressed or engine-decoded
/// outputs are materialized owned, by design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorrowedPixels<'a> {
    /// Image dimensions (must equal the container header's).
    pub dims: manifold::codec::Dims,
    /// The container's pixel format (echoed for the caller).
    pub format: PixelFormat,
    /// Interleaved row-major samples: borrowed for raw planes, owned
    /// for decompressed/decoded outputs.
    pub samples: Cow<'a, [u8]>,
}

/// Decode a container's PIXEL chunk into pixels.
///
/// W8-D2: the owning wrapper — the ONE interpretation implementation
/// ([`decode_pixels_view`]) runs over chunk views and this
/// materializes the (possibly borrowed) plane into the historical
/// owning `Pixels`.
pub fn decode_pixels(hdr: &ContainerHeader, chunks: &[Chunk]) -> Result<Pixels> {
    let (dims, samples) = decode_pixels_gen(hdr, chunks, None)?;
    Ok(Pixels {
        dims,
        format: hdr.pixel_format,
        samples: samples.into_owned(),
    })
}

/// W8-D2: borrowed decode — same interpretation, payloads (and RAW
/// planes) borrowed from the input wherever the format allows.
/// W8-E2: limits-aware borrowed decode — every ceiling enforced before
/// the matching allocation/decompression; semantics are otherwise the
/// unlimited reader's (relaxed limits behave identically).
pub fn decode_pixels_borrowed_limited<'a>(
    hdr: &ContainerHeader,
    chunks: &'a [BorrowedChunk<'a>],
    limits: &crate::limits::DecodeLimits,
) -> Result<BorrowedPixels<'a>> {
    let (dims, samples) = decode_pixels_gen(hdr, chunks, Some(limits))?;
    Ok(BorrowedPixels {
        dims,
        format: hdr.pixel_format,
        samples,
    })
}

/// W8-E2: owning limits-aware decode (plane ceilings enforced in the
/// shared core before the materialization copy).
pub fn decode_pixels_limited(
    hdr: &ContainerHeader,
    chunks: &[Chunk],
    limits: &crate::limits::DecodeLimits,
) -> Result<Pixels> {
    let (dims, samples) = decode_pixels_gen(hdr, chunks, Some(limits))?;
    Ok(Pixels {
        dims,
        format: hdr.pixel_format,
        samples: samples.into_owned(),
    })
}

pub fn decode_pixels_borrowed<'a>(
    hdr: &ContainerHeader,
    chunks: &'a [BorrowedChunk<'a>],
) -> Result<BorrowedPixels<'a>> {
    let (dims, samples) = decode_pixels_gen(hdr, chunks, None)?;
    Ok(BorrowedPixels {
        dims,
        format: hdr.pixel_format,
        samples,
    })
}

/// The ONE interpretation implementation (D2.2): header-derived bounds,
/// embed magic agreement, raw/zstd plane decoding, planar16 assembly —
/// identical for owned and borrowed callers; only the raw plane's
/// backing differs (`Cow::Borrowed` vs a materialized copy). Generic
/// over the chunk representation: for borrowed chunks the RAW plane is
/// the input's bytes (zero-copy; asserted in the parity suite).
fn decode_pixels_gen<'a, T: AsChunkView<'a>>(
    hdr: &ContainerHeader,
    chunks: &'a [T],
    limits: Option<&crate::limits::DecodeLimits>,
) -> Result<(manifold::codec::Dims, Cow<'a, [u8]>)> {
    // 16-bit hi/lo plane formats: the pixels are the PIX16-HI/LO pair
    // (the parser enforces exactly one each, no Pixel chunk).
    if hdr.pixel_format.is_planar16() {
        return decode_planar16_view(hdr, chunks, limits).map(|samples| {
            (
                manifold::codec::Dims {
                    width: hdr.width,
                    height: hdr.height,
                },
                Cow::Owned(samples),
            )
        });
    }
    let views = views_of(chunks);
    let pixel = views
        .iter()
        .find(|v| v.chunk_type == ChunkType::Pixel)
        .ok_or_else(|| Error::fmt_msg("mnfx: no pixel chunk"))?;
    if pixel.codec == Codec::MnF8 || pixel.codec == Codec::MnF9 {
        // Embed payloads are the engines' native RGB8; other formats ride
        // the plane codecs (grammar rule 9).
        if hdr.pixel_format != PixelFormat::Rgb8 {
            return Err(Error::fmt_msg(format!(
                "mnfx: embed codec {} requires pixel_format 1 (RGB8), got {}",
                pixel.codec as u8, hdr.pixel_format as u8
            )));
        }
        // An embed payload is a whole standalone file: dispatch by magic,
        // and the magic must AGREE with the chunk's codec field (a codec-5
        // payload whose magic says MNF8 is corrupt, not a silent remap).
        let magic: &[u8] = pixel
            .stored
            .get(0..4)
            .ok_or_else(|| Error::fmt_msg("mnfx: pixel embed shorter than a magic (4 B)"))?;
        let want: &[u8] = if pixel.codec == Codec::MnF8 {
            b"MNF8"
        } else {
            manifold::mnf9::MAGIC.as_slice()
        };
        if magic != want {
            return Err(Error::fmt_msg(format!(
                "mnfx: pixel embed codec {} wants magic {} got {magic:02x?}",
                pixel.codec as u8,
                String::from_utf8_lossy(want)
            )));
        }
        // The codec field routed here; its payload is exactly that engine's
        // whole standalone file (magic already agrees above). The engine
        // takes borrowed bytes as-is (D2.2, no engine changes).
        if let Some(l) = limits {
            // W8-E2: the embed payload budget BEFORE the engine runs. For
            // MNF8 the caller's engine ceilings pass through to the
            // engine's own limits-aware decoder; MNF9 has no engine
            // counterpart — the budget here is the recorded proxy, and
            // the RECORDED limitation stands for its working memory.
            crate::limits::check_embed_input(pixel.stored.len(), l)?;
        }
        let (dims, rgb) = if pixel.codec == Codec::MnF8 {
            let decode = match limits {
                Some(l) => manifold::mnf8::decode_mnf8_with_limits(pixel.stored, &l.embed_engine),
                None => manifold::mnf8::decode_mnf8(pixel.stored),
            };
            let (d, px, _) =
                decode.map_err(|e| Error::fmt_msg(format!("mnfx: MNF8 embed decode: {e}")))?;
            (d, px)
        } else {
            manifold::mnf9::decode_mnf9(pixel.stored)
                .map_err(|e| Error::fmt_msg(format!("mnfx: MNF9 embed decode: {e}")))?
        };
        check_dims(hdr, dims.width, dims.height)?;
        if let Some(l) = limits {
            crate::limits::check_plane("embed", rgb.len() as u64, l)?;
        }
        return Ok((dims, Cow::Owned(plane_bytes(hdr, rgb)?)));
    }
    // Raw / Zstd: an interleaved byte plane, bound = stride × pixels
    // (grammar rule 4: raw_len == stored_len == plane length).
    let bound = plane_bound(hdr)?;
    match pixel.codec {
        Codec::Raw => {
            if pixel.stored.len() != bound || pixel.raw_len as usize != bound {
                return Err(Error::fmt_msg(format!(
                    "mnfx: raw pixel layer {} B != header-derived {} B",
                    pixel.stored.len(),
                    bound
                )));
            }
            // W8-E2: the plane bytes themselves are the output; only the
            // ceiling check runs (the backing is already zero-copy).
            if let Some(l) = limits {
                crate::limits::check_plane("raw", bound as u64, l)?;
            }
            // W8-D2 zero-copy: the raw plane IS the input's bytes.
            Ok((
                manifold::codec::Dims {
                    width: hdr.width,
                    height: hdr.height,
                },
                Cow::Borrowed(pixel.stored),
            ))
        }
        Codec::Zstd => {
            // W8-E2: enforce the decompression bound BEFORE the frame.
            if let Some(l) = limits {
                crate::limits::check_plane("zstd", bound as u64, l)?;
            }
            Ok((
                manifold::codec::Dims {
                    width: hdr.width,
                    height: hdr.height,
                },
                Cow::Owned(decompress_bounded(pixel.stored, bound)?),
            ))
        }
        _ => Err(Error::fmt_msg(
            "mnfx: non-embed pixel codec must be raw or zstd",
        )),
    }
}

/// The complete decoded image: pixels + the W4 metadata surface.
///
/// Semantics (grammar rule 12, arbitration corrected W8-A): color
/// precedence ICC > CICP (the PNG spec's own rule); orientation =
/// EXIF tag 0x0112 > ORNT chunk > header field, INDEPENDENT of chunk
/// order (ORNT mirrors the header field: producers keep them
/// consistent, the reader resolves ORNT as the header field's
/// expression, losing only to an EXIF tag). Samples are NEVER rotated
/// at decode — the container stores pixels unrotated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata<'a> {
    /// EXIF payload (preserved bytes; TIFF/HEIF structure per EXIF spec).
    pub exif: Option<&'a [u8]>,
    /// XMP packet (preserved bytes; RDF/XML).
    pub xmp: Option<&'a [u8]>,
    /// ICC profile (preserved bytes; color management's source of truth).
    pub icc: Option<&'a [u8]>,
    /// CICP signaling (preserved bytes).
    pub cicp: Option<&'a [u8]>,
    /// Orientation 1..=8, resolved EXIF tag 0x0112 > ORNT chunk >
    /// header field (grammar rule 12; independent of chunk order).
    pub orientation: u8,
}

/// Extract the metadata surface from chunk VIEWS (W8-D2: one
/// implementation over owned and borrowed chunks; the returned
/// `Metadata` borrows the payload bytes — original input for borrowed
/// callers, the owning chunks' payloads for owning callers; valid
/// lifetimes both ways).
///
/// Validated: ORNT payload must be one byte 1..=8; CICP exactly 4
/// bytes; EXIF must start with a byte-order marker — the minimal
/// preservation-boundary checks; full structure is the consumer's job.
///
/// Orientation arbitration (grammar rule 12, W8-A1): the EXIF tag and
/// the ORNT value are collected INDEPENDENTLY during iteration and
/// resolved ONCE after the loop — EXIF tag > ORNT > header field — so
/// the result never depends on chunk order.
fn validate_metadata_gen<'a, T: AsChunkView<'a>>(
    hdr: &ContainerHeader,
    chunks: &'a [T],
) -> Result<Metadata<'a>> {
    let views = views_of(chunks);
    let mut m = Metadata {
        exif: None,
        xmp: None,
        icc: None,
        cicp: None,
        orientation: hdr.orientation,
    };
    // Collected per source, resolved after iteration (W8-A1).
    let mut exif_tag: Option<u8> = None;
    let mut ornt: Option<u8> = None;
    for v in views {
        let p = v.stored;
        match v.chunk_type {
            ChunkType::Exif => {
                // EXIF starts "II"/2A00 or "MM"/002A (byte order + 42);
                // anything else will not parse for ANY consumer.
                let ok = p.len() >= 8
                    && matches!(&p[0..2], b"II" | b"MM")
                    && (p[2..4] == [42, 0] || p[2..4] == [0, 42]);
                if !ok {
                    return Err(Error::fmt_msg(
                        "mnfx: EXIF payload lacks a TIFF byte-order header",
                    ));
                }
                m.exif = Some(p);
                // EXIF orientation tag 0x0112 (SHORT, count 1); read
                // both TIFF byte orders. A payload without a well-formed
                // entry contributes nothing to the arbitration. Full IFD
                // walking is the consumer's job (ponytail: the container
                // only arbitrates precedence, not EXIF parsing).
                exif_tag = read_exif_orientation(p);
            }
            ChunkType::Xmp => m.xmp = Some(p),
            ChunkType::Icc => m.icc = Some(p),
            ChunkType::Cicp => {
                if p.len() != 4 {
                    return Err(Error::fmt_msg(format!(
                        "mnfx: CICP payload {} B != 4",
                        p.len()
                    )));
                }
                m.cicp = Some(p);
            }
            ChunkType::Orientation => {
                if p.len() != 1 || !(1..=8).contains(&p[0]) {
                    return Err(Error::fmt_msg(format!(
                        "mnfx: ORNT payload must be one byte 1..=8 (got {} B)",
                        p.len()
                    )));
                }
                ornt = Some(p[0]);
            }
            _ => {}
        }
    }
    m.orientation = exif_tag.or(ornt).unwrap_or(hdr.orientation);
    Ok(m)
}

/// Read EXIF's orientation tag (0x0112, type SHORT) by walking IFD0
/// entries in either TIFF byte order; None = tag absent or malformed.
fn read_exif_orientation(p: &[u8]) -> Option<u8> {
    let be = p[0..2] == *b"MM";
    let u16_at = |b: &[u8]| {
        if be {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        }
    };
    let u32_at = |b: &[u8]| {
        if be {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        }
    };
    let ifd = usize::try_from(u32_at(&p[4..8])).ok()?;
    let n = usize::from(u16_at(p.get(ifd..ifd + 2)?));
    for k in 0..n {
        let e = p.get(ifd + 2 + k * 12..ifd + 2 + (k + 1) * 12)?;
        let (tag, typ, cnt) = (u16_at(&e[0..2]), u16_at(&e[2..4]), u32_at(&e[4..8]));
        if tag == 0x0112 && typ == 3 && cnt == 1 {
            let v = if be {
                u16::from_be_bytes([e[8], e[9]])
            } else {
                u16::from_le_bytes([e[8], e[9]])
            };
            return ((1..=8).contains(&v)).then_some(v as u8);
        }
    }
    None
}

/// Decode a container fully: pixels + validated metadata (owning
/// wrapper over the shared W8-D2 implementation).
pub fn decode_image<'a>(
    hdr: &ContainerHeader,
    chunks: &'a [Chunk],
) -> Result<(Pixels, Metadata<'a>)> {
    let (dims, samples) = decode_pixels_gen(hdr, chunks, None)?;
    let meta = validate_metadata_gen(hdr, chunks)?;
    Ok((
        Pixels {
            dims,
            format: hdr.pixel_format,
            samples: samples.into_owned(),
        },
        meta,
    ))
}

/// W8-D2: the borrowed full image — raw planes stay zero-copy, metadata
/// borrows the ORIGINAL input bytes, and all lifetimes tie to the input.
/// W8-E2: limits-aware owning full image.
pub fn decode_image_limited<'a>(
    hdr: &ContainerHeader,
    chunks: &'a [Chunk],
    limits: &crate::limits::DecodeLimits,
) -> Result<(Pixels, Metadata<'a>)> {
    let (dims, samples) = decode_pixels_gen(hdr, chunks, Some(limits))?;
    let meta = validate_metadata_gen(hdr, chunks)?;
    Ok((
        Pixels {
            dims,
            format: hdr.pixel_format,
            samples: samples.into_owned(),
        },
        meta,
    ))
}

/// W8-E2: limits-aware borrowed full image (compose with
/// `parse_borrowed_limited` for the bounded full path).
pub fn decode_image_borrowed_limited<'a>(
    hdr: &ContainerHeader,
    chunks: &'a [BorrowedChunk<'a>],
    limits: &crate::limits::DecodeLimits,
) -> Result<(BorrowedPixels<'a>, Metadata<'a>)> {
    let (dims, samples) = decode_pixels_gen(hdr, chunks, Some(limits))?;
    let meta = validate_metadata_gen(hdr, chunks)?;
    Ok((
        BorrowedPixels {
            dims,
            format: hdr.pixel_format,
            samples,
        },
        meta,
    ))
}

pub fn decode_image_borrowed<'a>(
    hdr: &ContainerHeader,
    chunks: &'a [BorrowedChunk<'a>],
) -> Result<(BorrowedPixels<'a>, Metadata<'a>)> {
    let (dims, samples) = decode_pixels_gen(hdr, chunks, None)?;
    let meta = validate_metadata_gen(hdr, chunks)?;
    Ok((
        BorrowedPixels {
            dims,
            format: hdr.pixel_format,
            samples,
        },
        meta,
    ))
}

/// The exact byte length of the pixel plane: width × height × stride(format).
pub(crate) fn plane_bound(hdr: &ContainerHeader) -> Result<usize> {
    let w =
        usize::try_from(hdr.width).map_err(|_| Error::fmt_msg("mnfx: width overflows usize"))?;
    let h =
        usize::try_from(hdr.height).map_err(|_| Error::fmt_msg("mnfx: height overflows usize"))?;
    w.checked_mul(h)
        .and_then(|a| a.checked_mul(hdr.pixel_format.stride_mult()))
        .ok_or_else(|| Error::fmt_msg("mnfx: pixel area overflows usize"))
}

/// Decode a 16-bit container (W8-D2 view core): join PIX16-HI + PIX16-LO
/// into the interleaved big-endian sample plane (PNG wire order: hi
/// plane = each sample's first byte). Each plane chunk carries exactly
/// half the plane bound, per its own codec (raw/zstd; embed codecs stay
/// RGB8-only by rule 9, so plane chunks must not carry them). The
/// assembly is ALWAYS materialized (exact sample assembly, D2.2).
fn decode_planar16_view<'a, T: AsChunkView<'a>>(
    hdr: &ContainerHeader,
    chunks: &'a [T],
    limits: Option<&crate::limits::DecodeLimits>,
) -> Result<Vec<u8>> {
    let views = views_of(chunks);
    let half = plane_bound(hdr)? / 2;
    // W8-E2: the COMBINED plane consumption check, before the assembly.
    if let Some(l) = limits {
        crate::limits::check_plane("planar16-hi+lo", half as u64 * 2, l)?;
    }
    let mut planes: [Option<Vec<u8>>; 2] = [None, None];
    for c in views {
        let slot = match c.chunk_type {
            ChunkType::Pix16Hi => 0usize,
            ChunkType::Pix16Lo => 1usize,
            _ => continue,
        };
        if c.codec == Codec::MnF8 || c.codec == Codec::MnF9 {
            return Err(Error::fmt_msg(format!(
                "mnfx: 16-bit plane chunk {slot} must not carry an embed codec"
            )));
        }
        let raw = match c.codec {
            Codec::Raw => {
                if c.stored.len() != half || c.raw_len as usize != half {
                    return Err(Error::fmt_msg(format!(
                        "mnfx: 16-bit plane {slot} {} B != half-bound {} B",
                        c.stored.len(),
                        half
                    )));
                }
                c.stored.to_vec()
            }
            Codec::Zstd => decompress_bounded(c.stored, half)?,
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
        _ => {
            return Err(Error::fmt_msg(
                "mnfx: missing PIX16 hi/lo plane chunks (parser guarantees one each)",
            ))
        }
    };
    // Interleave: samples[n] = (hi[n], lo[n]) big-endian.
    let mut samples = Vec::with_capacity(hi.len() * 2);
    for i in 0..hi.len() {
        samples.push(hi[i]);
        samples.push(lo[i]);
    }
    Ok(samples)
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
        // W8-A2 (grammar rule 9 enforced at parse): a PIXEL embed under
        // a non-RGB8 header rejects at PARSE — pre-W8 the parser passed
        // it through to decode. Payload bytes are irrelevant to the
        // parse gate; the magic/routing check stays decode's job.
        let hdr = ContainerHeader {
            pixel_format: PixelFormat::Rgba8,
            ..hdr_for(8, 8)
        };
        let fake: &[u8] = b"MNF9\x00\x00\x00\x00";
        let buf = wrap_with(&hdr, &[(ChunkType::Pixel, Codec::MnF9, fake)]);
        assert!(parse(&buf).is_err());
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
        // W8-A2 (grammar rule 10 enforced at parse): an embed codec on a
        // PIX16 plane rejects at PARSE now; the decode-side guard
        // remains for callers that hand-build chunks (tested below).
        assert!(parse(&buf).is_err());
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

    // ---- W4: metadata preservation + semantics ----

    /// A minimal valid EXIF TIFF header (LE byte order, magic 42, one IFD
    /// offset) — just enough to pass the byte-order boundary check.
    fn exif_bytes() -> Vec<u8> {
        let mut v = b"II".to_vec();
        v.extend_from_slice(&42u16.to_le_bytes()); // the magic
        v.extend_from_slice(&8u32.to_le_bytes()); // first IFD offset
        v.extend_from_slice(&0u16.to_le_bytes()); // IFD entry count 0
        v
    }

    #[test]
    fn metadata_preserves_payloads_byte_for_byte() {
        // W4-A: a metadata-heavy container roundtrips every payload
        // byte-exact, in order, through parse + decode_image.
        let exif = exif_bytes();
        let xmp = b"<x:xmpmeta>packet-body-0xF00F</x:xmpmeta>".to_vec();
        let mut icc = vec![0u8; 132];
        icc[0..4].copy_from_slice(&128u32.to_be_bytes()); // profile size
        icc[36..40].copy_from_slice(b"mnfr"); // fake color-space sig
        let cicp = [9u8, 16, 0, 1]; // BT.2100 PQish
        let hdr = ContainerHeader {
            chunk_count: 6,
            ..hdr_for(8, 8)
        };
        let plane = vec![3u8; 8 * 8 * 3];
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &plane),
                (ChunkType::Exif, Codec::Raw, &exif),
                (ChunkType::Xmp, Codec::Raw, &xmp),
                (ChunkType::Icc, Codec::Raw, &icc),
                (ChunkType::Cicp, Codec::Raw, &cicp),
                (ChunkType::Unknown(0x0011), Codec::Raw, &[1, 2, 3]),
            ],
        );
        // Round-trip 1: bytes→parse→serialize == bytes (the lossless
        // rewrite contract, order preserved).
        let (h1, c1) = parse(&buf).unwrap();
        let re = serialize(
            &h1,
            &c1.iter()
                .map(|c| (c.header.chunk_type, c.header.codec, &*c.stored))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(re, buf, "parse+serialize must rewrite losslessly");
        // Round-trip 2: decode_image returns the exact payload references.
        let (px, meta) = decode_image(&h1, &c1).unwrap();
        assert_eq!(meta.exif, Some(&exif[..]));
        assert_eq!(meta.xmp, Some(&xmp[..]));
        assert_eq!(meta.icc, Some(&icc[..]));
        assert_eq!(meta.cicp, Some(&cicp[..]));
        assert_eq!(px.samples, plane);
    }

    #[test]
    fn parser_rejects_duplicate_metadata_chunks() {
        let plane = vec![3u8; 8 * 8 * 3];
        let exif = exif_bytes();
        let hdr = ContainerHeader {
            chunk_count: 3,
            ..hdr_for(8, 8)
        };
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &plane),
                (ChunkType::Exif, Codec::Raw, &exif),
                (ChunkType::Exif, Codec::Raw, &exif),
            ],
        );
        assert!(parse(&buf).is_err(), "duplicate EXIF must reject");
        // Same for ORNT.
        let hdr2 = ContainerHeader {
            chunk_count: 3,
            ..hdr_for(8, 8)
        };
        let buf = wrap_with(
            &hdr2,
            &[
                (ChunkType::Pixel, Codec::Raw, &plane),
                (ChunkType::Orientation, Codec::Raw, &[6]),
                (ChunkType::Orientation, Codec::Raw, &[7]),
            ],
        );
        assert!(parse(&buf).is_err(), "duplicate ORNT must reject");
    }

    #[test]
    fn orientation_exif_overrides_header_fallback() {
        // W4-B: EXIF orientation (big-endian tag 0x0112) wins; the
        // header field is the fallback.
        let plane = vec![3u8; 8 * 8 * 3];
        // MINIMAL EXIF with an orientation entry: IFD offset 8, 1 entry.
        let mut exif = b"II".to_vec();
        exif.extend_from_slice(&42u16.to_le_bytes());
        exif.extend_from_slice(&8u32.to_le_bytes());
        exif.extend_from_slice(&1u16.to_le_bytes()); // 1 IFD entry
        exif.extend_from_slice(&0x0112u16.to_le_bytes()); // tag
        exif.extend_from_slice(&3u16.to_le_bytes()); // type SHORT
        exif.extend_from_slice(&1u32.to_le_bytes()); // count
        exif.extend_from_slice(&6u16.to_le_bytes()); // value: 6
        exif.extend_from_slice(&0u16.to_le_bytes()); // pad
        let exif = exif; // ~26 B
                         // Header SAYS 1; the EXIF tag says 6 → 6 wins.
        let hdr = ContainerHeader {
            orientation: 1,
            chunk_count: 2,
            ..hdr_for(8, 8)
        };
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &plane),
                (ChunkType::Exif, Codec::Raw, &exif),
            ],
        );
        let (h, c) = parse(&buf).unwrap();
        let (_px, meta) = decode_image(&h, &c).unwrap();
        assert_eq!(meta.orientation, 6);
        // No EXIF chunk → the header field (or its default) applies.
        let hdr2 = ContainerHeader {
            orientation: 3,
            chunk_count: 1,
            ..hdr_for(8, 8)
        };
        let buf2 = wrap_with(&hdr2, &[(ChunkType::Pixel, Codec::Raw, &plane)]);
        let (h2, c2) = parse(&buf2).unwrap();
        let (_px, meta2) = decode_image(&h2, &c2).unwrap();
        assert_eq!(meta2.orientation, 3);
    }

    #[test]
    fn malformed_metadata_rejects() {
        let plane = vec![3u8; 8 * 8 * 3];
        let hdr = ContainerHeader {
            chunk_count: 2,
            ..hdr_for(8, 8)
        };
        // EXIF without a TIFF byte-order header: reject (it would be
        // garbage for every consumer).
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &plane),
                (ChunkType::Exif, Codec::Raw, b"not-exif"),
            ],
        );
        let (h, c) = parse(&buf).unwrap();
        assert!(decode_image(&h, &c).is_err());
        // CICP of the wrong length: reject.
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &plane),
                (ChunkType::Cicp, Codec::Raw, &[1, 2, 3]),
            ],
        );
        let (h, c) = parse(&buf).unwrap();
        assert!(decode_image(&h, &c).is_err());
        // ORNT out of 1..=8: reject (mirrors the header rule).
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &plane),
                (ChunkType::Orientation, Codec::Raw, &[9]),
            ],
        );
        let (h, c) = parse(&buf).unwrap();
        assert!(decode_image(&h, &c).is_err());
        // ORNT of zero bytes: reject.
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &plane),
                (ChunkType::Orientation, Codec::Raw, &[]),
            ],
        );
        let (h, c) = parse(&buf).unwrap();
        assert!(decode_image(&h, &c).is_err());
    }

    #[test]
    fn malformed_exif_short_payload_rejects() {
        let plane = vec![3u8; 8 * 8 * 3];
        let hdr = ContainerHeader {
            chunk_count: 2,
            ..hdr_for(8, 8)
        };
        let buf = wrap_with(
            &hdr,
            &[
                (ChunkType::Pixel, Codec::Raw, &plane),
                (ChunkType::Exif, Codec::Raw, b"II"),
            ],
        );
        let (h, c) = parse(&buf).unwrap();
        assert!(decode_image(&h, &c).is_err());
    }

    // ---- W8-A1: orientation arbitration (rule 12, order-independent) ----

    /// A minimal valid EXIF blob with ONE IFD0 entry: tag 0x0112
    /// (Orientation), type SHORT, count 1, value `orientation`, in
    /// either TIFF byte order.
    fn exif_with_orientation(be: bool, orientation: u8) -> Vec<u8> {
        fn num16(v: &mut Vec<u8>, x: u16, be: bool) {
            v.extend_from_slice(&if be { x.to_be_bytes() } else { x.to_le_bytes() });
        }
        fn num32(v: &mut Vec<u8>, x: u32, be: bool) {
            v.extend_from_slice(&if be { x.to_be_bytes() } else { x.to_le_bytes() });
        }
        let mut v: Vec<u8> = (if be { "MM" } else { "II" }).as_bytes().to_vec();
        num16(&mut v, 42, be);
        num32(&mut v, 8, be); // IFD0 offset
        num16(&mut v, 1, be); // one entry
        num16(&mut v, 0x0112, be); // the Orientation tag
        num16(&mut v, 3, be); // type SHORT
        num32(&mut v, 1, be); // count 1
        num16(&mut v, u16::from(orientation), be);
        num16(&mut v, 0, be); // pad the entry to 12 bytes
        v
    }

    /// Serialize an 8x8 RGB8 raw-plane container with an optional EXIF
    /// and/or ORNT chunk (header field = `header_orientation`); when
    /// both are present, `exif_first` picks the chunk order.
    fn meta_container(
        header_orientation: u8,
        exif: Option<&[u8]>,
        ornt: Option<u8>,
        exif_first: bool,
    ) -> Vec<u8> {
        let plane = vec![3u8; 8 * 8 * 3];
        let mut chunks: Vec<(ChunkType, Codec, Vec<u8>)> =
            vec![(ChunkType::Pixel, Codec::Raw, plane)];
        if let Some(e) = exif {
            chunks.push((ChunkType::Exif, Codec::Raw, e.to_vec()));
        }
        if let Some(o) = ornt {
            chunks.push((ChunkType::Orientation, Codec::Raw, vec![o]));
        }
        if !exif_first && exif.is_some() && ornt.is_some() {
            let n = chunks.len();
            chunks.swap(n - 2, n - 1);
        }
        let hdr = ContainerHeader {
            orientation: header_orientation,
            chunk_count: chunks.len() as u16,
            ..hdr_for(8, 8)
        };
        let refs: Vec<(ChunkType, Codec, &[u8])> = chunks
            .iter()
            .map(|(t, c, v)| (*t, *c, v.as_slice()))
            .collect();
        serialize(&hdr, &refs).unwrap()
    }

    fn decoded_orientation(buf: &[u8]) -> u8 {
        let (h, c) = parse(buf).unwrap();
        decode_image(&h, &c).unwrap().1.orientation
    }

    #[test]
    fn orientation_arbitration_matrix() {
        // The W8-A1 acceptance matrix (grammar rule 12, amended):
        // EXIF tag > ORNT > header field > default 1.
        let e6 = exif_with_orientation(false, 6);
        assert_eq!(
            decoded_orientation(&meta_container(4, Some(&e6), Some(3), true)),
            6
        ); // EXIF 6 wins over ORNT 3.
        assert_eq!(
            decoded_orientation(&meta_container(4, Some(&e6), None, true)),
            6
        );
        // ORNT mirrors the header field (no EXIF tag): 3 over header 4.
        assert_eq!(
            decoded_orientation(&meta_container(4, None, Some(3), true)),
            3
        );
        // Neither chunk: the header field stands.
        assert_eq!(decoded_orientation(&meta_container(4, None, None, true)), 4);
        // EXIF present WITHOUT an orientation tag: ORNT, else header.
        let bare = exif_bytes();
        assert_eq!(
            decoded_orientation(&meta_container(4, Some(&bare), Some(3), true)),
            3
        );
        assert_eq!(
            decoded_orientation(&meta_container(4, Some(&bare), None, true)),
            4
        );
        // An EXIF tag value outside 1..=8 is ignored (falls through).
        let bad = exif_with_orientation(false, 9);
        assert_eq!(
            decoded_orientation(&meta_container(4, Some(&bad), None, true)),
            4
        );
    }

    #[test]
    fn orientation_arbitration_ignores_chunk_order() {
        // The W8-A1 defect: pre-W8, an ORNT placed after a tagged EXIF
        // overrode it (last-chunk-wins). Both orders resolve to the
        // EXIF tag now, and the lossless rewrite still holds byte-for-byte.
        let e6 = exif_with_orientation(false, 6);
        for exif_first in [true, false] {
            let buf = meta_container(1, Some(&e6), Some(3), exif_first);
            assert_eq!(decoded_orientation(&buf), 6, "exif_first={exif_first}");
            let (h, c) = parse(&buf).unwrap();
            let re = serialize(
                &h,
                &c.iter()
                    .map(|c| (c.header.chunk_type, c.header.codec, &*c.stored))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(re, buf, "lossless rewrite, exif_first={exif_first}");
        }
    }

    #[test]
    fn orientation_exif_tag_reads_in_both_byte_orders() {
        // The W8-A1 fixtures must hold for "II" (LE) and "MM" (BE) shells.
        let le = exif_with_orientation(false, 5);
        let be = exif_with_orientation(true, 5);
        assert_eq!(
            decoded_orientation(&meta_container(1, Some(&le), None, true)),
            5
        );
        assert_eq!(
            decoded_orientation(&meta_container(1, Some(&be), None, true)),
            5
        );
    }

    #[test]
    fn decode_defensive_guards_still_reject_without_parse() {
        // Hand-built Chunk values bypass parse: the decode-side matrix
        // guards stay in force (defense in depth for direct callers).
        let (w, h) = (8u32, 8u32);
        let half = (w * h) as usize;
        let mk_chunk = |chunk_type: ChunkType, codec: Codec, payload: &[u8]| Chunk {
            header: ChunkHeader {
                chunk_type,
                codec,
                stored_len: payload.len() as u32,
                raw_len: if matches!(codec, Codec::Raw) {
                    payload.len() as u32
                } else {
                    0
                },
                crc: crc32c(payload),
                payload_offset: 0,
            },
            stored: payload.to_vec(),
        };
        // A PIX16 plane carrying an embed codec.
        let mut g16 = hdr_for(w, h);
        g16.pixel_format = PixelFormat::Gray16;
        let hi_bytes = vec![1u8; half];
        let lo_bytes = vec![2u8; half];
        let hi = mk_chunk(ChunkType::Pix16Hi, Codec::MnF8, &hi_bytes);
        let lo = mk_chunk(ChunkType::Pix16Lo, Codec::Raw, &lo_bytes);
        assert!(decode_pixels(&g16, &[hi, lo]).is_err());
        // A PIXEL embed under a non-RGB8 header.
        let mut rgba8 = hdr_for(w, h);
        rgba8.pixel_format = PixelFormat::Rgba8;
        let px_payload: &[u8] = b"MNF9\x00\x00\x00\x00";
        let px = mk_chunk(ChunkType::Pixel, Codec::MnF9, px_payload);
        assert!(decode_pixels(&rgba8, &[px]).is_err());
    }
}
