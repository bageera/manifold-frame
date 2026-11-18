//! W8-E2: resource limits for untrusted MNFX input (additive; the
//! acceptance semantics of the unlimited readers are unchanged).
//!
//! Enforcement points, all BEFORE the matching allocations:
//! - header fields (width/height caps, pixel count, declared chunk count)
//! - the chunk walk (walked chunk count, metadata aggregate)
//! - decode (plane bound exact for raw, the zstd decompression bound,
//!   planar16 hi+lo aggregate, and the embed payload budget)
//!
//! Embedded-stream working memory: MNF8 embeds pass the caller's
//! `embed_engine` ceilings through to the engine's own limits-aware
//! decoder (`decode_mnf8_with_limits`), so engine-internal memory is
//! bounded. MNF9 embeds: the engine API (`decode_mnf9`) is not
//! limits-aware — the frame enforces the payload-bytes budget
//! (`max_embed_input_bytes`) before invoking it, bounds the decoded
//! dims after, and RECORDS this as a limitation rather than a full
//! working-memory guarantee.

use crate::error::{Error, Result};
use crate::{ContainerHeader, PixelFormat};

#[derive(Debug, Clone, Copy)]
pub struct DecodeLimits {
    /// Header-field cap: width.
    pub max_width: u32,
    /// Header-field cap: height.
    pub max_height: u32,
    /// Pixel-count cap: width × height.
    pub max_pixels: u64,
    /// Aggregate decoded-plane cap: the format-derived plane bound
    /// (exact for raw and zstd's decompression bound, hi+lo aggregate
    /// for planar16; for embeds the post-decode plane size, enforced
    /// post-hoc by check_dims).
    pub max_decoded_bytes: u64,
    /// Per-embed stored-payload cap: the bytes handed to the engine
    /// (the MNF9 exposure budget; MNF8 additionally passes through the
    /// engine ceilings below).
    pub max_embed_input_bytes: u64,
    /// Aggregate EXIF/XMP/ICC/CICP/ORNT stored-payload bytes.
    pub max_metadata_bytes: u64,
    /// Chunk-count cap (the header's declared count AND the walked
    /// count).
    pub max_chunks: usize,
    /// Pass-through ceilings for MNF8 embeds (engine-internal working
    /// memory: pixels, tiles, decompressed, atlas — see the codec's
    /// `DecodeLimits`). MNF9 has no engine counterpart (limitation
    /// recorded in the module docs).
    pub embed_engine: manifold::codec::DecodeLimits,
}

impl Default for DecodeLimits {
    /// Frame ceilings open (the geometry the v1 grammar itself admits):
    /// the engine limits default to the codec's own conservative
    /// defaults, which stay ACTIVE for embeds unless relaxed here.
    fn default() -> Self {
        DecodeLimits {
            max_width: u32::MAX,
            max_height: u32::MAX,
            max_pixels: u64::MAX,
            max_decoded_bytes: u64::MAX,
            max_embed_input_bytes: u64::MAX,
            max_metadata_bytes: u64::MAX,
            max_chunks: usize::MAX,
            embed_engine: manifold::codec::DecodeLimits::default(),
        }
    }
}

/// The walked-chunk-count ceiling error (parser-side helper).
pub fn max_chunks(actual: u64, limit: usize) -> Error {
    Error::fmt_msg(format!(
        "mnfx: resource limit (max_chunks): {actual} > {limit}"
    ))
}

/// The metadata-aggregate ceiling error (parser-side helper).
pub fn max_metadata(actual: u64, limit: u64) -> Error {
    Error::fmt_msg(format!(
        "mnfx: resource limit (max_metadata_bytes): {actual} > {limit}"
    ))
}

fn limit(which: &str, limit: u64, actual: u64) -> Error {
    Error::fmt_msg(format!(
        "mnfx: resource limit ({which}): {actual} > {limit}"
    ))
}

/// Header-stage checks, run immediately after the header is parsed and
/// before any chunk walk or allocation.
pub fn check_header(hdr: &ContainerHeader, lim: &DecodeLimits) -> Result<()> {
    if hdr.width > lim.max_width {
        return Err(limit(
            "max_width",
            u64::from(lim.max_width),
            u64::from(hdr.width),
        ));
    }
    if hdr.height > lim.max_height {
        return Err(limit(
            "max_height",
            u64::from(lim.max_height),
            u64::from(hdr.height),
        ));
    }
    let pixels = u64::from(hdr.width) * u64::from(hdr.height);
    if pixels > lim.max_pixels {
        return Err(limit("max_pixels", lim.max_pixels, pixels));
    }
    if usize::from(hdr.chunk_count) > lim.max_chunks {
        return Err(limit(
            "max_chunks",
            lim.max_chunks as u64,
            u64::from(hdr.chunk_count),
        ));
    }
    Ok(())
}

/// Decode-stage plane checks, run before the corresponding allocation
/// or decompression.
pub fn check_plane(kind: &str, plane_bytes: u64, lim: &DecodeLimits) -> Result<()> {
    if plane_bytes > lim.max_decoded_bytes {
        return Err(limit(
            &format!("max_decoded_bytes ({kind})"),
            lim.max_decoded_bytes,
            plane_bytes,
        ));
    }
    Ok(())
}

/// Per-embed input check, run before the engine sees the bytes.
pub fn check_embed_input(stored_len: usize, lim: &DecodeLimits) -> Result<()> {
    if stored_len as u64 > lim.max_embed_input_bytes {
        return Err(limit(
            "max_embed_input_bytes",
            lim.max_embed_input_bytes,
            stored_len as u64,
        ));
    }
    Ok(())
}

/// Plane bound helper shared with the decode core (stride-derived).
pub fn plane_bound_u64(fmt: PixelFormat, width: u32, height: u32) -> Result<u64> {
    let stride = fmt.stride_mult() as u64;
    u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|area| area.checked_mul(stride))
        .ok_or_else(|| Error::fmt_msg("mnfx: pixel area overflows usize"))
}
