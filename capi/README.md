# manifold-frame-capi — the W5-D C ABI for the MNFX container.

Status codes, the size-query idiom, and the never-panics contract mirror
the proven `manifold-codec-capi` shape (sibling precedent). The crate
exports no allocator: callers own every buffer.

## Symbols

- `mnfx_decode` — any MNFX container → the full interleaved sample
  plane (all six W2/W3 formats; embedded formats route through the
  engines). `pixel_format`/`orientation` out-pointers are optional.
  The plane is never rotated; the orientation out is the raw header
  field (rule-12 EXIF precedence is a consumer read).
- `mnfx_rewrite_chunk` — replace one chunk's payload, preserving every
  other byte (the lossless-rewrite contract); CRC32C recomputed
  in-crate, no caller CRC input.

## Gates

- `cargo test -p manifold-frame-capi` — the Rust-cores suite.
- `cargo test -p mnfx-ffi-smoke` — the cc smoke (REAL compile + link
  of `capi/tests/c-smoke/smoke.c` against the staticlib + header).
  Build the staticlib FIRST: `cargo build -p manifold-frame-capi
  --release`.
- `scripts/abi_fuzz.py` — hostile-input gate across the ABI.

## Publish

`publish = false` for now (the sibling capi publishes only after its
dependency is on the registry); flip when manifold-frame next bumps.