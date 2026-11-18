//! MNFX — the **manifold-frame** container (W0: specification + bounded
//! parser).
//!
//! A versioned lossless image container wrapping Manifold compression
//! streams (ADR-009 + docs/adr-009-mnfx-review.md). The wrapper adds
//! image semantics (alpha, 16-bit samples, metadata, orientation); it
//! never modifies the compression engines — an MNF8/MNF9 payload
//! embedded under MNFX decodes byte-identically to its standalone
//! decode (the W1 exit gate).
//!
//! W8-E1: **no unsafe code may enter the container crate** — the
//! parser, decoders, serializer and CRC are safe Rust by construction;
//! raw-pointer work belongs to the `capi` crate (which scopes each
//! unsafe block explicitly).
//!
// W8-E1: forbidden (not denied) — a future unsafe block must be an
// explicit, reviewed opt-OUT of this line.
#![forbid(unsafe_code)]

//! # Wire grammar (v1, W0-frozen)
//!
//! Container header (18 bytes):
//! ```text
//! magic 4 "MNFX" | version u16 LE | pixel_format u8 | orientation u8
//! chunk_count u16 LE | width u32 LE | height u32 LE
//! ```
//! Chunk header (16 bytes):
//! ```text
//! type u16 LE (bit 0x8000 = critical) | codec u8 | flags u8 (=0)
//! raw_len u32 LE | stored_len u32 LE | crc32c u32 LE (stored bytes)
//! ```
//! Rules (normative): bounds derive from the file length; CRC32C is
//! validated BEFORE parse; unknown non-critical chunks skip, unknown
//! critical chunks reject; exactly one PIXEL chunk; the rule-13
//! codec/chunk compatibility matrix (embed codecs on the RGB8 pixel
//! layer only, PIX16 planes raw/zstd, metadata raw-coded); malformed
//! input = Err, never panic.

/// W1: pixel-layer decode (embedded == standalone gate).
pub mod decode;
/// W8-E3: the checked zstd plane producer (additive API).
pub mod encode;
pub mod error;
/// W8-E2: resource limits for untrusted input (additive API).
pub mod limits;
/// The bounded container parser.
pub mod parser;

/// Container magic: `MNFX`.
pub const MAGIC: [u8; 4] = *b"MNFX";
/// Container grammar version 1.
pub const VERSION: u16 = 1;
/// Fixed container header size (bytes).
pub const HEADER_SIZE: usize = 18;
/// Fixed chunk header size (bytes).
pub const CHUNK_HEADER_SIZE: usize = 16;

/// Pixel sample formats (v1 vocabulary; grows per phase W2/W3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PixelFormat {
    /// One RGB8 plane (the MNF8/MNF9 native shape).
    Rgb8 = 1,
    /// One RGBA8 plane (alpha; hidden RGB preserved per grammar rule 8).
    Rgba8 = 2,
    /// One Gray8 plane.
    Gray8 = 3,
    /// One GrayA8 plane.
    GrayA8 = 4,
    /// 16-bit RGB: ONE PIX16-HI chunk + ONE PIX16-LO chunk (the
    /// alphabet-relayout split). Samples decode big-endian interleaved
    /// (PNG wire order): hi plane = each sample's first byte.
    Rgb16 = 5,
    /// 16-bit Gray: the same hi/lo plane split, 1 sample/pixel.
    Gray16 = 6,
}

impl PixelFormat {
    /// Total pixel bytes per pixel: 3 RGB8, 4 RGBA8, 1 Gray8, 2 GrayA8,
    /// 6 RGB16 (2 planes × 3), 2 Gray16. The plane bound derives from
    /// this (never a remembered constant — the fixture-geometry lesson);
    /// 16-bit plane chunks each carry exactly half.
    pub fn stride_mult(self) -> usize {
        match self {
            PixelFormat::Rgb8 => 3,
            PixelFormat::Rgba8 => 4,
            PixelFormat::Gray8 => 1,
            PixelFormat::GrayA8 => 2,
            PixelFormat::Rgb16 => 6,
            PixelFormat::Gray16 => 2,
        }
    }

    /// True for the 16-bit hi/lo-plane formats.
    pub fn is_planar16(self) -> bool {
        matches!(self, PixelFormat::Rgb16 | PixelFormat::Gray16)
    }
}

impl std::convert::TryFrom<u8> for PixelFormat {
    type Error = ();

    fn try_from(v: u8) -> core::result::Result<Self, Self::Error> {
        match v {
            1 => Ok(PixelFormat::Rgb8),
            2 => Ok(PixelFormat::Rgba8),
            3 => Ok(PixelFormat::Gray8),
            4 => Ok(PixelFormat::GrayA8),
            5 => Ok(PixelFormat::Rgb16),
            6 => Ok(PixelFormat::Gray16),
            _ => Err(()),
        }
    }
}

/// Compression codec for one chunk's payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Codec {
    /// Bytes stored verbatim.
    Raw = 0,
    /// An MNF8 file embedded whole (decoded via manifold-codec).
    MnF8 = 4,
    /// An MNF9 file embedded whole (decoded via manifold-codec).
    MnF9 = 5,
    /// A zstd frame (checksum mandatory per the MNF7 precedent).
    Zstd = 6,
}

impl std::convert::TryFrom<u8> for Codec {
    type Error = ();

    fn try_from(v: u8) -> core::result::Result<Self, Self::Error> {
        match v {
            0 => Ok(Codec::Raw),
            4 => Ok(Codec::MnF8),
            5 => Ok(Codec::MnF9),
            6 => Ok(Codec::Zstd),
            _ => Err(()),
        }
    }
}

/// Chunk types. Values >= 0x8000 carry the critical bit: unknown
/// critical chunks reject; unknown non-critical chunks skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ChunkType {
    /// The pixel layer (exactly one per container).
    Pixel = 1,
    /// EXIF payload, preserved byte-for-byte.
    Exif = 2,
    /// XMP payload, preserved byte-for-byte.
    Xmp = 3,
    /// ICC profile, preserved raw (never replaced with sRGB).
    Icc = 4,
    /// CICP color signaling.
    Cicp = 5,
    /// Display orientation override (1..=8).
    Orientation = 6,
    /// 16-bit HIGH byte plane (W3; one per 16-bit container).
    Pix16Hi = 7,
    /// 16-bit LOW byte plane (W3; one per 16-bit container).
    Pix16Lo = 8,
    /// Unknown/extension types.
    Unknown(u16),
}

impl ChunkType {
    fn from_raw(v: u16) -> ChunkType {
        match v {
            1 => ChunkType::Pixel,
            2 => ChunkType::Exif,
            3 => ChunkType::Xmp,
            4 => ChunkType::Icc,
            5 => ChunkType::Cicp,
            6 => ChunkType::Orientation,
            7 => ChunkType::Pix16Hi,
            8 => ChunkType::Pix16Lo,
            other => ChunkType::Unknown(other),
        }
    }

    /// The wire u16 (W5-D: public for the C-ABI re-serializer).
    pub fn raw(self) -> u16 {
        match self {
            ChunkType::Pixel => 1,
            ChunkType::Exif => 2,
            ChunkType::Xmp => 3,
            ChunkType::Icc => 4,
            ChunkType::Cicp => 5,
            ChunkType::Orientation => 6,
            ChunkType::Pix16Hi => 7,
            ChunkType::Pix16Lo => 8,
            ChunkType::Unknown(v) => v,
        }
    }

    /// The critical bit (0x8000).
    pub fn is_critical(self) -> bool {
        self.raw() & 0x8000 != 0
    }
}

/// A parsed chunk header (payload bytes follow).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkHeader {
    /// Chunk type.
    pub chunk_type: ChunkType,
    /// Payload codec.
    pub codec: Codec,
    /// Stored (wire) payload length.
    pub stored_len: u32,
    /// Decoded (logical) payload length — the decode bound.
    pub raw_len: u32,
    /// CRC32C over the stored bytes.
    pub crc: u32,
    /// File offset of the payload (absolute).
    pub payload_offset: usize,
}

/// The container header, parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerHeader {
    /// Sample format (v1 = RGB8).
    pub pixel_format: PixelFormat,
    /// Display orientation (1..=8; samples are stored unrotated).
    pub orientation: u8,
    /// Declared chunk count (the walk must match exactly).
    pub chunk_count: u16,
    /// Image width.
    pub width: u32,
    /// Image height.
    pub height: u32,
}

/// CRC32C (Castagnoli, reflected 0x82F63B78), W8-D1: **slicing-by-16** —
/// 16 KiB of const-evaluated tables, safe Rust, dependency-free. The
/// candidate matrix and e2e attribution live in `tests/w8d1_probe.rs`
/// (bitwise ≈ 0.45 GiB/s; slice16 ≈ 2.9–3.5 GiB/s; parse of a 16 MiB raw
/// plane 58.0 ms → ~10 ms, the CRC previously ~90 % of parse). The
/// promotion gates: RFC-3720-style known-answer vectors (incl.
/// `123456789` → `0xE3069283`), randomized offset/length/trailing parity
/// against the retained bitwise oracle, and wire-identical serialized
/// files (all downstream CRC assertions unchanged).
const POLY: u32 = 0x82F63B78;

const fn table0() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            j += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

const fn tables16() -> [[u32; 256]; 16] {
    let mut t = [[0u32; 256]; 16];
    t[0] = table0();
    let mut n = 1;
    while n < 16 {
        let mut i = 0;
        while i < 256 {
            t[n][i] = (t[n - 1][i] >> 8) ^ t[0][(t[n - 1][i] & 0xFF) as usize];
            i += 1;
        }
        n += 1;
    }
    t
}

static T16: [[u32; 256]; 16] = tables16();

/// The retained independent correctness oracle (bitwise; test builds only);
/// the W8-D1 promotion gates diff every candidate against exactly this.
#[cfg(test)]
fn crc32c_bitwise(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// CRC32C (Castagnoli, reflected polynomial 0x1EDC6F41); slicing-by-16
/// above — the wire values are unchanged. Table-free only in the sense
/// the CRC32C table derives at compile time; see the retained bitwise
/// oracle for the pre-W8-D1 reference algorithm.
pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let (chunks, remainder) = data.as_chunks::<16>();
    for c in chunks {
        crc ^= u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        crc = T16[15][(crc & 0xFF) as usize]
            ^ T16[14][((crc >> 8) & 0xFF) as usize]
            ^ T16[13][((crc >> 16) & 0xFF) as usize]
            ^ T16[12][((crc >> 24) & 0xFF) as usize]
            ^ T16[11][c[4] as usize]
            ^ T16[10][c[5] as usize]
            ^ T16[9][c[6] as usize]
            ^ T16[8][c[7] as usize]
            ^ T16[7][c[8] as usize]
            ^ T16[6][c[9] as usize]
            ^ T16[5][c[10] as usize]
            ^ T16[4][c[11] as usize]
            ^ T16[3][c[12] as usize]
            ^ T16[2][c[13] as usize]
            ^ T16[1][c[14] as usize]
            ^ T16[0][c[15] as usize];
    }
    for &b in remainder {
        crc = (crc >> 8) ^ T16[0][(crc ^ u32::from(b)) as usize & 0xFF];
    }
    !crc
}

pub use crate::decode::{
    decode_image, decode_image_borrowed, decode_image_borrowed_limited, decode_image_limited,
    decode_pixels, decode_pixels_borrowed, decode_pixels_borrowed_limited, decode_pixels_limited,
    BorrowedPixels, Metadata,
};
pub use crate::error::Error;
pub use crate::limits::{check_embed_input, check_header, check_plane, DecodeLimits};
pub use crate::parser::{
    parse, parse_borrowed, parse_borrowed_limited, parse_limited, serialize, serialize_checked,
    validate_codec_compatibility, BorrowedChunk, Chunk,
};

#[cfg(test)]
mod crc32c_w8d1_parity {
    use super::*;

    // Promotion gate: randomized lengths, alignment offsets and
    // trailing-byte coverage — the shipped slicing implementation must
    // equal the bitwise oracle at every offset, never just in bulk.
    #[test]
    fn shipped_slice16_matches_bitwise_oracle_randomized() {
        let poly_seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut s = poly_seed;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let blob_len = 70_000;
        let mut blob = vec![0u8; blob_len];
        let mut t: u32 = 0x2545_F491;
        for b in blob.iter_mut() {
            t ^= t << 13;
            t ^= t >> 17;
            t ^= t << 5;
            *b = (t >> 24) as u8;
        }
        for _ in 0..300 {
            let off = (next() % 16) as usize;
            let len = 1 + ((next() as usize) % 4096);
            let len = len.min(blob_len - off);
            let win = &blob[off..off + len];
            assert_eq!(
                crc32c(win),
                crc32c_bitwise(win),
                "slice16 != oracle at off={off} len={len}"
            );
        }
        // known-answer vectors: the same rows the parser tests pin
        assert_eq!(crc32c(b""), 0x0000_0000);
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(&[0u8; 32]), 0x8A91_36AA);
    }
}
