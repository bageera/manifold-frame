//! C FFI for the MNFX container (manifold-frame, W5-D).
//!
//! Contract (the manifold-capi precedent): no panics across the
//! boundary — every fallible call returns a `MNFX_STATUS_*` code;
//! outputs are written through caller-owned buffers; the size-query
//! idiom (call with out = NULL first) reports the required size. The
//! crate exports no allocator helpers on purpose: callers own every
//! byte, so cross-language memory ownership stays unambiguous.

// Status codes (stable ABI: values fixed by C header contract).
pub const STATUS_OK: i32 = 0;
pub const STATUS_NULL_ARG: i32 = -1;
pub const STATUS_BUF_TOO_SMALL: i32 = -2;
pub const STATUS_FORMAT_INVALID: i32 = -3;
pub const STATUS_INTERNAL: i32 = -7;

use manifold_frame::{parse, Chunk, ContainerHeader};

/// Map the frame error onto a stable status code. Every
/// `manifold_frame::Error` is a format violation (the parser's only
/// variant), so the mapping is total — INTERNAL stays reserved for a
/// bug caught by a defensive check, never for expected input.
fn status(_: &manifold_frame::Error) -> i32 {
    STATUS_FORMAT_INVALID
}

/// Copy `bytes` into the caller buffer (the manifold-capi `copy_out`):
/// null or too-small out = BUF_TOO_SMALL with the required size
/// written back; otherwise the copy plus the written length.
unsafe fn copy_out(bytes: &[u8], out: *mut u8, out_len: *mut usize) -> i32 {
    let need = bytes.len();
    if out.is_null() || *out_len < need {
        *out_len = need;
        return STATUS_BUF_TOO_SMALL;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, need);
    *out_len = need;
    STATUS_OK
}

/// Decode any MNFX container to its full interleaved sample plane.
/// Query pass: call with `out = NULL`; the required size is written
/// through `out_len`, and width/height — plus pixel_format/orientation
/// (when their pointers are non-null) — always receive the header
/// values on OK and BUF_TOO_SMALL. The header's orientation field is
/// the raw wire value (rule-12 EXIF precedence is a consumer read).
///
/// # Safety
/// `buf` must point to `len` readable bytes; `out` (if non-null) to
/// `*out_len` writable bytes; every non-null out pointer must be
/// writable. Never panics on caller data (the parser + decoders are
/// panic-free by construction and exercised by the mutation fuzzer).
#[no_mangle]
pub unsafe extern "C" fn mnfx_decode(
    buf: *const u8,
    len: usize,
    out: *mut u8,
    out_len: *mut usize,
    width: *mut u32,
    height: *mut u32,
    pixel_format: *mut u8,
    orientation: *mut u8,
) -> i32 {
    if buf.is_null() || out_len.is_null() || width.is_null() || height.is_null() {
        return STATUS_NULL_ARG;
    }
    let data = std::slice::from_raw_parts(buf, len);
    let (hdr, chunks) = match parse(data) {
        Ok(v) => v,
        Err(e) => return status(&e),
    };
    write_header_wide(&hdr, width, height, pixel_format, orientation);
    // The full plane: embedded formats decode through the engines;
    // raw/zstd planes are checked against the header-derived bound.
    let plane = match decode_plane(&hdr, &chunks) {
        Ok(p) => p,
        Err(e) => return status(&e),
    };
    copy_out(&plane, out, out_len)
}

/// Rewrite one chunk's payload in place, preserving every other byte
/// of the container (the lossless-rewrite contract). The CRC32C over
/// the stored payload is recomputed; a caller CRC input is not taken.
///
/// # Safety
/// `buf` = `len` readable bytes; `new_payload` = `new_len` readable
/// bytes when `new_len > 0` (null + 0 is rejected as NULL_ARG);
/// `out` (if non-null) = `*out_len` writable bytes. Never panics.
#[no_mangle]
pub unsafe extern "C" fn mnfx_rewrite_chunk(
    buf: *const u8,
    len: usize,
    chunk_index: u32,
    new_payload: *const u8,
    new_len: usize,
    out: *mut u8,
    out_len: *mut usize,
) -> i32 {
    if buf.is_null() || out_len.is_null() || (new_payload.is_null() && new_len != 0) {
        return STATUS_NULL_ARG;
    }
    let data = std::slice::from_raw_parts(buf, len);
    let payload: &[u8] = if new_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(new_payload, new_len)
    };
    let rewritten = match rewrite_chunk(data, chunk_index, payload) {
        Ok(v) => v,
        Err(e) => return status(&e),
    };
    copy_out(&rewritten, out, out_len)
}

/// The header-field writes shared by both status outcomes of the
/// decode query pass (dims/pixfmt/orientation are usable even when
/// the caller's plane buffer was too small).
unsafe fn write_header_wide(
    hdr: &ContainerHeader,
    width: *mut u32,
    height: *mut u32,
    pixel_format: *mut u8,
    orientation: *mut u8,
) {
    *width = hdr.width;
    *height = hdr.height;
    if !pixel_format.is_null() {
        *pixel_format = hdr.pixel_format as u8;
    }
    if !orientation.is_null() {
        *orientation = hdr.orientation;
    }
}

// ---- the shared Rust cores (unit-testable without raw pointers) ----

fn decode_plane(hdr: &ContainerHeader, chunks: &[Chunk]) -> Result<Vec<u8>, manifold_frame::Error> {
    let px = manifold_frame::decode::decode_pixels(hdr, chunks)?;
    Ok(px.samples)
}

/// Rewrite one chunk's payload in place, preserving every other byte
/// of the container (the lossless-rewrite contract). The CRC32C over
/// each stored payload is recomputed; a caller CRC input is not taken.
///
/// The rebuilt chunk header follows the new payload (raw chunks
/// re-declare raw_len = stored_len; embed/zstd stay 0), so a
/// shape-inconsistent RESULT (e.g. a resized raw pixel plane that no
/// longer equals the container's plane bound) parses fine but is
/// rejected by the bound checks at decode — the smoke pins that.
///
/// Returns `Err` when the container does not parse, the index is out
/// of range, or a payload length exceeds u32 stored_len.
fn rewrite_chunk(
    buf: &[u8],
    chunk_index: u32,
    new_payload: &[u8],
) -> Result<Vec<u8>, manifold_frame::Error> {
    use manifold_frame::{crc32c, Codec};
    let (hdr, chunks) = parse(buf)?;
    let idx = usize::try_from(chunk_index)
        .ok()
        .filter(|&i| i < chunks.len())
        .ok_or_else(|| {
            manifold_frame::Error::fmt_msg(format!(
                "mnfx: rewrite index {chunk_index} >= chunk_count {}",
                chunks.len()
            ))
        })?;
    // Rebuild EVERY chunk: index i's payload is replaced, its CRC is
    // recomputed; all other chunks (payload + header) re-serialize
    // byte-identically (the W4 rewrite contract, exercised by
    // tests/meta_differential.rs).
    let mut out = Vec::with_capacity(buf.len() + new_payload.len());
    out.extend_from_slice(&manifold_frame::MAGIC);
    out.extend_from_slice(&manifold_frame::VERSION.to_le_bytes());
    out.push(hdr.pixel_format as u8);
    out.push(hdr.orientation);
    out.extend_from_slice(&hdr.chunk_count.to_le_bytes());
    out.extend_from_slice(&hdr.width.to_le_bytes());
    out.extend_from_slice(&hdr.height.to_le_bytes());
    let mut header_buf = [0u8; 16];
    for (i, c) in chunks.iter().enumerate() {
        let payload_i: &[u8] = if i == idx { new_payload } else { &c.stored };
        // stored_len must fit u32: the replaced payload is checked here;
        // unchanged chunks already carried u32 wire lengths.
        let stored_len_u32 = u32::try_from(payload_i.len()).map_err(|_| {
            manifold_frame::Error::fmt_msg("mnfx: rewrite payload over u32 stored_len")
        })?;
        let raw_len = match c.header.codec {
            Codec::Raw => stored_len_u32, // rule 4: raw_len == stored_len
            _ => 0,                       // embed + zstd chunks declare 0
        };
        header_buf[0..2].copy_from_slice(&c.header.chunk_type.raw().to_le_bytes());
        header_buf[2] = c.header.codec as u8;
        header_buf[3] = 0; // flags reserved
        header_buf[4..8].copy_from_slice(&raw_len.to_le_bytes());
        header_buf[8..12].copy_from_slice(&stored_len_u32.to_le_bytes());
        header_buf[12..16].copy_from_slice(&crc32c(payload_i).to_le_bytes());
        out.extend_from_slice(&header_buf);
        out.extend_from_slice(payload_i);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use manifold_frame::Codec;

    /// A raw RGB8 container (the smoke's shape), built through the
    /// public lib API.
    fn raw_container() -> Vec<u8> {
        let hdr = ContainerHeader {
            pixel_format: manifold_frame::PixelFormat::Rgb8,
            orientation: 1,
            chunk_count: 1,
            width: 8,
            height: 8,
        };
        let plane: Vec<u8> = (0..8 * 8 * 3).map(|i| (i * 7 % 256) as u8).collect();
        manifold_frame::serialize(
            &hdr,
            &[(manifold_frame::ChunkType::Pixel, Codec::Raw, &plane)],
        )
        .unwrap()
    }

    #[test]
    fn rewrite_core_same_length_roundtrips() {
        let buf = raw_container();
        let newp: Vec<u8> = (0..8 * 8 * 3).map(|i| (255 - i % 251) as u8).collect();
        let out = rewrite_chunk(&buf, 0, &newp).unwrap();
        assert_eq!(out.len(), buf.len());
        assert_eq!(
            &out[..30],
            &buf[..30],
            "headers through stored_len preserved"
        );
        assert_eq!(&out[34..], &newp[..], "payload replaced");
        // the rewritten container parses + decodes to the NEW plane
        let (h, chunks) = parse(&out).unwrap();
        let px = decode_plane(&h, &chunks).unwrap();
        assert_eq!(px, newp);
    }

    #[test]
    fn rewrite_core_index_and_len_reject() {
        let buf = raw_container();
        assert!(rewrite_chunk(&buf, 1, &[0u8; 4]).is_err());
        // a 5-byte payload for a raw chunk would break raw_len ==
        // stored_len == payload (rule 4): the rewrite API re-serializes
        // the chunk header with the new shape, so raw_len follows the
        // payload — but decode_pixels then rejects the bound mismatch.
        let out = rewrite_chunk(&buf, 0, &[9u8; 5]).unwrap();
        let (h, chunks) = parse(&out).unwrap();
        assert!(decode_plane(&h, &chunks).is_err(), "bound mismatch");
    }

    #[test]
    fn decode_core_accepts_and_rejects() {
        let buf = raw_container();
        let (h, chunks) = parse(&buf).unwrap();
        assert_eq!(decode_plane(&h, &chunks).unwrap().len(), 8 * 8 * 3);
        assert!(parse("NOPE".as_bytes()).is_err());
    }
}
