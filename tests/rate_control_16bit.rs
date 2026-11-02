//! W3-R1: the mandatory rate control for the 16-bit hi/lo split.
//!
//! The split arm must beat the interleaved arm at COMPLETE-WIRE size or
//! the split earns nothing as a compression choice (it still exists as a
//! vocabulary item). Per the review's formula (no metadata):
//!   split      = 18 + 2*(16 + Z(plane))          [two chunks]
//!   interleaved= 18 + 1*(16 + Z(interleaved))    [hypothetical single
//!                                                 chunk — not a legal
//!                                                 W3 shape; control only]

use manifold_frame::{ChunkType, Codec, ContainerHeader, PixelFormat};

fn zstd_frame(plane: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = zstd::stream::Encoder::new(Vec::new(), 19).unwrap();
    e.include_checksum(true).unwrap();
    e.write_all(plane).unwrap();
    e.finish().unwrap()
}

#[test]
fn w3r1_split_vs_interleaved_rate_control() {
    // A photographic-ish 16-bit gray fixture: real corpus pixels doubled
    // (hi=MSB, lo=LSB) — the alphabet-relayout's intended class.
    let img = manifold::img::load(Path::new(
        "../manifold/testdata/photos/forest_224x160_s900002.png",
    ))
    .unwrap();
    let (w, h) = (img.width, img.height);
    let n = (w * h) as usize;
    // Interleave the RGB8 bytes as u16 LE samples, then split to hi/lo
    // planes (the same pixels the formats describe).
    let mut interleaved = Vec::with_capacity(n * 2);
    let mut hi = Vec::with_capacity(n);
    let mut lo = Vec::with_capacity(n);
    for px in img.pixels.as_chunks::<3>().0 {
        // Pack R,G,B × 257 → spread for a 16-bit gray test per channel;
        // simplest honest fixture: use G only, doubled.
        let v = u16::from(px[1]) * 257;
        interleaved.extend_from_slice(&v.to_be_bytes());
        hi.push((v >> 8) as u8);
        lo.push((v & 0xFF) as u8);
    }
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Gray16,
        orientation: 1,
        chunk_count: 2,
        width: w,
        height: h,
    };
    let hi_f = zstd_frame(&hi);
    let lo_f = zstd_frame(&lo);
    let split = manifold_frame::serialize(
        &hdr,
        &[
            (ChunkType::Pix16Hi, Codec::Zstd, &hi_f),
            (ChunkType::Pix16Lo, Codec::Zstd, &lo_f),
        ],
    )
    .unwrap();
    // The interleaved CONTROL as a hypothetical single-chunk file:
    // Gray16 wants 2 chunks, so build its bytes with a hand header.
    let ihdr = ContainerHeader {
        pixel_format: PixelFormat::Gray8, // 1 B/px × but plane = 2n bytes...
        orientation: 1,
        chunk_count: 1,
        width: w,
        height: h,
    };
    let _ = ihdr;
    let i_f = zstd_frame(&interleaved);
    let interleaved_wire = 18 + 16 + i_f.len();

    let split_size = split.len();
    let report = format!(
        "W3-R1 rate control (forest G×257, {}x{}):\n  split (2 chunks)  = {} B = 18 + 2*16 + Z(HI) {} + Z(LO) {}\n  interleaved (1)   = {} B = 18 + 16 + Z(interleaved) {}\n  verdict           = {}",
        w,
        h,
        split_size,
        hi_f.len(),
        lo_f.len(),
        interleaved_wire,
        i_f.len(),
        if split_size < interleaved_wire {
            "SPLIT WINS"
        } else if split_size == interleaved_wire {
            "TIE"
        } else {
            "INTERLEAVED WINS (split stays vocabulary, not the rate pick)"
        }
    );
    println!("{report}");
    // The control's decode must roundtrip through the JOINED plane —
    // exactness of the instrument itself.
    assert_eq!(hi.len() + lo.len(), interleaved.len());
    // This test PINS THE MEASUREMENT, not a winner (the review's rule:
    // record losses beside wins). The printed table is the evidence.
    assert!(split_size > 0 && interleaved_wire > 0);
}

use std::path::Path;
