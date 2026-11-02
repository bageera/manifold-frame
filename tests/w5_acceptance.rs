//! W5 acceptance: the integrated pipeline the reviewer named — a real
//! PNG into MNFX through the lib path (engine pick mirrors the CLI), MNFX
//! back out, pixels byte-identical. Plus negative: a corrupted container
//! rejects without panicking.

use manifold_frame::{parse, serialize, ChunkType, Codec, ContainerHeader, PixelFormat};
use std::path::Path;

fn engine_pick(pixels: &[u8], dims: manifold::codec::Dims) -> (Vec<u8>, Codec) {
    // The CLI's --mnf8 flow: MNF8 balanced vs the MNF9 arm at file size.
    let (mnf8, _st, _b) =
        manifold::mnf8::encode_mnf8(pixels, dims, false, manifold::mnf8::Profile::Balanced);
    match manifold::mnf9::encode_exact(pixels, dims) {
        Some(f9) if f9.len() < mnf8.len() => (f9, Codec::MnF9),
        _ => (mnf8, Codec::MnF8),
    }
}

#[test]
fn w5_real_png_through_mnfx_roundtrip() {
    let img =
        manifold::img::load(Path::new("../manifold/testdata/heldout/poster_208x144.png")).unwrap();
    let dims = manifold::codec::Dims {
        width: img.width,
        height: img.height,
    };
    let (file, codec) = engine_pick(&img.pixels, dims);

    // A source ICC chunk ships with the image (the "complete image info"
    // half of the acceptance; EXIF/XMP empty for this fixture).
    let icc = vec![0u8; 132];
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 2,
        width: dims.width,
        height: dims.height,
    };
    let buf = serialize(
        &hdr,
        &[
            (ChunkType::Pixel, codec, &file),
            (ChunkType::Icc, Codec::Raw, &icc),
        ],
    )
    .unwrap();

    // MNFX → image: pixel-exact, metadata preserved.
    let (h, c) = parse(&buf).unwrap();
    let (px, meta) = manifold_frame::decode_image(&h, &c).unwrap();
    assert_eq!(
        (px.dims.width, px.dims.height),
        (img.width, img.height),
        "dims must match the source"
    );
    assert_eq!(px.samples, img.pixels, "pixel reconstruction differs");
    assert_eq!(meta.icc, Some(&icc[..]), "the ICC must come back whole");
    assert_eq!(meta.orientation, 1);

    // And the CLI's verify contract, exercised on the lib surface.
    assert_eq!(px.format, PixelFormat::Rgb8);
}

#[test]
fn w5_corrupted_container_rejects_not_panics() {
    let img = manifold::img::load(Path::new("../manifold/testdata/solid_256x256.png")).unwrap();
    let dims = manifold::codec::Dims {
        width: img.width,
        height: img.height,
    };
    let (file, codec) = engine_pick(&img.pixels, dims);
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 1,
        width: dims.width,
        height: dims.height,
    };
    let buf = serialize(&hdr, &[(ChunkType::Pixel, codec, &file)]).unwrap();

    // Byte flips MUST reject or decode (never panic): sweep representative
    // positions — magic, version, fmt, count, dims, a chunk header, a
    // payload byte, the CRC word.
    for pos in [0usize, 5, 6, 9, 20, 30, 60, buf.len() - 1, buf.len() / 2] {
        let mut b = buf.clone();
        b[pos] ^= 0xFF;
        // The decode path itself must not panic; accept (valid different
        // file) and reject are both fine per the fuzz's audit rule.
        if let Ok((h, c)) = parse(&b) {
            let _ = manifold_frame::decode_image(&h, &c);
        }
    }
    // Truncations at representative cuts.
    for cut in [0usize, 5, 17, 18, 30, buf.len() - 20] {
        if let Ok((h, c)) = parse(&buf[..cut]) {
            let _ = manifold_frame::decode_image(&h, &c);
        }
    }
}

#[allow(dead_code)] // fixture helper kept alongside the corrupt-loop tests
fn hdr() -> ContainerHeader {
    ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 1,
        width: 8,
        height: 8,
    }
}
