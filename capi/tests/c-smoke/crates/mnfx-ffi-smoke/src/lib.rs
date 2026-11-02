//! The cc-smoke harness: smoke.c is compiled and linked into THIS test
//! binary by build.rs (cc-rs + the manifold_frame_capi staticlib) — no
//! subprocess, no compiler probe. The test calls the C main through
//! FFI; a Rust panic crossing back (or an abort inside the ABI) fails
//! the gate, which is the W5-D property.

extern "C" {
    /// The smoke program's entry point (renamed in smoke.c to dodge
    /// the host binary's own `main`).
    fn mnfx_smoke_main() -> i32;
}

#[cfg(test)]
mod gate {
    use super::mnfx_smoke_main;

    #[test]
    fn c_smoke_passes_in_process() {
        // SAFETY: the compiled-in C main is a plain function; it reads
        // no rust-owned state and returns a process-style exit code.
        let rc = unsafe { mnfx_smoke_main() };
        assert_eq!(rc, 0, "c-smoke reported failures (rc = {rc})");
    }
}