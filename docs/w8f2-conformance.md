# W8-F2 — historical conformance (release-candidate evidence)

Executed at the F1-complete revision (`b5c6336` tree; the F3 bump after
this run changed only package manifests, the CHANGELOG stamp and
docs — a semantic no-op; the final revision's own CI + release run
re-qualifies everything on the exact release-candidate SHA, per the
requalification rule).

## Required verification rows

| Row | Required result | Executed evidence (release profile, --locked) |
| --- | --- | --- |
| Published v0.1.0/0.1.1-era fixtures | Exact reconstruction | `tests/corpus_embed.rs` 1/1 — the 30-file historical corpus: every container's embedded decode == standalone engine decode, framing exact (380.96 s); `tests/w6e_conformance.rs` 8/8 |
| Embedded MNF8/MNF9 | Standalone == embedded equality | corpus (both engines), the decode unit suites (`46 passed` incl. `embedded_mnf8_equals_standalone_*` both profiles), w6e |
| All six sample formats | Exact samples | `w6e_conformance` (per-format roundtrips), `w8d2_parity` (RGB16 exact assembly + metadata), corpus RGB8 |
| Metadata | Preservation contract retained | `tests/meta_differential.rs` 1/1; W8-A1 arbitration rows inside the `46` (EXIF > ORNT > header) |
| Owning and borrowed APIs | Acceptance + reconstruction parity | `tests/w8d2_parity.rs` 3/3 (error parity incl. pure-appends; pointer identity; rewrite identity) |
| Existing valid MNFX rewrites | Byte identity | parity rewrite-identity rows; capi `rewrite_core_same_length_roundtrips` (4/4) |
| Malformed containers | Controlled rejection | mutation fuzz **350 trials / 240 rejected / 110 accepted / 0 panics**; ABI fuzz **190 / 130 rejected / 60 accepted (all pure appends, benign) / 0 crashes** (seed 2026, distributions IDENTICAL to the frozen pre-W8-E vintages) |
| Legacy ORNT/header mismatches | Documented compatibility retained | W8-A1 legacy-acceptance rows inside the unit suite (orientation, 32-bit walk, rule-13 matrix) |
| i686 hostile lengths | Runtime rejection, debug + release | W8-A3 regression runs in CI on a REAL 32-bit process (both profiles) at the release-candidate SHA (verified by the four-platform CI run) |

## Historical evidence retained (not overwritten)

- `docs/w6d-performance.md` + `docs/w6d-results.json` — the W6-D
  performance measurements stand untouched.
- `docs/w7a-candidate-concurrency.md` + `docs/w7a-results.json` — the
  W7-A NEGATIVE result stands untouched.
- The D/E phase result records (`docs/w8d1-results.json`,
  `docs/w8d2-results.json`, `docs/w8e-dependency-audit.md`) remain as
  their own attributed evidence; F2 introduces no new performance
  claims.

## Suite totals (this gate's run)

46 (lib) + 3 (cli_audit) + 1 (corpus) + 1 (meta_differential) +
1 (rate control) + 2 (W5 acceptance) + 8 (W6-E) + 4 (W7-A probe) +
3 (W7-B1 probe) + 4 (W8-D1 probe) + 3 (W8-D2 parity) +
1 (W8-D2 probe) + 5 (E limits) + 4 (E producer) + 4 (capi) = **90 green,
0 failed**, plus the two fuzz gates above.
