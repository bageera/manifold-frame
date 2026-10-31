//! W4-C differential verification: a real engine file + preserved
//! metadata (EXIF/XMP/ICC/CICP) decodes pixel-exact, metadata
//! byte-exact, and rewrites losslessly (parse→serialize == bytes).

use manifold_frame::{ChunkType, Codec, ContainerHeader, PixelFormat};

/// "II"\x2A\x00 + IFD offset 8 + zero-entry IFD — a valid empty TIFF shell.
const EXIF: &[u8] = &[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];

/// A 132-byte ICC shell with a plausible BE size header.
static ICC: [u8; 132] = {
    let mut v = [0u8; 132];
    v[3] = 132;
    v
};

#[test]
fn metadata_heavy_container_differential() {
    let img = manifold::img::load(Path::new(
        "../manifold/testdata/photos/forest_224x160_s900002.png",
    ))
    .unwrap();
    let dims = manifold::codec::Dims {
        width: img.width,
        height: img.height,
    };
    // Real engine file, Balanced profile (single zstd-19 candidate; the
    // Maximum sweep is the corpus gate's job, not every test's).
    let (file, _st, _b) =
        manifold::mnf8::encode_mnf8(&img.pixels, dims, false, manifold::mnf8::Profile::Balanced);
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 5,
        width: dims.width,
        height: dims.height,
    };
    let xmp: &[u8] = b"<x:xmpmeta>W4-C-payload</x:xmpmeta>";
    let cicp: &[u8] = &[9, 16, 0, 1];
    let buf = manifold_frame::serialize(
        &hdr,
        &[
            (ChunkType::Pixel, Codec::MnF8, &file),
            (ChunkType::Exif, Codec::Raw, EXIF),
            (ChunkType::Xmp, Codec::Raw, xmp),
            (ChunkType::Icc, Codec::Raw, &ICC),
            (ChunkType::Cicp, Codec::Raw, cicp),
        ],
    )
    .unwrap();

    // Gate 1: lossless rewrite (parse→serialize == bytes).
    let (h, c) = manifold_frame::parse(&buf).unwrap();
    let re = manifold_frame::serialize(
        &h,
        &c.iter()
            .map(|c| (c.header.chunk_type, c.header.codec, &*c.stored))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(re, buf, "metadata-heavy rewrite must be byte-lossless");

    // Gate 2: pixel decode == standalone == source.
    let (px, meta) = manifold_frame::decode_image(&h, &c).unwrap();
    let (sdims, spx, _b) = manifold::mnf8::decode_mnf8(&file).unwrap();
    assert_eq!(px.dims, sdims);
    assert_eq!(px.samples, spx, "pixel decode != standalone");
    assert_eq!(px.samples, img.pixels, "pixels lost vs source");

    // Gate 3: metadata preserved byte-exact.
    assert_eq!(meta.exif, Some(EXIF));
    assert_eq!(meta.xmp, Some(xmp));
    assert_eq!(meta.icc, Some(&ICC[..]));
    assert_eq!(meta.cicp, Some(cicp));
    assert_eq!(meta.orientation, 1);

    println!(
        "W4-C differential ok: {} B container, {}x{} px, 4 metadata chunks preserved losslessly",
        buf.len(),
        dims.width,
        dims.height
    );
}

use std::path::Path;
