//! W8-E3: the checked zstd plane producer — the missing producer-side
//! convenience API using the EXISTING format vocabulary (an API
//! addition, not a new compression backend or wire representation).
//!
//! Contract: chunk types valid per format (PIXEL for the interleaved
//! formats, PIX16-HI/LO for the planar16 pair), sample length checked
//! against the format-derived plane bound, zstd checksum ENABLED, and
//! `raw_len` left ZERO by the frozen grammar (the serializer writes
//! zstd raw_len = 0). The chunk shapes are proven against the rule-13
//! codec matrix BEFORE they are returned, and the container helper
//! serializes through `serialize_checked` (producer contract).

use crate::error::{Error, Result};
use crate::{ChunkType, Codec, ContainerHeader, PixelFormat};

/// The frozen zstd framing: level 19 with the CRC checksum enabled
/// (W6 conformance vocabulary; the decoder reads any checksum frame).
fn zstd_frame(bytes: &[u8]) -> Vec<u8> {
    let mut e = zstd::stream::Encoder::new(Vec::new(), 19).expect("zstd encoder");
    e.include_checksum(true).expect("zstd checksum");
    std::io::Write::write_all(&mut e, bytes).expect("zstd write");
    e.finish().expect("zstd finish")
}

/// Produce zstd-coded chunk triplets for a whole pixel plane.
///
/// Supported shapes: the interleaved formats (one raw PIXEL chunk
/// equivalent, zstd-coded) and the planar16 formats (the PIX16-HI/LO
/// pair, each half zstd-coded from the interleaved plane). The plane
/// length is checked against the format-derived bound
/// (`width × height × stride`) exactly; the returned shapes pass the
/// rule-13 codec matrix (proven, not assumed).
pub fn zstd_plane_chunks(
    plane: &[u8],
    fmt: PixelFormat,
    width: u32,
    height: u32,
) -> Result<Vec<(ChunkType, Codec, Vec<u8>)>> {
    let want = crate::limits::plane_bound_u64(fmt, width, height)?;
    let want_usize =
        usize::try_from(want).map_err(|_| Error::fmt_msg("mnfx: plane bound overflows usize"))?;
    if plane.len() != want_usize {
        return Err(Error::fmt_msg(format!(
            "mnfx: plane is {} B; format-derived bound for {fmt:?} {width}x{height} is {want} B",
            plane.len()
        )));
    }
    if fmt.is_planar16() {
        // The hi/lo split: PNG wire order (hi = each sample's first
        // byte), matching the decode assembly exactly.
        let half = plane.len() / 2;
        let (hi, lo) = plane.split_at(half);
        // rule-13 proof for the pair, through the same validator the
        // serializer uses.
        crate::parser::validate_codec_compatibility(ChunkType::Pix16Hi, Codec::Zstd, fmt)?;
        crate::parser::validate_codec_compatibility(ChunkType::Pix16Lo, Codec::Zstd, fmt)?;
        Ok(vec![
            (ChunkType::Pix16Hi, Codec::Zstd, zstd_frame(hi)),
            (ChunkType::Pix16Lo, Codec::Zstd, zstd_frame(lo)),
        ])
    } else {
        crate::parser::validate_codec_compatibility(ChunkType::Pixel, Codec::Zstd, fmt)?;
        Ok(vec![(ChunkType::Pixel, Codec::Zstd, zstd_frame(plane))])
    }
}

/// The convenience producer: checked-checked — the chunks round-trip
/// through `serialize_checked` (rule-13 + ORNT-mirrors-header proof at
/// write time) and the result is a complete MNFX v1 container.
pub fn zstd_plane_container(
    plane: &[u8],
    fmt: PixelFormat,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let chunks = zstd_plane_chunks(plane, fmt, width, height)?;
    let hdr = ContainerHeader {
        pixel_format: fmt,
        orientation: 1,
        chunk_count: chunks.len() as u16,
        width,
        height,
    };
    // The serializer's chunk triplet carries borrowed payload slices;
    // the produced frames are owned Vec<u8>, so borrow them for the
    // checked write.
    let refs: Vec<(ChunkType, Codec, &[u8])> = chunks
        .iter()
        .map(|(chunk_type, codec, payload)| (*chunk_type, *codec, payload.as_slice()))
        .collect();
    crate::parser::serialize_checked(&hdr, &refs)
}
