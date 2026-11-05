# W6-E release verification — 2026-11-05

Deliverable 5 of the W6-E gate: a clean external build + run of the
published components, performed by a THIRD project with no Manifold
checkouts and no path dependencies.

## What ran

`scripts/consume_release.sh` (commit f56cdcc+): in a fresh mktemp
project,

1. registry dependency: `cargo add manifold-frame@0.1.0` (crates.io
   only — the packaged manifest pins `manifold-codec = "1.14"`);
2. lib-level gate in the consumer's `src/main.rs`: synthetic 4x2 RGB8
   plane -> `manifold_frame::serialize` -> `parse` -> `decode_pixels`
   -> byte-equality assert on the samples;
3. CLI gate via `cargo install manifold-frame@0.1.0 --features cli
   --root <temp-prefix>` (the shippable model: an installed binary):
   synthesized stdlib-only 16x16 PNG -> `mnfx encode` (mnf8, 98 B
   container, +34 framing) -> `mnfx decode` -> `mnfx verify` ==
   PASS (exact RGB8 reconstruction).

## Result (real run)

```
mnfx encode in.png mnf8: 98 B (+34 framing), 256 px, flags=0xe0, 64
mnfx decode out.mnfx 16x16 → restored.png
PASS: exact RGB8 reconstruction (16x16)
release verification: /tmp/mnfx-release-check.X4hB2O
CONSUMER RUN OK
RC=0
```

Script history (all real runs, reproducible): run 1 failed on the
stdlib PNG writer (bytes-of-tuples TypeError) — fixed with
struct.pack rows; run 2 failed because a consumer can't build a
dependency's bin target (`cargo build --bin mnfx` is scope-invalid) —
replaced with `cargo install --root`, which is the model an external
adopter actually uses; run 3 green, RC=0.

## Scope

- Crate publication: the v0.1.0 vintage stays the published baseline
  (crates.io `manifold-frame 0.1.0`, tag `v0.1.0`); W6-E changed no
  wire bytes and published no new crate version.
- C-ABI distribution remains the deferred packaging decision (capi
  stays `publish = false`); its integration evidence lives in
  `integrations/` with self-contained gates.
