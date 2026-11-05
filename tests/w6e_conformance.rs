//! W6-E conformance gate: sample-exactness for every MNFX v1 pixel
//! format, from synthetic fixtures (no engine testdata required — runs
//! in CI). Each format round-trips through `serialize` -> `parse` ->
//! `decode_pixels` and must reproduce its source plane byte-exactly.
//!
//! With `MNFX_WRITE_FIXTURES=1` the fixtures are ALSO dumped to
//! /tmp/w6e/ (container + decoded-plane truth + a PAM source image) as
//! the published fixtures the C-ABI integrations run against:
//! see integrations/README.md.

use manifold_frame::{ChunkType, Codec, ContainerHeader, PixelFormat, MAGIC, VERSION};
use std::io::Write;

/// The shared fixture pattern set (deterministic; not solid fills).
mod sources {
    pub const W: u32 = 8;
    pub const H: u32 = 6;
    pub const N: usize = 8 * 6;

    pub fn rgb8() -> Vec<u8> {
        (0..N)
            .flat_map(|i| [(i * 7) as u8, (i * 11) as u8, (i * 13) as u8])
            .collect()
    }
    pub fn rgba8() -> Vec<u8> {
        rgb8()
            .as_chunks::<3>()
            .0
            .iter()
            .enumerate()
            .flat_map(|(i, px)| [px[0], px[1], px[2], (i * 3) as u8])
            .collect()
    }
    pub fn gray8() -> Vec<u8> {
        (0..N).map(|i| (i * 9) as u8).collect()
    }
    pub fn graya8() -> Vec<u8> {
        gray8()
            .iter()
            .enumerate()
            .flat_map(|(i, g)| [*g, (i * 5) as u8])
            .collect()
    }
    /// Byte*257 = the documented 8->16 spread (BE u16, interleaved).
    pub fn rgb16() -> Vec<u8> {
        let mut out = Vec::with_capacity(N * 6);
        for px in rgb8().as_chunks::<3>().0 {
            for b in px {
                let v = u16::from(*b) * 257;
                out.extend_from_slice(&v.to_be_bytes());
            }
        }
        out
    }
    pub fn gray16() -> Vec<u8> {
        gray8()
            .iter()
            .flat_map(|b| {
                let v = u16::from(*b) * 257;
                [(v >> 8) as u8, (v & 0xFF) as u8]
            })
            .collect()
    }
    pub fn planes(fmt: manifold_frame::PixelFormat) -> Vec<u8> {
        use manifold_frame::PixelFormat::*;
        match fmt {
            Rgb8 => rgb8(),
            Rgba8 => rgba8(),
            Gray8 => gray8(),
            GrayA8 => graya8(),
            Rgb16 => rgb16(),
            Gray16 => gray16(),
        }
    }
}

fn hdr(fmt: PixelFormat, chunks: usize) -> ContainerHeader {
    ContainerHeader {
        pixel_format: fmt,
        orientation: 1,
        chunk_count: chunks as u16,
        width: sources::W,
        height: sources::H,
    }
}

fn plane_chunks(fmt: PixelFormat) -> Vec<(ChunkType, Codec, Vec<u8>)> {
    let plane = sources::planes(fmt);
    if fmt.is_planar16() {
        let hi: Vec<u8> = plane.iter().step_by(2).copied().collect();
        let lo: Vec<u8> = plane.iter().skip(1).step_by(2).copied().collect();
        vec![
            (ChunkType::Pix16Hi, Codec::Raw, hi),
            (ChunkType::Pix16Lo, Codec::Raw, lo),
        ]
    } else {
        vec![(ChunkType::Pixel, Codec::Raw, plane)]
    }
}

fn roundtrip(fmt: PixelFormat, chunks: Vec<(ChunkType, Codec, Vec<u8>)>, plane: &[u8]) {
    let head = hdr(fmt, chunks.len());
    let refs: Vec<(ChunkType, Codec, &[u8])> = chunks
        .iter()
        .map(|(t, c, v)| (*t, *c, v.as_slice()))
        .collect();
    let buf = manifold_frame::serialize(&head, &refs).expect("serialize");
    assert_eq!(&buf[..4], &MAGIC, "magic");
    assert_eq!(
        u16::from_le_bytes([buf[4], buf[5]]),
        VERSION,
        "version (LE on the wire)"
    );
    let (h2, parsed) = manifold_frame::parse(&buf).expect("parse");
    let px = manifold_frame::decode_pixels(&h2, &parsed).expect("decode");
    assert_eq!(
        (px.dims.width, px.dims.height),
        (sources::W, sources::H),
        "dims"
    );
    assert_eq!(px.format, fmt, "format");
    assert_eq!(px.samples, plane, "sample-exact {fmt:?}");
}

macro_rules! raw_roundtrip {
    ($name:ident, $fmt:expr) => {
        #[test]
        fn $name() {
            let fmt: PixelFormat = $fmt;
            let chunks = plane_chunks(fmt);
            let plane = sources::planes(fmt);
            roundtrip(fmt, chunks, &plane);
        }
    };
}

raw_roundtrip!(rgb8_raw, PixelFormat::Rgb8);
raw_roundtrip!(rgba8_raw, PixelFormat::Rgba8);
raw_roundtrip!(gray8_raw, PixelFormat::Gray8);
raw_roundtrip!(graya8_raw, PixelFormat::GrayA8);
raw_roundtrip!(rgb16_hi_lo_raw, PixelFormat::Rgb16);
raw_roundtrip!(gray16_hi_lo_raw, PixelFormat::Gray16);

/// The full-codec arms for RGB8 (raw vs zstd vs embedded MNF8) must all
/// reproduce the same source plane exactly.
#[test]
fn rgb8_all_codecs() {
    let plane = sources::rgb8();
    // zstd plane
    let mut e = zstd::stream::Encoder::new(Vec::new(), 19).unwrap();
    e.include_checksum(true).unwrap();
    e.write_all(&plane).unwrap();
    let z = e.finish().unwrap();
    roundtrip(
        PixelFormat::Rgb8,
        vec![(ChunkType::Pixel, Codec::Zstd, z)],
        &plane,
    );
    // embedded MNF8 (the engine does the compression; dims must agree)
    let enc = manifold::mnf8::encode_mnf8(
        &plane,
        manifold::codec::Dims {
            width: sources::W,
            height: sources::H,
        },
        false,
        manifold::mnf8::Profile::Balanced,
    )
    .0;
    roundtrip(
        PixelFormat::Rgb8,
        vec![(ChunkType::Pixel, Codec::MnF8, enc)],
        &plane,
    );
}

/// PAM (P7) source image: PIL/ImageMagick/libvips all read it, alpha
/// included, and the tuple type carries the geometry the bridges need.
fn write_pam(path: &std::path::Path, fmt: PixelFormat, plane: &[u8]) {
    let (depth, tupltype) = match fmt {
        PixelFormat::Rgb8 => ("3", "RGB"),
        PixelFormat::Rgba8 => ("4", "RGB_ALPHA"),
        PixelFormat::Gray8 | PixelFormat::Gray16 => ("1", "GRAYSCALE"),
        PixelFormat::GrayA8 => ("2", "GRAYSCALE_ALPHA"),
        PixelFormat::Rgb16 => ("3", "RGB"),
    };
    let maxval: u32 = if fmt.is_planar16() { 65535 } else { 255 };
    let mut out = format!(
        "P7\nWIDTH {}\nHEIGHT {}\nDEPTH {}\nMAXVAL {}\nTUPLTYPE {}\nENDHDR\n",
        sources::W,
        sources::H,
        depth,
        maxval,
        tupltype
    )
    .into_bytes();
    out.extend_from_slice(plane);
    std::fs::write(path, out).unwrap();
}

/// MNFX_WRITE_FIXTURES=1: dump the published fixture set the C-ABI
/// integrations consume (.mnfx RAW-charset containers + .plane truths +
/// .pam sources, one per format; the 16-bit PAMs carry the BE bytes
/// exactly as the wire does).
#[test]
fn fixtures_write() {
    let Some(dir) = std::env::var_os("MNFX_WRITE_FIXTURES").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    for fmt in [
        PixelFormat::Rgb8,
        PixelFormat::Rgba8,
        PixelFormat::Gray8,
        PixelFormat::GrayA8,
        PixelFormat::Rgb16,
        PixelFormat::Gray16,
    ] {
        let name = format!("{fmt:?}").to_lowercase();
        let plane = sources::planes(fmt);
        let chunks = plane_chunks(fmt);
        let head = hdr(fmt, chunks.len());
        let refs: Vec<(ChunkType, Codec, &[u8])> = chunks
            .iter()
            .map(|(t, c, v)| (*t, *c, v.as_slice()))
            .collect();
        let buf = manifold_frame::serialize(&head, &refs).unwrap();
        std::fs::write(dir.join(format!("{name}.mnfx")), &buf).unwrap();
        std::fs::write(dir.join(format!("{name}.plane")), &plane).unwrap();
        write_pam(&dir.join(format!("{name}.pam")), fmt, &plane);
        eprintln!("fixture: {}/{name}.mnfx ({} B)", dir.display(), buf.len());
    }
}
