# MNFX W7-A — encoder candidate evaluation & peak memory

Measurement-only. The memo's W7-A hypothesis — engine candidate
evaluation runs concurrently, inflating encode peak RSS — is falsified
by source inspection AND by decomposition measurement.

## Source evidence (engine tree, clean at v1.15.0, commit 586370e)

- No rayon / par_iter / thread-spawn anywhere in the encode path
  (`src/mnf8.rs`, `src/mix_coder.rs`, `src/mnf6.rs`).
- `encode_mnf8` drives its candidate lattice in a PLAIN FOR LOOP:
  Balanced = 1 backend (zstd19) × 4 predictor arms × 2 planar × 4
  reference fields = **32 sequential full encodes**; Maximum = 5
  backends = **160**. The `mnfx` CLI is likewise sequential (mnf8
  lattice, then the mnf9 arm, then the smallest-ship pick).

## Measured decomposition (M2 Pro; one leg per cargo-test process)

Median of 1 warm-up + 3 timed runs; peak RSS via fresh-process
`RUSAGE_CHILDREN.ru_maxrss`. The constant test-binary overhead cancels
across legs (same binary per image). Full record + environment in
`w7a-results.json`.

| leg (one arm vs full profile) | photograph 27k px | high_entropy 65k px | bytes identity |
| --- | --- | --- | --- |
| l1 — one plain zstd-19 arm | 0.062 s · **85,744 KB** | 0.063 s · **86,064 KB** | exact |
| l2 — one mix6 (BACKEND6) arm | 0.128 s · **37,040 KB** | 0.411 s · **37,104 KB** | exact |
| l3 — full balanced lattice (32 arms) | 5.143 s · 88,512 KB | 7.222 s · 99,968 KB | exact, run-stable |
| l4 — full maximum lattice (160 arms) | 30.983 s · 89,008 KB | 61.822 s · 99,808 KB | exact, run-stable |

## Findings

1. **"Sequential candidate selection" is not a variant — it is the
   current behavior.** The already-shipped lattice is sequential, and
   the measured RSS confirms no accumulation: the full 32-arm balanced
   lattice (88.5 / 100 MB) sits at the size of ONE zstd-19 arm
   (85.7 / 86.1 MB) plus engine constants, not 32× it. Each arm's
   buffers are per-iteration transient.
2. **The per-arm working set is the memory lever.** zstd-19's own
   encode tables cost ~85 MB nearly independent of input size at the
   measured sizes (level 19's fixed chain/search parameters); the mix
   coder arm costs only ~37 MB. Maximum's extra arms add ~6% RSS over
   balanced's, while costing 6x time — matching the W6-D marginal.
3. **The mix coder is the memory-cheaper single arm** (37 vs 86 MB).
   A "mix-only balanced profile" would be a byte-changing encode
   policy — per the standing rule that is a NEW backend/profile id
   (engine-side decision), not a silent change. Flagged for the next
   research step, not implemented here.
4. Wire contract: unchanged (no encode-path edits anywhere; both
   profiles' containers bit-identical across runs against the
   1.15.0-clean engine tree).

## Disposition

- W7-A closes measurement-only: hypothesis dead, structure measured.
- Next: W7-B (allocation/scratch-buffer lifetimes — with the lever
  identified as zstd-19's per-arm tables and the mix arm's 37 MB as
  the low-memory template) or the engine-side bounded-memory profile
  decision. No codec or frame changes made by this gate.
