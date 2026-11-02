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
//! critical chunks reject; exactly one PIXEL chunk; codec 5 (MNF9
//! embed) = whole-frame, sole pixel layer; malformed input = Err,
//! never panic.

/// W1: pixel-layer decode (embedded == standalone gate).
pub mod decode;
pub mod error;
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

/// CRC32C (Castagnoli, reflected polynomial 0x1EDC6F41). Table-free.
pub fn crc32c(data: &[u8]) -> u32 {
    let poly: u32 = 0x82F63B78;
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ poly
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
pub use crate::decode::{decode_image, decode_pixels, Metadata};
pub use crate::error::Error;
pub use crate::parser::{parse, serialize, Chunk};
