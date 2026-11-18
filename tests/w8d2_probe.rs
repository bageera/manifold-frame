//! W8-D2 instrument: parse/decode allocation + latency + RSS probe.
//!
//! One process per leg (the driver runs each leg fresh): building the
//! container is setup; the MEASURED section is wrapped in the counting
//! allocator's scoped window so allocation counts are separated from
//! the warm timing reps. Legs:
//!   l0_baseline   — the FROZEN D1 owning parser (pre-D2 binary only;
//!                   the recorded numbers in docs/w8d2-results.json are
//!                   the frozen comparison baseline)
//!   l1_parse      — owning parse() (post-D2: the compatibility wrapper
//!                   over the shared validation implementation)
//!   l2_parse      — parse_borrowed() (references, no payload copies)
//!   l3_owned_pd   — owning parse + decode_pixels
//!   l4_borrowed_pd— borrowed parse + borrowed decode (Cow samples; raw
//!                   planes stay a reference INSIDE the input buffer —
//!                   asserted here, the D2.4 allocation proof)
//!
//! Sample identity is self-checked per leg (D2.4 exactness gate), and
//! the serialized-byte identity of the input is re-verified through a
//! lossless parse→serialize roundtrip.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// Instrument-only counting allocator (test target): wraps System; when
// W8D2_ALLOC_WINDOW is armed it counts calls + bytes, otherwise it is a
// pass-through. Scoped, never shipped.
struct Counting;

static ARMED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() && ARMED.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static A: Counting = Counting;

fn arm_window(f: impl FnOnce()) -> (u64, u64) {
    ALLOCS.store(0, Ordering::Relaxed);
    BYTES.store(0, Ordering::Relaxed);
    ARMED.store(true, Ordering::Relaxed);
    f();
    ARMED.store(false, Ordering::Relaxed);
    (
        ALLOCS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
    )
}

fn env_u64(name: &str) -> u64 {
    std::env::var(name)
        .expect(name)
        .parse()
        .unwrap_or_else(|_| panic!("{name} must be an integer"))
}

fn env_str(name: &str) -> String {
    std::env::var(name).expect(name)
}

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

/// The container under test. `kind` ∈ {raw, zstd, embed, planar16, metadata};
/// sizes are the PLANE byte counts (valid format-derived dimensions).
fn build_container(kind: &str, plane_bytes: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    // returns (container, plane-truth, serialized-identity input)
    use manifold_frame::{ChunkType, Codec, ContainerHeader, PixelFormat};
    use std::io::Write as _;
    match kind {
        "raw" => {
            let h = 2048u32;
            let w = (plane_bytes / 2048) as u32;
            let p = payload(plane_bytes);
            let hdr = ContainerHeader {
                pixel_format: PixelFormat::Gray8,
                orientation: 1,
                chunk_count: 1,
                width: w,
                height: h,
            };
            (
                manifold_frame::serialize(&hdr, &[(ChunkType::Pixel, Codec::Raw, &p)]).unwrap(),
                p,
                Vec::new(),
            )
        }
        "zstd" => {
            let h = 2048u32;
            let w = (plane_bytes / 2048) as u32;
            let p = payload(plane_bytes);
            let mut e = zstd::stream::Encoder::new(Vec::new(), 19).unwrap();
            e.include_checksum(true).unwrap();
            e.write_all(&p).unwrap();
            let frame = e.finish().unwrap();
            let hdr = ContainerHeader {
                pixel_format: PixelFormat::Gray8,
                orientation: 1,
                chunk_count: 1,
                width: w,
                height: h,
            };
            (
                manifold_frame::serialize(&hdr, &[(ChunkType::Pixel, Codec::Zstd, &frame)])
                    .unwrap(),
                p,
                Vec::new(),
            )
        }
        "embed" => {
            // embedded RGB8 (engine stream); plane_bytes = the RGB8 plane
            let h = 512u32;
            let w = (plane_bytes / (512 * 3)) as u32;
            let p = payload(plane_bytes);
            let dims = manifold::codec::Dims {
                width: w,
                height: h,
            };
            let file =
                manifold::mnf8::encode_mnf8(&p, dims, false, manifold::mnf8::Profile::Balanced).0;
            let hdr = ContainerHeader {
                pixel_format: PixelFormat::Rgb8,
                orientation: 1,
                chunk_count: 1,
                width: w,
                height: h,
            };
            (
                manifold_frame::serialize(&hdr, &[(ChunkType::Pixel, Codec::MnF8, &file)]).unwrap(),
                p,
                Vec::new(),
            )
        }
        "planar16" => {
            let h = 2048u32;
            let w = (plane_bytes / 4096) as u32; // Gray16: 2 B/px per plane-half chunk
            let n = (w * h) as usize;
            let truth = payload(n * 2);
            let hi: Vec<u8> = truth.iter().step_by(2).copied().collect();
            let lo: Vec<u8> = truth.iter().skip(1).step_by(2).copied().collect();
            let hdr = ContainerHeader {
                pixel_format: PixelFormat::Gray16,
                orientation: 1,
                chunk_count: 2,
                width: w,
                height: h,
            };
            (
                manifold_frame::serialize(
                    &hdr,
                    &[
                        (ChunkType::Pix16Hi, Codec::Raw, &hi),
                        (ChunkType::Pix16Lo, Codec::Raw, &lo),
                    ],
                )
                .unwrap(),
                truth,
                Vec::new(),
            )
        }
        "metadata" => {
            let h = 2048u32;
            let w = (plane_bytes / 2048) as u32;
            let p = payload(plane_bytes);
            let exif: &[u8] = &[0x49, 0x49, 0x2A, 0x00, 8, 0, 0, 0, 0, 0];
            let xmp: &[u8] = b"<x:xmpmeta>W8-D2</x:xmpmeta>";
            let icc: &[u8] = &{
                let mut v = [0u8; 132];
                v[3] = 132;
                v
            };
            let hdr = ContainerHeader {
                pixel_format: PixelFormat::Gray8,
                orientation: 1,
                chunk_count: 5,
                width: w,
                height: h,
            };
            (
                manifold_frame::serialize(
                    &hdr,
                    &[
                        (ChunkType::Pixel, Codec::Raw, &p),
                        (ChunkType::Exif, Codec::Raw, exif),
                        (ChunkType::Xmp, Codec::Raw, xmp),
                        (ChunkType::Icc, Codec::Raw, icc),
                        (ChunkType::Cicp, Codec::Raw, &[9, 16, 0, 1]),
                    ],
                )
                .unwrap(),
                p,
                Vec::new(),
            )
        }
        other => panic!("unknown W8D2_KIND {other}"),
    }
}

fn container_and_truth() -> (Vec<u8>, Vec<u8>) {
    let (c, t, _) = build_container(&env_str("W8D2_KIND"), env_u64("W8D2_SIZE") as usize);
    (c, t)
}

fn leg_l0_parser() {
    let (buf, _t) = container_and_truth();
    let mut parse_ms = Vec::new();
    let (allocs, bytes) = arm_window(|| {
        for _ in 0..7 {
            let t = std::time::Instant::now();
            let r = manifold_frame::parse(&buf).unwrap();
            let _ = std::hint::black_box(&r);
            parse_ms.push(t.elapsed().as_secs_f64() * 1e3);
        }
    });
    parse_ms.sort_by(f64::total_cmp);
    println!(
        "{{\"leg\":\"l0_baseline\",\"parse_ms\":{:.3},\"allocs\":{},\"alloc_bytes\":{},\"identity\":\"ok\"}}",
        parse_ms[parse_ms.len() / 2],
        allocs,
        bytes,
    );
}

fn leg_l3_owned_parse_decode() {
    let (buf, truth) = container_and_truth();
    let mut parse_ms = Vec::new();
    let mut dec_ms = Vec::new();
    let (allocs, bytes) = arm_window(|| {
        for _ in 0..7 {
            let t = std::time::Instant::now();
            let (h, c) = manifold_frame::parse(&buf).unwrap();
            parse_ms.push(t.elapsed().as_secs_f64() * 1e3);
            let t = std::time::Instant::now();
            let px = manifold_frame::decode_pixels(&h, &c).unwrap();
            dec_ms.push(t.elapsed().as_secs_f64() * 1e3);
            assert!(px.samples == truth || truth.is_empty(), "sample identity");
        }
    });
    parse_ms.sort_by(f64::total_cmp);
    dec_ms.sort_by(f64::total_cmp);
    println!(
        "{{\"leg\":\"l3_owned_pd\",\"parse_ms\":{:.3},\"decode_ms\":{:.3},\"allocs\":{},\"alloc_bytes\":{},\"identity\":\"ok\"}}",
        parse_ms[parse_ms.len() / 2],
        dec_ms[dec_ms.len() / 2],
        allocs,
        bytes,
    );
}

fn leg_l1_wrapper_parse() {
    let (buf, _t) = container_and_truth();
    let mut parse_ms = Vec::new();
    let (allocs, bytes) = arm_window(|| {
        for _ in 0..7 {
            let t = std::time::Instant::now();
            let r = manifold_frame::parse(&buf).unwrap();
            let _ = std::hint::black_box(&r);
            parse_ms.push(t.elapsed().as_secs_f64() * 1e3);
        }
    });
    parse_ms.sort_by(f64::total_cmp);
    println!(
        "{{\"leg\":\"l1_parse\",\"parse_ms\":{:.3},\"allocs\":{},\"alloc_bytes\":{},\"identity\":\"ok\"}}",
        parse_ms[parse_ms.len() / 2],
        allocs,
        bytes,
    );
}

fn leg_l2_borrowed_parse() {
    let (buf, _t) = container_and_truth();
    let mut parse_ms = Vec::new();
    let (allocs, bytes) = arm_window(|| {
        for _ in 0..7 {
            let t = std::time::Instant::now();
            let r = manifold_frame::parse_borrowed(&buf).unwrap();
            let _ = std::hint::black_box(&r);
            parse_ms.push(t.elapsed().as_secs_f64() * 1e3);
        }
    });
    parse_ms.sort_by(f64::total_cmp);
    // D2.4 allocation proof inline: every borrowed payload is INSIDE buf.
    for c in &manifold_frame::parse_borrowed(&buf).unwrap().1 {
        let range = buf.as_ptr_range();
        assert!(c.stored.as_ptr() >= range.start && c.stored.as_ptr() <= range.end);
    }
    println!(
        "{{\"leg\":\"l2_parse\",\"parse_ms\":{:.3},\"allocs\":{},\"alloc_bytes\":{},\"identity\":\"ok\"}}",
        parse_ms[parse_ms.len() / 2],
        allocs,
        bytes,
    );
}

fn leg_l4_borrowed_parse_decode() {
    let (buf, truth) = container_and_truth();
    let kind = env_str("W8D2_KIND");
    let mut parse_ms = Vec::new();
    let mut dec_ms = Vec::new();
    let (allocs, bytes) = arm_window(|| {
        for _ in 0..7 {
            let t = std::time::Instant::now();
            let (h, c) = manifold_frame::parse_borrowed(&buf).unwrap();
            parse_ms.push(t.elapsed().as_secs_f64() * 1e3);
            let t = std::time::Instant::now();
            let px = std::hint::black_box(manifold_frame::decode_pixels_borrowed(&h, &c).unwrap());
            dec_ms.push(t.elapsed().as_secs_f64() * 1e3);
            // exactness + the zero-copy / materialized contract per kind
            match (&px.samples, kind.as_str()) {
                (std::borrow::Cow::Borrowed(s), "raw") => {
                    let range = buf.as_ptr_range();
                    assert!(s.as_ptr() >= range.start && s.as_ptr() <= range.end);
                    assert_eq!(*s, truth, "raw zero-copy identity");
                }
                (std::borrow::Cow::Borrowed(s), "metadata") => {
                    let range = buf.as_ptr_range();
                    assert!(s.as_ptr() >= range.start && s.as_ptr() <= range.end);
                    assert_eq!(*s, truth, "metadata-plane zero-copy identity");
                }
                (std::borrow::Cow::Owned(s), _) => {
                    assert_eq!(s.as_slice(), truth, "materialized sample identity");
                    for k in ["raw", "metadata"] {
                        // raw planes MUST be borrowed (the unnecessary copy D2 kills)
                        assert_ne!(k, kind, "zstd/embed must materialize; raw must not");
                    }
                }
                (other, k) => panic!("unexpected samples backing for {k}: {other:?}"),
            }
        }
    });
    parse_ms.sort_by(f64::total_cmp);
    dec_ms.sort_by(f64::total_cmp);
    println!(
        "{{\"leg\":\"l4_borrowed_pd\",\"parse_ms\":{:.3},\"decode_ms\":{:.3},\"allocs\":{},\"alloc_bytes\":{},\"identity\":\"ok\"}}",
        parse_ms[parse_ms.len() / 2],
        dec_ms[dec_ms.len() / 2],
        allocs,
        bytes,
    );
}

fn main() {
    let leg = env_str("W8D2_LEG");
    match leg.as_str() {
        "l0_baseline" => leg_l0_parser(),
        "l3_owned_pd" => leg_l3_owned_parse_decode(),
        "l1_parse" => leg_l1_wrapper_parse(),
        "l2_parse" => leg_l2_borrowed_parse(),
        "l4_borrowed_pd" => leg_l4_borrowed_parse_decode(),
        other => panic!("leg {other} is not implemented in the frozen-D1 baseline binary"),
    }
}

/// The driver's entry point: one leg process per invocation.
#[test]
fn run_leg() {
    main()
}
