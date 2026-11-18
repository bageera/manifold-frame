//! W8-E2: resource-limit verification (all ceilings enforced BEFORE the
//! matching allocations; relaxed limits behave EXACTLY like the
//! unlimited readers; embedded limits pass through to the engine).

use manifold_frame::limits::{check_header, DecodeLimits};
use manifold_frame::parse_borrowed_limited;
use manifold_frame::{
    decode_image_borrowed_limited, parse_borrowed, Codec, ContainerHeader, PixelFormat, *,
};

/// A raw RGB8 container at w×h (or oversized geometry on demand).
fn raw_rgb8(width: u32, height: u32) -> Vec<u8> {
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 1,
        width,
        height,
    };
    let plane = vec![0xAAu8; width as usize * height as usize * 3];
    serialize(&hdr, &[(ChunkType::Pixel, Codec::Raw, &plane)]).unwrap()
}

fn relaxed() -> DecodeLimits {
    DecodeLimits::default()
}

#[test]
fn header_stage_limits_reject_before_any_chunk_work() {
    // width over the cap: parse_header succeeds then the height check
    // order (w then h then pixels then chunks) is asserted.
    let big = raw_rgb8(1024, 64);
    let mut lim = relaxed();
    lim.max_width = 512;
    let err = parse_borrowed_limited(&big, &lim).unwrap_err();
    assert!(err.to_string().contains("max_width"), "{err}");
    // height cap.
    let mut lim = relaxed();
    lim.max_height = 16;
    let err = parse_borrowed_limited(&big, &lim).unwrap_err();
    assert!(err.to_string().contains("max_height"), "{err}");
    // pixel count cap: exactly at the boundary passes, one less fails.
    let img = raw_rgb8(100, 10); // 1000 px
    let mut lim = relaxed();
    lim.max_pixels = 1000;
    let (hdr, _) = parse_borrowed_limited(&img, &lim).unwrap();
    lim.max_pixels = 999;
    let err = parse_borrowed_limited(&img, &lim).unwrap_err();
    assert!(err.to_string().contains("max_pixels"), "{err}");
    check_header(&hdr, &relaxed()).unwrap();
}

#[test]
fn plane_and_planar16_ceiling_enforced_before_decode() {
    // raw plane: 300 B > the 299 cap must reject BEFORE the read.
    let img = raw_rgb8(10, 10); // 300 B
    let mut lim = relaxed();
    lim.max_decoded_bytes = 299;
    let (hdr, chunks) = parse_borrowed_limited(&img, &limits::DecodeLimits::default()).unwrap();
    let err = decode_image_borrowed_limited(&hdr, &chunks, &lim).unwrap_err();
    assert!(err.to_string().contains("max_decoded_bytes (raw)"), "{err}");
    // exactly at the bound passes and yields the exact samples.
    lim.max_decoded_bytes = 300;
    let (px, meta) = decode_image_borrowed_limited(&hdr, &chunks, &lim).unwrap();
    assert_eq!(meta.orientation, 1);
    assert_eq!(px.samples.len(), 300);
}

#[test]
fn embed_budget_and_engine_passthrough() {
    // Build a minimal MNF8 embed via the engine, wrap in a container.
    let dims = manifold::codec::Dims {
        width: 2,
        height: 2,
    };
    let px: Vec<u8> = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    let (stream, _stats, _crc_in_stream) =
        manifold::mnf8::encode_mnf8(&px, dims, false, manifold::mnf8::Profile::Balanced);
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 1,
        width: 2,
        height: 2,
    };
    let cont = serialize(&hdr, &[(ChunkType::Pixel, Codec::MnF8, stream.as_slice())]).unwrap();
    let (hdr, chunks) = parse_borrowed(&cont).unwrap();
    // unlimited decode works; the engine's own decode agrees.
    let (px2, _) = decode_image_borrowed_limited(&hdr, &chunks, &relaxed()).unwrap();
    assert_eq!(px2.samples, px);
    // the input budget bites BEFORE the engine (even with engine limits
    // open).
    let mut lim = relaxed();
    lim.max_embed_input_bytes = stream.len() as u64 - 1;
    lim.embed_engine.max_pixels = u64::MAX;
    let err = decode_image_borrowed_limited(&hdr, &chunks, &lim).unwrap_err();
    assert!(err.to_string().contains("max_embed_input_bytes"), "{err}");
    // engine passthrough: a tight engine pixel cap rejects THROUGH the
    // engine's limits-aware decoder (2*2 px > 3 px budget).
    let mut lim = relaxed();
    lim.embed_engine.max_pixels = 3;
    let err = decode_image_borrowed_limited(&hdr, &chunks, &lim).unwrap_err();
    assert!(
        err.to_string().contains("MNF8 embed decode"),
        "engine passthrough surfaced: {err}"
    );
}

#[test]
fn relaxed_limits_are_behaviorally_identical_to_the_unlimited_readers() {
    // A handful of shapes through both reader styles must agree exactly.
    for &(w, h, _fmt) in &[
        (3u32, 2u32, PixelFormat::Rgb8),
        (5, 4, PixelFormat::Gray8),
        (4, 3, PixelFormat::Rgba8),
    ] {
        let img = raw_rgb8(w, h);
        let (h1, c1) = parse_borrowed(&img).unwrap();
        let (h2, c2) = parse_borrowed_limited(&img, &relaxed()).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(c1.len(), c2.len());
        let a = manifold_frame::decode_image_borrowed(&h1, &c1).unwrap();
        let b = decode_image_borrowed_limited(&h2, &c2, &relaxed()).unwrap();
        assert_eq!(a.0.samples, b.0.samples);
        assert_eq!(a.1, b.1);
    }
}

#[test]
fn metadata_and_chunk_count_ceilings() {
    // A container with a large ICC metadata chunk; the aggregate limit
    // rejects during the walk.
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 2,
        width: 1,
        height: 1,
    };
    let plane = vec![1u8, 2, 3];
    let icc = vec![0x02u8; 512];
    let cont = serialize(
        &hdr,
        &[
            (ChunkType::Pixel, Codec::Raw, plane.as_slice()),
            (ChunkType::Icc, Codec::Raw, icc.as_slice()),
        ],
    )
    .unwrap();
    let mut lim = relaxed();
    lim.max_metadata_bytes = 511;
    let err = parse_borrowed_limited(&cont, &lim).unwrap_err();
    assert!(err.to_string().contains("max_metadata_bytes"), "{err}");
    // chunk-count ceiling: the declared header field is checked first.
    let mut lim = relaxed();
    lim.max_chunks = 1;
    let err = parse_borrowed_limited(&cont, &lim).unwrap_err();
    assert!(err.to_string().contains("max_chunks"), "{err}");
}
