//! abi_fuzz_runner (W5-D): one in-process `mnfx_decode` pass per input
//! file — query pass, undersized copy, sized copy. Any panic, abort, or
//! segfault inside the ABI kills this process, which the Python driver
//! reads as the alarm; every status is flushed to stdout BEFORE the
//! next input, so a crash pins its file.

use manifold_frame_capi::{mnfx_decode, STATUS_BUF_TOO_SMALL, STATUS_INTERNAL, STATUS_OK};
use std::{env, io::Write};

fn main() {
    let dir = env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: abi_fuzz_runner <dir-of-mnfx>");
        std::process::exit(2)
    });
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .expect("read inputs dir")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mnfx"))
        .collect();
    paths.sort();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for p in paths {
        let buf = std::fs::read(&p).expect("read input");
        // SAFETY: buf is a full allocation owned on this thread; the
        // call reads only inside buf and writes only the locals below.
        let st = unsafe { run_decode(&buf) };
        writeln!(out, "{} {}", p.file_name().unwrap().to_string_lossy(), st).unwrap();
        out.flush().unwrap();
    }
}

/// The three-pass decode contract check.
///
/// # Safety
/// `buf` must be a valid allocation (the caller just built it with
/// `fs::read`); the write targets are all function-local.
unsafe fn run_decode(buf: &[u8]) -> i32 {
    let mut need = 0usize;
    let mut w = 0u32;
    let mut h = 0u32;
    let mut pf = 0u8;
    let mut or = 0u8;
    // 1) Query pass with the optional outputs NULL (their skip branch).
    let st = mnfx_decode(
        buf.as_ptr(),
        buf.len(),
        std::ptr::null_mut(),
        &mut need,
        &mut w,
        &mut h,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    if st != STATUS_BUF_TOO_SMALL && st != STATUS_OK {
        return st;
    }
    // A zero-width/height container yields need == 0 and COMPLETES in
    // the query pass (0 < 0 is false) — nothing further to exercise.
    if need == 0 {
        return st;
    }
    // 2) Undersized copy pass: the bound check must fire, never copy.
    let mut short = vec![0u8; need - 1];
    let mut n = short.len();
    let st = mnfx_decode(
        buf.as_ptr(),
        buf.len(),
        short.as_mut_ptr(),
        &mut n,
        &mut w,
        &mut h,
        &mut pf,
        &mut or,
    );
    if st != STATUS_BUF_TOO_SMALL {
        return st;
    }
    // 3) Sized copy pass: structurally valid = OK with n == need.
    let mut full = vec![0u8; need];
    let mut n = full.len();
    let st = mnfx_decode(
        buf.as_ptr(),
        buf.len(),
        full.as_mut_ptr(),
        &mut n,
        &mut w,
        &mut h,
        &mut pf,
        &mut or,
    );
    if st != STATUS_OK {
        return st;
    }
    if n != need {
        return STATUS_INTERNAL;
    }
    STATUS_OK
}
