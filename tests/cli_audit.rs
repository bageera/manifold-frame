//! W8-F1: the resolved CLI audit items, executed against the real
//! binary (spawned via CARGO_BIN_EXE). Rows of
//! docs/w8-remaining-audit.md's resolution:
//! 1  --mnf8 selection agrees with the embedded payload magic
//! 2  help-and-error split pinned (--help/-h: stdout, exit 0;
//!    bad args: stderr, exit 2)
//! 3  the pixel-count report is geometry-exact (u64 arithmetic; the
//!    frame-side u32-boundary arithmetic is covered by w8e_limits
//!    and the CI-real-i686 hostile-length regression)
//! 4  the pixels-only conversion contract is printed in the usage
//! (the C-ABI early-out pin lives in capi/README.md and the W8-E2
//! capi unit test; noted in the audit doc, not re-tested here.)
//!
//! The whole file is feature-gated: the mnfx binary sits behind
//! `required-features = ["cli"]`, so this target only exists (and only
//! passes) when the CLI feature is on — runners must carry it.

#![cfg(feature = "cli")]

use manifold::codec::Dims;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug)]
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(bin: &str, args: &[String]) -> Run {
    let o = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    Run {
        code: o.status.code().unwrap(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// A real PNG fixture through the engine boundary (valid filtered
/// output; the same writer the CLI decode uses).
fn write_png(path: &Path, w: u32, h: u32) {
    let dims = Dims {
        width: w,
        height: h,
    };
    let pixels: Vec<u8> = (0..w as usize * h as usize * 3)
        .map(|i| ((i * 37 + 11) % 256) as u8)
        .collect();
    manifold::img::save_png(path, dims, &pixels).unwrap();
}

fn path_str(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

#[test]
fn help_split_pinned() {
    let bin = env!("CARGO_BIN_EXE_mnfx");
    // Audit #2: --help/-h to STDOUT exit 0; bad args to STDERR exit 2.
    for flag in ["--help", "-h"] {
        let args = vec![flag.to_string()];
        let r = run(bin, &args);
        assert_eq!(r.code, 0, "{flag}: stderr={}", r.stderr);
        assert!(r.stdout.contains("usage:"), "{flag}: {r:?}");
        assert!(r.stderr.is_empty(), "{flag}: {r:?}");
        assert!(r.stdout.contains("PIXELS only"), "audit #4: {r:?}");
    }
    let bogus = run(bin, &["bogus-command".to_string()]);
    assert_eq!(bogus.code, 2);
    assert!(bogus.stderr.contains("usage:"), "{bogus:?}");
    assert!(bogus.stderr.contains("unknown command"), "{bogus:?}");
    let empty = run(bin, &[]);
    assert_eq!(empty.code, 2);
    assert!(empty.stderr.contains("missing command"), "{empty:?}");
}

#[test]
fn mnf8_pin_matches_the_embedded_magic() {
    let bin = env!("CARGO_BIN_EXE_mnfx");
    let tmp = std::env::temp_dir();
    let src = tmp.join("w8f1_src.png");
    write_png(&src, 4, 2);
    // Audit #1: with the pin the embedded codec MUST be MnF8.
    let out = tmp.join("w8f1_pinned.mnfx");
    let args = vec![
        "encode".to_string(),
        path_str(&src),
        "-o".to_string(),
        path_str(&out),
        "--mnf8".to_string(),
    ];
    let r = run(bin, &args);
    assert_eq!(r.code, 0, "stderr: {r:?}");
    let buf = std::fs::read(&out).unwrap();
    let (hdr, chunks) = manifold_frame::parse(&buf).unwrap();
    assert_eq!((hdr.width, hdr.height), (4, 2));
    assert_eq!(chunks.len(), 1);
    assert_eq!(
        chunks[0].header.codec,
        manifold_frame::Codec::MnF8,
        "the pin forces the MNF8 engine"
    );
    // The unsigned selection: the report's engine label must equal the
    // actual payload codec, and the pixel count must be geometry-exact.
    let out2 = tmp.join("w8f1_auto.mnfx");
    let args2 = vec![
        "encode".to_string(),
        path_str(&src),
        "-o".to_string(),
        path_str(&out2),
    ];
    let r2 = run(bin, &args2);
    assert_eq!(r2.code, 0, "stderr: {r2:?}");
    let codec = manifold_frame::parse(&std::fs::read(&out2).unwrap())
        .unwrap()
        .1[0]
        .header
        .codec;
    let expect = if codec == manifold_frame::Codec::MnF9 {
        "mnf9"
    } else {
        "mnf8"
    };
    assert!(
        r2.stdout.contains(expect) && r2.stdout.contains("8 px"),
        "audit #3 (label + exact px count): {r2:?}"
    );
}

#[test]
fn pinned_profile_decodes_exactly() {
    let bin = env!("CARGO_BIN_EXE_mnfx");
    let tmp = std::env::temp_dir();
    let src = tmp.join("w8f1_src2.png");
    write_png(&src, 5, 3);
    let out = tmp.join("w8f1_max.mnfx");
    let args = vec![
        "encode".to_string(),
        path_str(&src),
        "-o".to_string(),
        path_str(&out),
        "--mnf8=maximum".to_string(),
    ];
    let r = run(bin, &args);
    assert_eq!(r.code, 0, "stderr: {r:?}");
    let buf = std::fs::read(&out).unwrap();
    let (_, chunks) = manifold_frame::parse(&buf).unwrap();
    assert_eq!(chunks[0].header.codec, manifold_frame::Codec::MnF8);
    let v = run(bin, &["verify".to_string(), path_str(&src), path_str(&out)]);
    assert!(
        v.stdout.contains("PASS: exact RGB8 reconstruction (5x3)"),
        "{v:?}"
    );
}
