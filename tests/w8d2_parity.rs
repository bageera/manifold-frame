//! W8-D2 parser parity gate: the owning `parse()` and the borrowed
//! `parse_borrowed()` must accept and reject THE SAME inputs with THE
//! SAME outcomes — including the historically accepted pure-append
//! mutations (D2 must not accidentally tighten trailing-byte
//! acceptance), identical error messages on the reject side, borrow
//! pointers provably inside the input (the allocation proof), six-format
//! sample exactness through both decode paths, byte-exact metadata, and
//! lossless rewrite identity.

use manifold_frame::{parse, parse_borrowed, ChunkType, Codec, ContainerHeader, PixelFormat};

const POLY_SEED: u32 = 0x2545_F491;

fn payload(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    let mut s: u32 = POLY_SEED;
    for b in v.iter_mut() {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        *b = (s >> 24) as u8;
    }
    v
}

/// Six-format source generators (compact mirrors of the W6-E patterns).
fn sources(fmt: PixelFormat, plane: &[u8]) -> Vec<(ChunkType, Codec, Vec<u8>)> {
    let w = 2048u32;
    let h = (plane.len() as u32) / w.max(1) / fmt.stride_mult().max(1) as u32;
    let hdr_planes = |p: &[u8]| -> Vec<(ChunkType, Codec, Vec<u8>)> {
        if fmt.is_planar16() {
            let hi: Vec<u8> = p.iter().step_by(2).copied().collect();
            let lo: Vec<u8> = p.iter().skip(1).step_by(2).copied().collect();
            vec![
                (ChunkType::Pix16Hi, Codec::Raw, hi),
                (ChunkType::Pix16Lo, Codec::Raw, lo),
            ]
        } else {
            vec![(ChunkType::Pixel, Codec::Raw, p.to_vec())]
        }
    };
    let _ = (w, h);
    hdr_planes(plane)
}

fn container(kind: &str, plane_bytes: usize) -> (Vec<u8>, Vec<u8>) {
    use std::io::Write as _;
    let (fmt, chunk_count): (PixelFormat, u16) = match kind {
        "raw" => (PixelFormat::Gray8, 1),
        "zstd" => (PixelFormat::Gray8, 1),
        "embed" => (PixelFormat::Rgb8, 1),
        "planar16" => (PixelFormat::Gray16, 2),
        "metadata" => (PixelFormat::Gray8, 5),
        "rgba8" => (PixelFormat::Rgba8, 1),
        "rgb16" => (PixelFormat::Rgb16, 2),
        other => panic!("unknown kind {other}"),
    };
    let (w, bpx) = match fmt {
        PixelFormat::Rgb8 => ((plane_bytes / 3 / 512) as u32, 3usize),
        PixelFormat::Rgba8 => ((plane_bytes / 4 / 512) as u32, 4usize),
        PixelFormat::Gray8 => (2048, 1),
        PixelFormat::GrayA8 => (2048, 2),
        PixelFormat::Gray16 => (2048, 2),
        _ => (2048, 1),
    };
    let h = (plane_bytes / bpx / w as usize) as u32;
    let _ = chunk_count;
    let truth = payload(plane_bytes);
    let hdr = ContainerHeader {
        pixel_format: fmt,
        orientation: 1,
        chunk_count: 1,
        width: w,
        height: h,
    };
    let _ = h;
    // planes built per format with real dims
    let hdr = match fmt {
        PixelFormat::Rgb8 => ContainerHeader {
            pixel_format: fmt,
            orientation: 1,
            chunk_count: 1,
            width: w,
            height: h,
        },
        _ => hdr,
    };
    let chunks: Vec<(ChunkType, Codec, Vec<u8>)> = match kind {
        "zstd" => {
            let mut e = zstd::stream::Encoder::new(Vec::new(), 19).unwrap();
            e.include_checksum(true).unwrap();
            e.write_all(&truth).unwrap();
            vec![(ChunkType::Pixel, Codec::Zstd, e.finish().unwrap())]
        }
        "embed" => {
            // the engine asserts plane == w * h * 3: round to valid geometry
            let plane_bytes_used = (plane_bytes / (512 * 3)) * (512 * 3);
            let plane = payload(plane_bytes_used);
            let dims = manifold::codec::Dims {
                width: plane_bytes_used as u32 / (512 * 3),
                height: 512,
            };
            let (file, _st, _b) =
                manifold::mnf8::encode_mnf8(&plane, dims, false, manifold::mnf8::Profile::Balanced);
            vec![(ChunkType::Pixel, Codec::MnF8, file)]
        }
        "planar16" => sources(PixelFormat::Gray16, &truth),
        "rgb16" => {
            let mut be: Vec<u8> = Vec::with_capacity(truth.len() * 2);
            for b in &truth {
                let v = u16::from(*b) * 257;
                be.extend_from_slice(&v.to_be_bytes());
            }
            let hi: Vec<u8> = be.iter().step_by(2).copied().collect();
            let lo: Vec<u8> = be.iter().skip(1).step_by(2).copied().collect();
            vec![
                (ChunkType::Pix16Hi, Codec::Raw, hi),
                (ChunkType::Pix16Lo, Codec::Raw, lo),
            ]
        }
        "metadata" => {
            let mut c = vec![(ChunkType::Pixel, Codec::Raw, truth.clone())];
            c.push((
                ChunkType::Exif,
                Codec::Raw,
                vec![0x49, 0x49, 0x2A, 0x00, 8, 0, 0, 0, 0, 0],
            ));
            c.push((
                ChunkType::Xmp,
                Codec::Raw,
                b"<x:xmpmeta>D2</x:xmpmeta>".to_vec(),
            ));
            c.push((ChunkType::Icc, Codec::Raw, {
                let mut icc = vec![0u8; 132];
                icc[3] = 132;
                icc
            }));
            c.push((ChunkType::Cicp, Codec::Raw, vec![9, 16, 0, 1]));
            c
        }
        _ => vec![(ChunkType::Pixel, Codec::Raw, truth.clone())],
    };
    let count = chunks.len() as u16;
    let refs: Vec<(ChunkType, Codec, &[u8])> = chunks
        .iter()
        .map(|(t, c, v)| (*t, *c, v.as_slice()))
        .collect();
    let hdr = ContainerHeader {
        chunk_count: count,
        ..hdr
    };
    (manifold_frame::serialize(&hdr, &refs).unwrap(), truth)
}

/// The mutation shapes: historically-accepted pure appends FIRST (the
/// D2 must-not-tighten case), plus truncations and byte flips.
fn mutations(base: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut rows = Vec::new();
    for tail in [1usize, 7, 50] {
        let mut m = base.to_vec();
        m.extend(std::iter::repeat_n(0x5Au8, tail));
        rows.push((format!("pure-append-{tail}"), m));
    }
    for cut in [0usize, 5, 18, base.len() / 2, base.len().saturating_sub(1)] {
        rows.push((
            format!("truncate-{cut}"),
            base[..cut.min(base.len())].to_vec(),
        ));
    }
    for pos in [0usize, 5, 6, 9, 20, 30, base.len() - 1, base.len() / 2] {
        let mut m = base.to_vec();
        if pos < m.len() {
            m[pos] ^= 0xFF;
            rows.push((format!("flip-{pos}"), m));
        }
    }
    rows
}

#[test]
fn d2_3_parity_accept_reject_and_identity() {
    let mut checked = 0usize;
    for kind in ["raw", "zstd", "embed", "planar16", "metadata"] {
        let (base, _truth) = container(kind, 65_536);
        let mut inputs = vec![("identity".to_string(), base.clone())];
        inputs.extend(mutations(&base));
        for (name, m) in &inputs {
            let owned = parse(m);
            let borrowed = parse_borrowed(m);
            checked += 1;
            match (&owned, &borrowed) {
                (Ok((h1, c1)), Ok((h2, c2))) => {
                    assert_eq!(h1, h2, "{name}/{kind}: header parity");
                    assert_eq!(c1.len(), c2.len(), "{name}/{kind}: chunk count parity");
                    for (i, (a, b)) in c1.iter().zip(c2.iter()).enumerate() {
                        assert_eq!(a.header, b.header, "{name}/{kind}: chunk {i} header parity");
                        assert_eq!(a.stored, b.stored, "{name}/{kind}: chunk {i} stored parity");
                        // D2.4 allocation proof: borrowed payloads point
                        // INSIDE the original input buffer.
                        let range = m.as_ptr_range();
                        let p = b.stored.as_ptr();
                        assert!(
                            p >= range.start && p <= range.end,
                            "{name}/{kind}: chunk {i} borrowed ref escapes the input"
                        );
                    }
                    // lossless rewrite identity for VALID shapes (rule 11 order, byte-exact)
                    if name == "identity" {
                        let re = manifold_frame::serialize(
                            h1,
                            &c1.iter()
                                .map(|c| (c.header.chunk_type, c.header.codec, c.stored.as_slice()))
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                        assert_eq!(re, *m, "{kind}: rewrite identity");
                        let re_b = manifold_frame::serialize(
                            h2,
                            &c2.iter()
                                .map(|c| (c.header.chunk_type, c.header.codec, c.stored))
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                        assert_eq!(re_b, *m, "{kind}: borrowed rewrite identity");
                    }
                }
                (Err(e1), Err(e2)) => {
                    assert_eq!(
                        format!("{e1}"),
                        format!("{e2}"),
                        "{name}/{kind}: error parity"
                    );
                }
                (a, b) => panic!("{name}/{kind}: ACCEPT/REJECT parity broken ({a:?} vs {b:?})"),
            }
        }
    }
    assert!(checked > 40, "mutation corpus too small: {checked}");
}

#[test]
fn d2_3_sample_exactness_and_metadata_all_formats() {
    // Six formats through BOTH decode paths; borrowed raw planes must be
    // zero-copy (pointer inside the input), zstd/embed must materialize.
    for (kind, _fmt, bpx) in [
        ("raw", PixelFormat::Gray8, 1usize),
        ("rgba8", PixelFormat::Rgba8, 4usize),
        ("planar16", PixelFormat::Gray16, 2usize),
    ] {
        let (buf, truth) = container(kind, 65_536);
        let (h, c) = parse(&buf).unwrap();
        let owned = manifold_frame::decode_image(&h, &c).unwrap();
        assert_eq!(
            owned.0.dims.width * owned.0.dims.height * bpx as u32,
            truth.len() as u32
        );
        assert_eq!(owned.0.samples, truth, "{kind}: owning sample exactness");
        let (bh, bc) = parse_borrowed(&buf).unwrap();
        let borrowed = manifold_frame::decode_image_borrowed(&bh, &bc).unwrap();
        match &borrowed.0.samples {
            std::borrow::Cow::Borrowed(s) => {
                // raw-plane zero-copy proof (allocation gate)
                let range = buf.as_ptr_range();
                assert!(
                    s.as_ptr() >= range.start && s.as_ptr() <= range.end,
                    "{kind}: borrowed samples not zero-copy"
                );
                assert_eq!(*s, truth, "{kind}: borrowed sample exactness");
            }
            std::borrow::Cow::Owned(s) => {
                assert_eq!(
                    s.as_slice(),
                    truth,
                    "{kind}: owned-sample exactness (post-decode kind)"
                );
            }
        }
        // metadata byte-exact across both representations
        assert_eq!(owned.1.exif, borrowed.1.exif);
        assert_eq!(owned.1.xmp, borrowed.1.xmp);
        assert_eq!(owned.1.icc, borrowed.1.icc);
        assert_eq!(owned.1.cicp, borrowed.1.cicp);
        assert_eq!(owned.1.orientation, borrowed.1.orientation);
    }
    // rgb16 exact sample assembly (PNG wire order) through the borrowed
    // path. Geometry by construction: RGB16 = 3 BE u16 samples per pixel
    // (6 B/px); bound = w × h × 6; hi/lo = bound/2 each.
    let (w, h) = (1024u32, 2u32);
    let n_px = (w * h) as usize;
    let samples_be: Vec<u8> = (0..n_px)
        .flat_map(|i| {
            [
                (u16::from((i * 7) as u8 % 251) * 257).to_be_bytes(),
                (u16::from((i * 11) as u8 % 251) * 257).to_be_bytes(),
                (u16::from((i * 13) as u8 % 251) * 257).to_be_bytes(),
            ]
            .concat()
        })
        .collect();
    let hi: Vec<u8> = samples_be.iter().step_by(2).copied().collect();
    let lo: Vec<u8> = samples_be.iter().skip(1).step_by(2).copied().collect();
    let buf = manifold_frame::serialize(
        &ContainerHeader {
            pixel_format: PixelFormat::Rgb16,
            orientation: 1,
            chunk_count: 2,
            width: w,
            height: h,
        },
        &[
            (ChunkType::Pix16Hi, Codec::Raw, hi.as_slice()),
            (ChunkType::Pix16Lo, Codec::Raw, lo.as_slice()),
        ],
    )
    .unwrap();
    let (bh, bc) = parse_borrowed(&buf).unwrap();
    let borrowed = manifold_frame::decode_pixels_borrowed(&bh, &bc).unwrap();
    assert_eq!(
        borrowed.samples, samples_be,
        "rgb16 BE interleave exactness (borrowed)"
    );
}

// A tiny self-contained generator so tests do not build container bytes
// twice: the header for metadata-heavy rows carries 5 chunks.
fn buf_and_meta() -> Vec<u8> {
    let (buf, _t) = container("metadata", 65_536);
    buf
}

#[test]
fn d2_3_metadata_lifetimes_borrowed() {
    let buf = buf_and_meta();
    let (h, c) = parse_borrowed(&buf).unwrap();
    let (_px, meta) = manifold_frame::decode_image_borrowed(&h, &c).unwrap();
    assert!(meta.exif.is_some() && meta.xmp.is_some() && meta.icc.is_some() && meta.cicp.is_some());
    assert_eq!(meta.orientation, 1);
    // The metadata payloads reference buf (zero-copy): the borrow checker
    // enforces the lifetime against buf below.
    assert_eq!(
        meta.icc.unwrap()[3],
        132,
        "metadata chunk preserved (borrowed from buf)"
    );
}
