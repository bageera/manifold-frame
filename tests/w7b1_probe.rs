//! W7-B1 probe: attribute the zstd-19 encoder working set (RSS).
//! Extends the W7-A one-leg-per-process shape (`tests/w7a_probe.rs`);
//! the question the W7-A disposition deferred: WHERE does a plain
//! zstd-19 arm's ~86 MB working set actually go — level-19 streaming
//! workspace vs window buffers vs plane bytes vs engine-side extras —
//! and is a bounded-window encode variant a material lever?
//!
//! Legs (each run as its own cargo-test process; the constant
//! test-binary overhead cancels across legs of this binary):
//!   zstd_leg        — generic parametrized leg (W7B1_LEVEL / W7B1_WINDOW /
//!                     W7B1_PLEDGE / W7B1_WIDTH / W7B1_HEIGHT): a minimal,
//!                     engine-identical zstd stream encode (level + checksum,
//!                     window knob only when W7B1_WINDOW != 0, pledged
//!                     source size when W7B1_PLEDGE=1) of a SYNTHESIZED
//!                     deterministic noise plane. The harness enumerates
//!                     the attribution matrix through this one leg: level
//!                     ladder (a), window knob (b), pledged bound (b2),
//!                     plane-size scaling (c). Each leg also prints the
//!                     analytic ZSTD_sizeof_CCtx() after the encode —
//!                     no second context is created, so the RSS column stays
//!                     clean. A self roundtrip guards the knob against a
//!                     broken/undecodable frame.
//!   d_anchor_engine — W7-A l1 replication on the current tree (the in-run
//!                     drift anchor, NOT an identity oracle): engine
//!                     `img::load` of the W7A_IMG-style noise PNG + the
//!                     engine-shaped zstd-19 frame (needs --features cli).
//!                     Legs of the generic matrix exercise NO engine code,
//!                     so (anchor − a_l19) isolates the engine-side extra
//!                     (img-load + engine-path allocations) in the W7-A l1
//!                     shape.
//!
//! Artifacts per RUN (tag = leg_run index) land in $W7B1_OUT
//! (default /tmp/w7b1); run-to-run identity is the harness's sha256 job
//! over the 4 kept artifacts.

use manifold::img;
use zstd::zstd_safe::{CCtx, CParameter};
use std::io::Write;
use std::path::PathBuf;

fn out_dir() -> PathBuf {
    let d = std::env::var_os("W7B1_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/w7b1"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn env_or(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Deterministic high-entropy RGB8 noise plane (xorshift64), the same
/// content class as the testdata noise PNG. Synthesized — attribution
/// legs must not run engine code, so engine statics stay out of them.
fn synth_plane(w: u32, h: u32) -> Vec<u8> {
    let n = (w as usize) * (h as usize) * 3;
    let mut v = Vec::with_capacity(n);
    let mut s: u64 = 0x9e3779b97f4a7c15
        ^ (u64::from(w) << 32)
            .wrapping_add(u64::from(h) | 1); // seed never zero
    for _ in 0..n {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        v.push((s >> 33) as u8);
    }
    v
}

fn identity_tag(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in b {
        h = (h ^ u64::from(x)).wrapping_mul(0x100000001b3);
    }
    h
}

/// One engine-identical zstd stream arm, parametrized. Params (level,
/// checksum, optional window cap) are set on a CCtx we own BEFORE the
/// first write — the same CCtx/stream-init path `zstd::stream::Encoder::new`
/// takes — so the frames are byte-identical to the engine's
/// compress_stream zstd arm shape at the same level; the retained CCtx
/// lets the leg report ZSTD_sizeof_CCtx() at no extra allocation.
fn zstd_arm(cctx: &mut CCtx<'static>, level: i32, winlog: u32, pledge: bool,
            data: &[u8]) -> (Vec<u8>, usize) {
    cctx.set_parameter(CParameter::CompressionLevel(level)).unwrap();
    if winlog != 0 {
        cctx.set_parameter(CParameter::WindowLog(winlog)).unwrap();
    }
    cctx.set_parameter(CParameter::ChecksumFlag(true)).unwrap();
    if pledge {
        // Bounded-workspace variant: pledging the source size lets zstd
        // clamp windowLog AND the match-state tables to the plane at
        // stream init (the streaming path otherwise never sees a size).
        cctx.set_pledged_src_size(Some(data.len() as u64)).unwrap();
    }
    let mut e = zstd::stream::Encoder::with_context(Vec::new(), cctx);
    e.write_all(data).unwrap();
    let z = e.finish().unwrap();
    (z, cctx.sizeof())
}

/// Process floor only: the cargo-test binary + runtime with NO zstd
/// context and no plane work — the base the ladder deltas subtract
/// from. (Same artifact convention so the harness keeps identity.)
#[test]
fn null_leg() {
    let tag = std::env::var("W7B1_TAG").unwrap_or_default();
    let path = out_dir().join(format!("{tag}_null.zst"));
    std::fs::write(&path, b"null").unwrap();
    let _ = path;
}

#[test]
fn zstd_leg() {
    let level = env_or("W7B1_LEVEL", 19) as i32;
    let winlog = env_or("W7B1_WINDOW", 0);
    let w = env_or("W7B1_WIDTH", 256);
    let h = env_or("W7B1_HEIGHT", 256);
    let pledge = env_or("W7B1_PLEDGE", 0) != 0;
    let plane = synth_plane(w, h);

    let mut cctx = CCtx::create();
    let (z, ctx_bytes) = zstd_arm(&mut cctx, level, winlog, pledge, &plane);

    // The one real check: a bounded (or unbounded) window must still
    // decode back to the exact plane, or the knob measurement is void.
    let back = zstd::stream::decode_all(&z[..]).expect("zstd decode");
    assert_eq!(back, plane, "zstd leg roundtrip");

    let tag = std::env::var("W7B1_TAG").unwrap_or_default();
    let path = out_dir().join(format!("{tag}_zstd_leg.zst"));
    std::fs::write(&path, &z).unwrap();
    println!(
        "{{\"leg\":\"zstd\",\"level\":{},\"window\":{},\"pledge\":{},\"w\":{},\"h\":{},\"plane\":{},\"len\":{},\"sizeof_cctx\":{},\"sha\":\"{:016x}\"}}",
        level,
        winlog,
        pledge as u8,
        w,
        h,
        plane.len(),
        z.len(),
        ctx_bytes,
        identity_tag(&z)
    );
    let _ = path;
}

/// In-run anchor: the exact W7-A l1 shape (engine img::load + plain
/// zstd-19 arm with checksum, no window knob) on the current tree.
/// Same-tree comparison to W7-A's recorded l1 number is a DRIFT CHECK,
/// not an identity oracle (the frame's engine-tree trap applies).
#[test]
fn d_anchor_engine() {
    let img = img::load(
        &std::env::var_os("W7B1_IMG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../manifold/testdata/noise_256x256.png")),
    )
    .expect("load W7B1_IMG");
    let mut cctx = CCtx::create();
    let (z, ctx_bytes) = zstd_arm(&mut cctx, 19, 0, false, &img.pixels);

    let tag = std::env::var("W7B1_TAG").unwrap_or_default();
    let path = out_dir().join(format!("{tag}_anchor.zst"));
    std::fs::write(&path, &z).unwrap();
    println!(
        "{{\"leg\":\"anchor\",\"w\":{},\"h\":{},\"plane\":{},\"len\":{},\"sizeof_cctx\":{},\"sha\":\"{:016x}\"}}",
        img.width,
        img.height,
        img.pixels.len(),
        z.len(),
        ctx_bytes,
        identity_tag(&z)
    );
    let _ = path;
}
