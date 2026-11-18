//! W8-D1 instrument: CRC32C candidate throughput + parse/decode latency.
//!
//! Four implementations (the shipped bitwise reference, byte-lookup,
//! slicing-by-8, slicing-by-16) over payload sizes 64 B .. 64 MiB, then
//! the candidate table forms against complete MNFX parse/decode latency
//! on synthetic containers (plane sizes 1/4/16 MiB). The candidates are
//! PROTOTYPED here (safe Rust, const-eval tables, no deps, no unsafe);
//! promotion moves ONE into the shipped crc32c and this file keeps only
//! the measurement role. Deterministic payload ( xorshift ), warmup +
//! timed reps, medians; every candidate is self-checked per size
//! against the bitwise reference BEFORE timing (a wrong table never
//! gets a throughput number).

use manifold_frame::crc32c as shipped_bitwise;
use std::time::Instant;

pub const POLY: u32 = 0x82F63B78;

// ---- bitwise reference (identical algorithm to the shipped pre-D1 crc32c) ----
pub fn crc32c_bitwise(data: &[u8]) -> u32 {
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

// ---- candidate 1: byte-lookup table (1 KiB) ----
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
pub static T0: [u32; 256] = table0();
pub fn crc32c_lookup(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc = (crc >> 8) ^ T0[(crc ^ u32::from(b)) as usize & 0xFF];
    }
    !crc
}

// ---- candidates 2/3: slicing-by-8 / 16 (8 / 16 KiB) ----
const fn tables_n<const N: usize>() -> [[u32; 256]; N] {
    let mut t = [[0u32; 256]; N];
    let mut k = 0;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut j = 0;
            while j < 8 {
                c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
                j += 1;
            }
            t[0][i] = c;
            i += 1;
        }
        let mut n = 1;
        while n < N {
            let mut i2 = 0;
            while i2 < 256 {
                t[n][i2] = (t[n - 1][i2] >> 8) ^ T0[(t[n - 1][i2] & 0xFF) as usize];
                i2 += 1;
            }
            n += 1;
        }
        k += 1;
    }
    t
}
pub static T8: [[u32; 256]; 8] = tables_n::<8>();
pub static T16: [[u32; 256]; 16] = tables_n::<16>();

pub fn crc32c_slice8(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let (chunks, remainder) = data.as_chunks::<8>();
    for c in chunks {
        crc ^= u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        crc = T8[7][(crc & 0xFF) as usize]
            ^ T8[6][((crc >> 8) & 0xFF) as usize]
            ^ T8[5][((crc >> 16) & 0xFF) as usize]
            ^ T8[4][((crc >> 24) & 0xFF) as usize]
            ^ T8[3][c[4] as usize]
            ^ T8[2][c[5] as usize]
            ^ T8[1][c[6] as usize]
            ^ T8[0][c[7] as usize];
    }
    for &b in remainder {
        crc = (crc >> 8) ^ T0[(crc ^ u32::from(b)) as usize & 0xFF];
    }
    !crc
}

pub fn crc32c_slice16(data: &[u8]) -> u32 {
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
        crc = (crc >> 8) ^ T0[(crc ^ u32::from(b)) as usize & 0xFF];
    }
    !crc
}

// ---- instrument ----
const SIZES: [usize; 9] = [
    64, 128, 256, 1024, 4096, 65_536, 1_048_576, 8_388_608, 67_108_864,
];

fn payload(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    let mut s: u32 = 0x2545_F491;
    for b in v.iter_mut() {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        *b = (s >> 24) as u8;
    }
    v
}

// A plane is a payload: the CRC dominates both parse and decode on raw planes.
fn plane(n: usize) -> Vec<u8> {
    payload(n)
}

fn bench_bytes<F: Fn(&[u8]) -> u32>(name: &str, f: F, data: &[u8], reps: usize) -> f64 {
    let expect = crc32c_bitwise(data);
    // correctness parity FIRST: the answer IS the payload's CRC, not an estimate
    assert_eq!(
        f(data),
        expect,
        "{name} disagrees with the bitwise reference at {} B",
        data.len()
    );
    let mut times = Vec::with_capacity(reps);
    for _ in 0..reps {
        let t = Instant::now();
        let got = f(data);
        let _ = std::hint::black_box(got);
        times.push(t.elapsed());
    }
    times.sort();
    data.len() as f64 / (1 << 20) as f64 / times[times.len() / 2].as_secs_f64()
}

#[test]
fn w8d1_crc_throughput_matrix() {
    println!("W8-D1 throughput (MiB/s, median of 5 at small sizes / 7 at large):");
    println!(
        "{:>9} {:>12} {:>10} {:>10} {:>10}",
        "bytes", "bitwise", "lookup", "slice8", "slice16"
    );
    for &n in &SIZES {
        let data = payload(n);
        let reps = if n <= 65_536 { 25 } else { 7 };
        let bw = bench_bytes("bitwise", crc32c_bitwise, &data, reps);
        let l8 = bench_bytes("lookup", crc32c_lookup, &data, reps);
        let s8 = bench_bytes("slice8", crc32c_slice8, &data, reps);
        let s16 = bench_bytes("slice16", crc32c_slice16, &data, reps);
        println!("{n:>9} {bw:>12.1} {l8:>10.1} {s8:>10.1} {s16:>10.1}");
    }
}

/// Complete MNFX parse/decode latency over synthetic containers whose
/// per-chunk CRC work dominates (raw planes): the e2e attribution rows.
/// Baseline = WHATEVER crc32c is shipped at compile time, so the pre-
/// and post-promotion runs of THIS test yield the before/after pair.
#[test]
fn w8d1_parse_decode_latency() {
    println!("W8-D1 parse/decode latency (Gray8 planes; median of 5):");
    println!(
        "{:>9} {:>12} {:>12} {:>14}",
        "plane", "parse ms", "decode ms", "crc ms"
    );
    for &mb in &[1usize, 4, 16] {
        let n = mb << 20;
        let (w, h) = (2048u32, (n / 2048) as u32); // Gray8: 1 B/px
        let p = plane(n);
        let buf = manifold_frame::serialize(
            &manifold_frame::ContainerHeader {
                pixel_format: manifold_frame::PixelFormat::Gray8,
                orientation: 1,
                chunk_count: 1,
                width: w,
                height: h,
            },
            &[(
                manifold_frame::ChunkType::Pixel,
                manifold_frame::Codec::Raw,
                &p,
            )],
        )
        .unwrap();
        let mut parse_ms = Vec::new();
        let mut dec_ms = Vec::new();
        let mut crc_ms = Vec::new();
        for _ in 0..5 {
            let t = Instant::now();
            let (hdr, chunks) = manifold_frame::parse(&buf).unwrap();
            parse_ms.push(t.elapsed());
            let t = Instant::now();
            let px = manifold_frame::decode_pixels(&hdr, &chunks).unwrap();
            let _ = std::hint::black_box(&px);
            dec_ms.push(t.elapsed());
            let t = Instant::now();
            let _ = std::hint::black_box(manifold_frame::crc32c(&p));
            crc_ms.push(t.elapsed());
        }
        parse_ms.sort();
        dec_ms.sort();
        crc_ms.sort();
        println!(
            "{mb:>7} MiB {p:>12.2} {d:>12.2} {c:>14.2}",
            p = parse_ms[2].as_secs_f64() * 1e3,
            d = dec_ms[2].as_secs_f64() * 1e3,
            c = crc_ms[2].as_secs_f64() * 1e3
        );
    }
}

#[test]
fn w8d1_known_answer_vectors() {
    for (msg, want) in [
        (b"".as_slice(), 0x0000_0000u32),
        (b"123456789".as_slice(), 0xE306_9283),
    ] {
        assert_eq!(crc32c_bitwise(msg), want);
        assert_eq!(shipped_bitwise(msg), want);
        assert_eq!(crc32c_lookup(msg), want);
        assert_eq!(crc32c_slice8(msg), want);
        assert_eq!(crc32c_slice16(msg), want);
    }
    assert_eq!(crc32c_bitwise(&[0u8; 32]), 0x8A91_36AA);
    assert_eq!(crc32c_slice16(&[0u8; 32]), 0x8A91_36AA);
}

#[test]
fn w8d1_randomized_equivalence() {
    // randomized lengths, alignment offsets, trailing-byte coverage:
    // every candidate must equal the bitwise oracle at every offset.
    let mut s: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let blob = payload(70_000);
    for _ in 0..200 {
        let off = (next() % 16) as usize; // alignment offsets 0..15
        for step in 1usize..=4 {
            let len = 1 + ((next() as usize) % 4096) * step; // randomized lengths + trailing bytes
            let len = len.min(blob.len() - off);
            let win = &blob[off..off + len];
            assert_eq!(
                crc32c_lookup(win),
                crc32c_bitwise(win),
                "lookup off={off} len={len}"
            );
            assert_eq!(
                crc32c_slice8(win),
                crc32c_bitwise(win),
                "slice8 off={off} len={len}"
            );
            assert_eq!(
                crc32c_slice16(win),
                crc32c_bitwise(win),
                "slice16 off={off} len={len}"
            );
        }
    }
}
