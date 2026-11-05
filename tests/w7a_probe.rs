//! W7-A probe: RSS decomposition of the encoder's candidate lattice.
//! The memo hypothesized concurrent candidate evaluation; the source
//! says the evaluation loop is ALREADY SEQUENTIAL (a plain `for` over
//! backends x pred arms x planar x reference; no rayon/threads anywhere
//! in the encode path). So the measured question is the memory
//! STRUCTURE: how much of the 88-99 MB is one arm's transient working
//! set vs accumulation across arms — measured as separate cargo-test
//! processes (one leg per process; a constant test-binary overhead
//! cancels across legs).
//!
//! Legs (run individually with `cargo test --release --test w7a_probe
//! <leg> -- --exact --nocapture`; the harness measures each):
//!   l1_zstd19_direct  — one plain zstd-19 frame (W2 shape)
//!   l2_mix6           — one mix-coder (BACKEND6) encode of the plane
//!   l3_full_balanced  — the CLI's exact shape (balanced + mnf9 race)
//!   l4_full_maximum   — the 160-arm maximum lattice
//! Each leg's artifacts + an identity tag land in $W7A_OUT.

use manifold::codec::Dims;
use manifold::mnf8::{encode_mnf8, Profile};
use manifold_frame::{ChunkType, Codec, ContainerHeader, PixelFormat};
use std::io::Write;
use std::path::PathBuf;

fn out_dir() -> PathBuf {
    let d = std::env::var_os("W7A_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/w7a"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn load_image() -> (Dims, Vec<u8>) {
    let img = manifold::img::load(
        &std::env::var_os("W7A_IMG")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from("../manifold/testdata/photos/autumn_192x144_s900001.png")
            }),
    )
    .expect("load W7A_IMG");
    let dims = Dims {
        width: img.width,
        height: img.height,
    };
    println!("{{\"img\":\"{}x{}\"}}", img.width, img.height);
    (dims, img.pixels)
}

fn zstd19(data: &[u8]) -> Vec<u8> {
    // Exactly the engine's compress_stream zstd arm: level 19,
    // include_checksum; NO window knob on the encode side.
    let mut e = zstd::stream::Encoder::new(Vec::new(), 19).unwrap();
    e.include_checksum(true).unwrap();
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn identity_tag(b: &[u8]) -> u64 {
    // Non-crypto run-to-run identity tag (cross-run sha256 comparison is
    // the harness's job via shasum on the written files).
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in b {
        h = (h ^ u64::from(x)).wrapping_mul(0x100000001b3);
    }
    h
}

fn write_artifact(name: &str, tag: &str, bytes: &[u8]) -> u64 {
    let d = out_dir();
    let sha = identity_tag(bytes);
    std::fs::write(d.join(format!("{}_{}", tag, name)), bytes).unwrap();
    sha
}

#[test]
fn l1_zstd19_direct() {
    let (_dims, plane) = load_image();
    let z = zstd19(&plane);
    let sha = write_artifact(
        "zstd19.zst",
        &std::env::var("W7A_TAG").unwrap_or_default(),
        &z,
    );
    println!(
        "{{\"leg\":\"l1\",\"len\":{},\"sha\":\"{:016x}\"}}",
        z.len(),
        sha
    );
}

#[test]
fn l2_mix6() {
    let (_dims, plane) = load_image();
    let m = manifold::mix_coder::mix_encode_with(&manifold::mix_coder::MixParams::BACKEND6, &plane);
    let sha = write_artifact(
        "mix6.bin",
        &std::env::var("W7A_TAG").unwrap_or_default(),
        &m,
    );
    println!(
        "{{\"leg\":\"l2\",\"len\":{},\"sha\":\"{:016x}\"}}",
        m.len(),
        sha
    );
}

/// The CLI's exact encode shape: balanced full lattice + the mnf9 race,
/// wrapped into a container — the reference bytes of the memo's
/// byte-identity requirement (per the encode_cmd flow).
#[test]
fn l3_full_balanced() {
    let (dims, plane) = load_image();
    let tag = std::env::var("W7A_TAG").unwrap_or_default();
    let (mnf8_file, st, backend) = encode_mnf8(&plane, dims, false, Profile::Balanced);
    let (file, codec) = match manifold::mnf9::encode_exact(&plane, dims) {
        Some(f) if f.len() < mnf8_file.len() => (f, Codec::MnF9),
        _ => (mnf8_file, Codec::MnF8),
    };
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 1,
        width: dims.width,
        height: dims.height,
    };
    let buf = manifold_frame::serialize(&hdr, &[(ChunkType::Pixel, codec, &file)]).unwrap();
    let sha = write_artifact("balanced.mnfx", &tag, &buf);
    println!(
        "{{\"leg\":\"l3\",\"bytes\":{},\"len\":{},\"codec\":\"{:?}\",\"backend\":{},\"flags\":{},\"sha\":\"{:016x}\"}}",
        file.len(),
        buf.len(),
        codec,
        backend,
        st.chosen_flags,
        sha
    );
}

/// The 160-arm maximum lattice, same shape.
#[test]
fn l4_full_maximum() {
    let (dims, plane) = load_image();
    let tag = std::env::var("W7A_TAG").unwrap_or_default();
    let (mnf8_file, st, backend) = encode_mnf8(&plane, dims, false, Profile::Maximum);
    let (file, codec) = match manifold::mnf9::encode_exact(&plane, dims) {
        Some(f) if f.len() < mnf8_file.len() => (f, Codec::MnF9),
        _ => (mnf8_file, Codec::MnF8),
    };
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 1,
        width: dims.width,
        height: dims.height,
    };
    let buf = manifold_frame::serialize(&hdr, &[(ChunkType::Pixel, codec, &file)]).unwrap();
    let sha = write_artifact("maximum.mnfx", &tag, &buf);
    println!(
        "{{\"leg\":\"l4\",\"bytes\":{},\"len\":{},\"codec\":\"{:?}\",\"backend\":{},\"flags\":{},\"sha\":\"{:016x}\"}}",
        file.len(),
        buf.len(),
        codec,
        backend,
        st.chosen_flags,
        sha
    );
}
