# MNFX W7-B1 — zstd-19 encoder working-set attribution

Measurement-only. The W7-A disposition deferred the question of WHERE
a plain zstd-19 arm's ~86 MB working set actually goes — level-19
streaming workspace vs window buffers vs plane bytes vs engine-side
allocations — and whether a bounded-window/bounded-workspace encode
variant is worth an engine-repo decision. This record decomposes it
with a parametric one-leg-per-process matrix. No lib/encode/wire code
was touched; both repos left uncommitted at HEAD
(frame `1853418c11fa`, engine tree `586370e`, v1.15.0 — the same
engine tree W7-A measured).

## Instrument

`tests/w7b1_probe.rs` + `scripts/w7b1_attribution.py`, extending the
W7-A one-leg-per-process probe shape (`tests/w7a_probe.rs` /
`scripts/w7a_bench.py`). Each leg is its own cargo-test process under
the fresh-python RSS wrapper (`RUSAGE_CHILDREN.ru_maxrss`, bytes → KiB;
1 warmup + 3 timed, medians; the constant test-binary overhead cancels
across legs). Attribution legs a–c SYNTHESIZE the plane (engine code
never runs), leg d replicates the W7-A l1 shape in-run as a drift
anchor (not an identity oracle — the engine-tree trap applies). Every
`zstd_leg` also prints the analytic `ZSTD_sizeof_CCtx()` of the SAME
context the stream uses (params pre-set on an owned CCtx; no second
context, so the RSS column stays clean), and self-roundtrips its frame
(encode→decode→byte-identity) so knob legs are verified, not assumed.

Reproduce (build once, then per leg — the harness does all of this):

```
cargo test --release -p manifold-frame --features cli --test w7b1_probe --no-run
W7B1_LEVEL=19 python3 -c "<wrapper>" cargo test --release -p manifold-frame \
  --features cli --test w7b1_probe zstd_leg -- --exact --nocapture
# or the whole matrix + JSON:
python3 scripts/w7b1_attribution.py   # writes docs/w7b1-results.json
```

## Measured matrix (Apple M2 Pro, Darwin 25.6.0, zstd 0.13.3 / zstd-safe 7.3.0 / zstd-sys 2.1.2+zstd 1.5.7)

Plane for a/b/d: 256×256 deterministic synthesized noise (RGB8,
196,608 B; incompressible class). Full data in `w7b1-results.json`.

| leg | params | RSS KB | sizeof_CCtx B | frame bytes | identity |
| --- | --- | --- | --- | --- | --- |
| a_null_base | process floor, no zstd work | 37,056 | — | — | — |
| a_l3 | level 3 | 37,152 | 3,663,377 | 196,624 | exact |
| a_l10 | level 10 | 37,168 | 25,945,617 | 196,624 | exact |
| a_l19 | level 19 | 86,416 | 93,848,207 | 196,624 | exact |
| b_l19_w23 | level 19, WindowLog(23) | 86,400 | 93,848,207 | 196,624 | exact |
| b_l19_w20 | level 19, WindowLog(20) | 86,384 | 86,508,175 | 196,624 | exact |
| b_l19_w16 | level 19, WindowLog(16) | 85,984 | 84,825,840 | 196,630 | exact |
| b2_l19_pledge | level 19, pledged src size | **37,472** | **5,964,431** | 196,627 | exact |
| c_l19_s64x64 | level 19, 64×64 plane (12,288 B) | 85,040 | 93,848,207 | 12,301 | exact |
| c_l19_s128x128 | level 19, 128×128 plane (49,152 B) | 85,504 | 93,848,207 | 49,165 | exact |
| d_anchor_engine | engine img::load + level 19 (W7-A l1 shape) | 86,224 | 93,848,207 | 196,624 | exact |

Reproducibility: three independent full matrix runs (14:43:10,
14:43:48, 14:44:25 UTC) agree within ±48 KiB on every RSS cell
(e.g. a_l19 = 86,400 / 86,400 / 86,416).

## Decomposition of the ~86 MB

1. **It is the level-19 zstd streaming workspace — nothing else.**
   The level-19 CCctx accounts 93,848,207 B while the process peak RSS
   is 86,416 KiB: the accounted context alone exceeds (is 94–109% of)
   the whole process working set. The zstd 1.5.7 level-19 cParams
   (clevels.h, unknown-srcSize table) are windowLog 23 / chainLog 24 /
   hashLog 22 / btultra2, so the match-state tables are up to
   4·2^24 = 67.1 MB (chain) + 4·2^22 = 16.8 MB (hash) + a 8.4 MB
   window buffer — 92.3 MB, i.e. 93,848,207 measured minus ~1.5 MB of
   seqStore/entropy/streaming buffers. The whale is the 64 MB CHAIN
   TABLE, sized from chainLog=24, NOT from the window.
2. **Touch subset.** RSS records touched-in pages: the level-19 leg
   adds 49,360 KiB over the process floor (37,056). Level 10's
   25.9 MB context adds ~0 KiB and level 3's 3.7 MB context adds ~0 —
   only level 19's table scale faults in materially.
3. **Plane-size slope ≈ 0 (fixed floor, not size-scaled).** Same
   level 19: 85,040 → 86,416 KiB across plane bytes 12,288 → 196,608
   (16×): +1,376 KiB total, i.e. the input/output buffers only; the
   workspace term is constant. The ~86 MB does not scale with input.
4. **Window contribution (explicit `WindowLog` on the ENCODER):**
   context-only, not RSS. Explicit w23 reproduces the level-19 default
   exactly (same 93,848,207 B accounting, same bytes) — control leg
   passes. Explicit w16 moves the accounting by 9,022,367 B ≈ the
   window buffer delta (8,388,608 − 65,536 = 8,323,072) plus ~0.7 MB
   of window-sized streaming buffers, but peak RSS by only −432 KiB.
   Zstd does NOT clamp the match tables to an explicit encoder
   windowLog — the tables follow the level, and the streaming path
   with unknown srcSize keeps level defaults.
5. **Engine-side extra: nil.** The in-run anchor (engine `img::load`
   + the engine-shaped zstd-19 arm, exactly W7-A's l1 shape) is
   86,224 KiB vs 86,416 for the pure synth arm: img-load + engine-path
   allocations are inside run noise. W7-A's mix6 arm (37,040/37,104)
   sits AT the null floor (37,056): every non-zstd-19 arm is
   floor-class; the zstd-19 tables are the only heavyweight.
6. **Drift anchor.** d_anchor_engine 86,224 KiB vs W7-A's recorded
   high_entropy l1 = 86,064 KiB, same engine tree (`586370e`): +160 KiB
   (+0.19%) — stable (drift report, per the documented tree trap).
7. **Pledged src size is the bounded-workspace lever.** Setting
   `set_pledged_src_size(plane.len())` before the stream inits lets
   zstd clamp windowLog AND the tables to the actual input at stream
   init: context 93,848,207 → 5,964,431 B (−93.6%) and peak RSS
   86,416 → 37,472 KiB — the touched level-19 workspace delta over
   the process floor (49,360 KiB) is essentially cancelled outright.
   Frame bytes: 196,624 → 196,627 (+3 B, the
   Frame_Content_Size header field) on this; the frame remains
   default-decodable (self roundtrip exact; the decoder cap
   `window_log_max(23)` is untouched by either variant).

## W7-A residual answer (Balanced − zstd19 = 2,768 / 13,904 KiB)

The zstd attribution does NOT explain the residual — that part of the
W7-A accounting is now bounded off:

- The zstd-19 arm's working set is shown constant (~86 MB) across
  plane sizes (leg c: +1.4 MB over 16× input) and across window knobs
  (leg b: ≤448 KiB). A residual that grows 5.0× when the plane grows
  2.4× (2,768 KiB @ 27k px → 13,904 KiB @ 65k px) cannot be composed
  of size-independent level/workspace/window terms.
- The bounded-window variant is not a residual lever either (its RSS
  effect, −432 KiB, is smaller than the smaller residual).
- Conclusion: the residual lives in engine-path working state (W7-A's
  own candidates: other arms' buffers, retained allocations,
  additional working state). W7-B2 (allocation/scratch lifetimes) is
  the isolating step; this probe's legs a–d cannot see inside the
  32-arm lattice.

## Verdict

- **Bounded WINDOW alone: not worth an engine decision.** RSS −432 KiB
  at WindowLog 16 (bytes +6 B): the window knob moves the 8.4 MB
  window buffer, but the 83.9 MB of match tables do not follow the
  window. A bounded-window backend/profile would buy nothing.
- **Bounded WORKSPACE (pledged src size): material — engine-repo
  decision, flagged, NOT implemented.** The pledged variant drops the
  zstd-19 arm from 86,416 to 37,472 KiB — −48,944 KiB (−56.6% of the
  arm peak; the touched level-19 workspace delta over the process
  floor, 49,360 KiB, is essentially cancelled outright) — at +3 B
  frame cost on the measured plane, frame still default-decodable, no
  decoder-side change needed. BUT the embedded content-size header
  makes it a byte-changing encode policy: per the manifold rule it
  needs a new backend/profile id (or an engine versioning decision),
  and its frame-byte cost on COMPRESSIBLE content (window clamped
  below the plane; here 2^18 ≥ the 196 KB plane so no loss was
  possible) is UNMEASURED — a corpus pass belongs to the engine's
  W7-B2/B3, not this container-scoped probe.

## Disposition

- W7-B1 question answered at the container-scoped attribution level;
  the zstd-19 working set is level-table-driven (chainLog 24), not
  window-driven, not plane-sized, not engine-shared; the only measured
  material lever is pledging the source size; bounded-window-only is
  a negative result. The engine-repo decision (any byte-changing
  variant) is flagged for W7-B2/B3 per the standing rule.
- Wire contract: unchanged — no encode-path edits, no profile changes,
  no artifacts that depend on uncommitted state; all knobs were
  exercised in the probe process only.
- Instruments are additive: `tests/w7b1_probe.rs`,
  `scripts/w7b1_attribution.py`, `docs/w7b1-results.json` (+ this
  file); `tests/w7a_probe.rs` / `scripts/w7a_bench.py` untouched.
- Boundary: measurement-only, this Mac only, one incompressible-plane
  class for a/b/d; c varies size; byte-generality of the pledge lever
  and full-corpus RSS are explicitly out of scope (engine W7-B2/B3);
  sha-vs-historical is a drift report, never an identity oracle.
