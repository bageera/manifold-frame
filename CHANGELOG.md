# Changelog

All notable changes to `manifold-frame` (the MNFX container). The
format-grammar normative reference is `docs/mnfx-grammar.md` (W0-frozen;
wire-visible additions are listed there per milestone).

# [0.1.1] — 2026-11-05

Published as `manifold-frame` 0.1.1 — no wire-API changes behind the
0.1.0 baseline; the release bundles the W6 interoperability evidence
(conformance tests, in-repo integration gates, third-project release
verification) and the W7-A negative result, plus the refreshed
README. Registry read-back + fresh-consumer smoke verified at 0.1.1.

# 0.2.0 — release candidate (rc1; W8-F; F5 publication in progress)

Release-qualification revision. Wire: MNFX v1, unchanged. The Rust
package moves 0.1.1 → 0.2.0 (additive API volume since 0.1.0: borrowed
parsing, W8-E limits, the checked producer; all additive, the 0.1.x
caller contracts intact). The C ABI crate (publish = false, ships in
the binary archives) moves in lockstep; the C surface — signatures,
status values, caller-owned memory — is unchanged. Historical notes
below were written for [Unreleased] during W8; they land in 0.2.0 as
the release content. A source change after the F4 requalification
triggers another rc stamp and a fresh F4 run.

- W8-F5 — publication sequence opened (F1–F4 qualification complete;
  publication state live in docs/w8f5-publication.md): the production
  tag run caught a latent publish-gate false positive at its first
  execution (the duplicate check keyed on version alone; the contract
  ships one version across two platform archives) and REFUSED to
  publish — correct guard behavior, zero side effects (no draft, no
  release; verified). Fixed at 62535b4: the check keys on
  (version, target) with one archive per platform, and the drafted
  release notes carry the two standing qualifications verbatim (MNF9
  resource exposure is an exposure proxy, not a strict bound on the
  engine's internal working memory or execution time; fuzz parity is
  acceptance parity across the tested mutation sets, not a universal
  proof). Packaged-inputs requalification: the diff of the packaged set
  (manifests, sources, headers, grammar, README, LICENSE) between the
  qualified revision and the fix is ZERO lines — the candidate's
  shipped content is byte-for-byte unchanged. The GitHub release stays
  unpublished until the registry publication and read-back complete.
- W8-F1 — retained CLI audit resolved (behavior-pinning CLI changes +
  docs; NO wire change): bare `--mnf8` (and the profile variants) now
  PIN the embedded payload to the MNF8 engine at the requested profile
  — the MNF9 smaller-stream arm stops competing, so a selection flag
  and the embedded magic can never disagree (the flag is no longer a
  silent no-op; usage text documents pin vs the unsigned auto-pick).
  Help-and-error split pinned by tests: `--help`/`-h` → usage on
  STDOUT exit 0; argument errors → usage on STDERR exit 2. The encode
  report's pixel count multiplies in u64 (no u32 wrap/panic near the
  grammar ceiling). The pixels-only CLI contract re-affirmed in the
  usage text (metadata conversion stays a future feature). The C ABI
  early-out semantics pinned in the shipped C consumer doc
  (capi-README.md): a size query establishes the required output
  length only, never payload reconstruction. Executed verification:
  tests/cli_audit.rs 3/3 (spawned-binary e2e; feature-gated on the
  CLI, `required-features = ["cli"]`), C-ABI gate still PASS, clippy
  clean with the CLI feature, rows in docs/w8-remaining-audit.md.
- W8-E4 — dependency audit (recorded negative result; no code change):
  `zstd`/`zstd-sys` already enter TRANSITIVELY through the vendored
  engine's default features (`manifold-codec` → zstd → zstd-safe →
  zstd-sys), so an optional frame-level zstd feature would remove only
  the frame's own call sites while every default-feature consumer keeps
  the identical native link. NO feature flags introduced. Tree evidence
  and consequences recorded in `docs/w8e-dependency-audit.md`.
- W8-E3 — checked zstd plane producer (additive API; uses the EXISTING
  format vocabulary — not a new backend or wire representation): new
  `encode` module with `zstd_plane_chunks(plane, fmt, width, height)`
  and the convenience `zstd_plane_container`. Contract honored: chunk
  types valid per format (PIXEL for the interleaved formats, the
  PIX16-HI/LO pair for planar16), sample length CHECKED against the
  format-derived plane bound (both too-short and too-long reject with
  the bound named), zstd level 19 with the checksum ENABLED, raw_len
  left zero by the frozen grammar (asserted), rule-13 proved through
  the same validator the serializer uses, and serialization through
  `serialize_checked`. Verification: tests/w8e_zstd_producer.rs 4/4 —
  all six supported shapes reconstruct EXACTLY through the W8-E2
  limits-aware reader.
- W8-E2 — resource limits for untrusted containers (additive API; no
  acceptance or wire change): `DecodeLimits` (max_width/height,
  max_pixels, max_decoded_bytes, max_embed_input_bytes,
  max_metadata_bytes, max_chunks, `embed_engine`) is enforced BEFORE
  the matching allocations at three stages: header fields + pixel
  count + declared chunk count right after the header; walked chunk
  count + the metadata aggregate inside the bounded walk; the plane
  ceiling (exact for raw, the zstd decompression bound, planar16
  hi+lo aggregate) plus the per-embed payload budget before the
  engine, with the engine's own ceilings passed THROUGH for MNF8
  (`decode_mnf8_with_limits`). MNF9 embeds: the engine exposes no
  limits-aware API — the payload budget is the exposure proxy and the
  limitation is RECORDED rather than claimed. Additive readers:
  `parse_borrowed_limited`, `parse_limited`, `decode_pixels_limited`,
  `decode_pixels_borrowed_limited`, `decode_image_limited`,
  `decode_image_borrowed_limited`; the unlimited readers are
  behaviorally unchanged (relaxed-limits identity verified). C ABI
  (signature-preserving): `mnfx_decode`'s size query is now the
  resource guard — a caller buffer smaller than the plane returns
  BUF_TOO_SMALL with the exact required size BEFORE the plane is
  materialized or the engine runs. Verification: tests/w8e_limits.rs
  5/5 (boundary arithmetic, embed budget + engine passthrough,
  relaxed-limits behavioral identity, metadata aggregate + chunk
  ceilings), capi early-out unit test, C-ABI gate PASS, mutation +
  ABI fuzz distributions identical to the pre-E2 vintages
  (350/240/110/0, 190/130/60/0 @ seed 2026).
- W8-E1 — structural safety restrictions (no behavior change):
  `#![forbid(unsafe_code)]` crate-level in the core library (safe by
  construction; forbidden, not denied — a future unsafe block is an
  explicit reviewed opt-OUT) and `#![deny(unsafe_op_in_unsafe_fn)]`
  in the C-ABI crate with every raw-pointer operation moved into
  explicitly scoped blocks whose SAFETY comments name the caller
  contracts. Behavior unchanged: C-ABI gate PASS (six formats
  byte-exact, hostile rc=2). Also: tests/w8d2_probe.rs run_leg now
  SKIPS cleanly when W8D2_LEG is absent (the bench-driver-only env;
  the unknown-leg panic contract stays for the driver path).
- W8-D2 — borrowed parsing and decoding (performance + additive API;
  NO wire change): the borrowed representation is now the fundamental
  parser product — `parse_borrowed()` returns `BorrowedChunk<'_>`
  (header + a payload reference INSIDE the input buffer; the historic
  walk, one-to-one, is its implementation) and the owning `parse()` is
  the materializing compatibility wrapper: ONE validation
  implementation, zero duplicated rules (W8-A rules untouched:
  CRC-before-interpretation, rule-13 matrix, duplicates, the W8-A3 u64
  arithmetic, pure-append acceptance). Additive public API:
  `parse_borrowed`, `BorrowedChunk`, `BorrowedPixels` (samples are a
  `Cow`: RAW planes stay a reference inside the input — zero-copy;
  zstd/embed and planar16 assembly materialize by design),
  `decode_pixels_borrowed`, `decode_image_borrowed`; the decode
  interpretation is ONE generic core over chunk views for both
  representations. D2.3 attribution (arm64 runner,
  `scripts/w8d2_bench.py`, one fresh process per leg, medians):
  borrowed parse removes the payload-copy allocations — 64 MiB raw:
  owning 44 allocs / ~470 MB vs borrowed 9 allocs / ~2 KB (parse
  19.5 → 17.6 ms, FASTER not slower); borrowed full decode at
  64 MiB metadata: decode 0.001 ms vs owning 1.39 ms and peak RSS
  264.5 → 198.9 MB (−25 %) — the modeled payload-copy elimination
  without overclaiming total-process halving. D2.4 gates all green:
  acceptance/reject parity incl. pure-append mutations (55 inputs),
  identical error strings, the pointer-inside-input allocation proof,
  six-format sample exactness through both paths, byte-exact metadata,
  lossless-rewrite identity, fuzz parity (mutation 350 / 110 accepted /
  0 panics and ABI 190 / 60 / 0 — distributions IDENTICAL to the
  pre-D2 vintages), owning-API compatibility (full suite green), no
  unsafe in shipped code (the only unsafe is the test-target counting
  allocator wrapper around `std::alloc::System`, instrument-only),
  wire unchanged. Rows: `docs/w8d2-results.json`.
- W8-D1 — CRC32C acceleration (performance; NO wire change): the
  shipped `crc32c` promotes slicing-by-16 — 16 KiB of const-evaluated
  tables, safe Rust, no new dependencies (`tests/w8d1_probe.rs` is the
  candidate instrument). Throughput (arm64 runner, medians): bitwise
  ≈ 0.39–0.45 GiB/s vs slice16 ≈ 2.9–3.5 GiB/s (byte-lookup == bitwise,
  eliminated; slice8 ≈ 2.08 GiB/s). E2e attribution on complete MNFX
  parse (Gray8 raw planes, median of 5): parse 3.16 → 0.59 ms (1 MiB),
  10.05 → 1.35 ms (4 MiB), 58.02 → 4.79 ms (16 MiB) — the CRC was
  ~90 % of parse; decode is sample-assembly bound and unchanged.
  Promotion gates all green: known-answer vectors (incl. `123456789`
  → `0xE3069283`), randomized offset/length/trailing parity against
  the retained bitwise oracle (now `#[cfg(test)]`-only), identical
  serialized files (the 30-file corpus differential + framing gates
  unchanged), full release suite green. Rows:
  `docs/w8d1-results.json`.
- W8-C — CLI correctness: the `mnfx` encoder is a PRODUCER and now
  writes through `serialize_checked` — every emitted container is
  proven against the full parse rule set (the rule-13 codec matrix +
  the ORNT-mirrors-the-header contract) before it touches disk; the
  unchecked `serialize` remains the lossless-rewrite builder
  (tooling). The stale encoder comment claiming source ICC "carries
  transparently" is corrected to the actual v1 contract: the CLI
  encodes PIXELS only — source ICC/EXIF/XMP are NOT carried
  (documented limitation, integrations/README.md). No wire change.
- W8-B4 — independent artifact consumer (reproducibility; no wire or
  ABI change): validation moves entirely outside the producer
  environment. `scripts/b4_consumer_gate.sh` exercises ONLY the
  downloaded archive as a consumer would: a standalone C consumer
  compiled against the packaged header + libs (B4.2), the libvips and
  ImageMagick W6 gates repeated against the packaged ABI in a new
  `MNFX_CONSUMER_ROOT` consumer mode of their `run.sh` scripts (B4.3/
  B4.4), relocatability via moving the extracted tree and re-linking
  (B4.5), the hostile leg through the packaged ABI (B4.6), and a
  `b4-evidence.json` receipt with the archive-hash + platform + every
  gate outcome (B4.7). `MNFX_CONSUMER_ROOT` mode never invokes cargo —
  and the gate's linkage audit fails if a build path or engine tree
  leaks into any consumer's resolved dependencies (B4.1's teeth). The
  release workflow gains a `consumer-b4` stage (per required target,
  deps installed runner-side; evidence uploaded per leg). VERIFIED on
  the runners (dry run 44077695255 at b71e476): B4.1–B4.7 pass on BOTH
  required platforms — standalone C consumer (six formats), libvips +
  ImageMagick W6 gates against the packaged ABI with the linkage audit
  clean, relocatability, hostile leg, `b4-evidence.json` receipts
  uploaded (`producer_environment_used: false`). The publish stage
  additionally stages a DRAFT release, uploads every asset,
  re-downloads and byte-verifies them against the manifest, and only
  then flips the draft public (draft-staged publication; GitHub asset
  uploads are not transactional).
- W8-B3 — binary distribution pipeline (reproducibility; no wire or
  ABI change): `workflow_dispatch`-dry-run + `v*`-tag release workflow
  building NATIVE-only archives (the packager hard-refuses a
  cross build and records target and runner architecture separately):
  Linux x86-64, macOS ARM64, macOS x86-64 (experimental leg — Intel
  runner retirement drifts; cannot block the required targets).
  Archive contract: `bin/mnfx`, `include/mnfx.h`,
  `lib/libmanifold_frame_capi.(dylib|so)` + `.a`,
  `share/mnfx/{mnfx-grammar.md, capi-README.md}`, LICENSE, README.md,
  `provenance.json` (every W8-B1 factor + git tag/commit, wire v1,
  features, and the otool -L / ldd shared-library lists), internal
  `SHA256SUMS`. macOS dylibs ship with install_name `@rpath`
  (recorded in provenance). The validator tests the SHIPPED files as a
  consumer: extraction, internal checksums, architecture matches the
  label, packaged header symbols vs exports, C consumers linked
  against BOTH the packaged dynamic and static libraries, six-format
  conformance through the packaged library, hostile-input controlled
  error, CLI encode→decode→verify from outside the tree —
  `scripts/validate_release.py`. Publication is tag-gated and atomic:
  all required platforms build AND validate before `publish` runs
  (release notes carry external SHA256SUMS). A dry run of the
  aarch64-apple-darwin leg passed end to end locally (all ten
  validator legs). VERIFIED on the runners (dry run 44077695255,
  archives pinned to b71e476): both required platforms build + all ten
  validator legs pass + artifacts downloadable + publish skipped. The
  Intel (macos-13) leg is OPT-IN (`include_intel_x86=true`): the
  retired-label fleet queued 9 h with no runner assigned on the first
  dry run — recorded as UNAVAILABLE (retryable any time).
- W8-B2 — native macOS CI + MSRV (reproducibility; no wire change):
  CI gains a macOS job — vendoring, provenance (the runner's actual
  architecture is RECORDED, never assumed from `macos-latest`),
  fmt + clippy --locked, lib tests + W6-E conformance --locked, and
  the C-ABI gate end to end (the `.dylib` builds and links a real C
  consumer: the cc smoke via `integrations/c-abi/run.sh`, whose
  hardcoded toolchain PATH is replaced by a resolvable-cargo
  fallback). A new MSRV job pins rustc 1.91 and builds/tests the
  LOCKED dependency tree — a failure names the offending dependency;
  the MSRV never raises automatically. Linux x86-64 and i686 gates
  unchanged.
- W8-A3 — parser arithmetic safety (correctness/security; NO wire
  change): every offset+length sum in the chunk walk is computed in
  u64 BEFORE any usize slice. The wire u32 `stored_len` can no longer
  enter usize arithmetic: on 32-bit targets the unchecked
  `pos + stored_len` wrapped, silently PASSED the bounds check, and
  panicked the payload slice (grammar rule 6 — malformed input must
  never panic). Rejection is a controlled `Err`. Adjacent sites
  audited: the chunk-header bound (safe by the slice-size invariant,
  width-guarded anyway), the pos advance (derived from the guarded
  end), payload_offset (payload-length based), serialize's
  u32::try_from gates, decode's checked_mul plane bound, the capi's
  try_from conversions. Regression: hostile stored_len = 0xFFFF_FFFD
  rejects clean in every build profile; the i686 CI job EXECUTES the
  hostile fixture on a real 32-bit process (debug + release) —
  `cargo check` alone proves nothing about runtime arithmetic.
- W8-B1 — deterministic engine resolution (reproducibility; no wire
  change): the committed `Cargo.lock` pins manifold-codec 1.16.0; CI
  vendors EXACTLY the lock-pinned engine via `scripts/vendor_engine.py`
  (crates.io download; sha256 verified against the API-published
  checksum AND the transferred bytes; refuses to clobber a real git
  clone) and every resolution-sensitive CI step runs `--locked` — the
  `cargo search` latest-version gamble is gone (the vendored 1.16.0 src
  is byte-identical to the local sibling, verified). `scripts/
  write_provenance.py` records a build's full provenance (engine
  version + sha, lockfile sha, rustc/cargo, target triple, git
  revision): the gate vintage committed as `docs/w8b1-provenance.json`,
  per-run artifact uploaded in CI. Local sibling development remains an
  explicit override: the path dep drives dev builds only — the packaged
  manifest stays registry-pinned, and any resolution drift alarms via
  `--locked`.
- W8-A — Format hardening (correctness + grammar alignment; NO wire
  change; ships as a crate patch release with the validation notes):
  - A1: orientation ARBITRATION corrected (rule 12): resolution is
    EXIF tag 0x0112 > ORNT chunk > header field > default 1,
    independent of chunk order. The pre-W8 parser resolved
    last-chunk-wins — an ORNT chunk placed after a tagged EXIF
    overrode it. Acceptance-matrix fixtures (all six rows), both TIFF
    byte orders, both chunk orders, lossless rewrite pinned.
  - A2: the codec/chunk compatibility matrix is normative (new rule
    13; shared implementation `validate_codec_compatibility`):
    metadata chunks (EXIF/XMP/ICC/CICP/ORNT) are raw-coded only;
    PIX16-HI/LO reject embed codecs at parse (was decode); PIXEL
    embeds require pixel_format 1 (RGB8) at parse (was decode).
    `serialize_checked` added — serialize + full-parse proof of the
    emitted bytes (one rule set, no drift), plus the producer-only
    ORNT-mirrors-the-header contract. Validation correction:
    compressed or embed-coded metadata containers that parsed before
    W8-A reject at parse now; decode-driven consumers (incl. the C
    ABI's FORMAT_INVALID mapping) see identical outcomes.
- W7-A (measurement-only): the encoder candidate evaluation is
  already sequential (no rayon/threads in the engine encode path;
  the lattice = a plain loop: balanced 32 arms, maximum 160). The
  88-99 MB encode RSS is NOT candidate concurrency: the full balanced
  lattice's peak sits at one zstd-19 arm's working set (~86 MB,
  near-size-independent at the measured sizes; the mix6 arm costs
  only ~37 MB) — buffers are per-arm transient, no accumulation.
  Lever identified: the per-arm zstd-19 encode tables; a mix-only
  balanced profile is byte-changing ⇒ NEW backend/profile id
  (engine-side decision, queued). Probes: tests/w7a_probe.rs +
  scripts/w7a_bench.py → docs/w7a-results.json +
  docs/w7a-candidate-concurrency.md. Wire untouched.
- W7-B1 (measurement-only, engine v1.15.0 tree 586370e — the W7-A
  vintage): the zstd-19 arm's working set is LEVEL-TABLE-driven — it
  is the level-19 match-table workspace (chainLog 24), not
  window-driven, not plane-sized, not engine-shared; the only measured
  material lever is pledging the source size; bounded-window-only is
  a negative result. The balanced−zstd residual (2,768 KiB of 13,904
  KiB) stands recorded-unexplained. The engine-side decision (any
  byte-changing variant needs a NEW backend/profile id) stays queued
  for the engine's W7-B2/B3. Instruments: tests/w7b1_probe.rs +
  scripts/w7b1_attribution.py → docs/w7b1-results.json +
  docs/w7b1-rss-attribution.md. Wire untouched.
- W6 — Interoperability. W6-A: an out-of-tree consumer repo
  (`/tmp/mnfx-adapter`) builds from the published crates.io crates
  alone (PNG→MNFX→PNG by pixel-plane equality; the adapter lesson:
  the engines embed RGB8 planes only — RGBA/16-bit containers ride
  raw/zstd paths). W6-B: the libvips loader through the shipped C ABI
  (in-pipeline pixel equality via subtract+abs, thumbnail op, hostile
  leg rc==1). W6-C: the MagickCore importer (import→export→memcmp ==
  the wire plane for rgba8/gray16/rgb16; 16-bit = explicit BE→LE
  swap in both directions; hostile leg rc==1). W6-D: the
  measurement-only baseline (`scripts/w6d_bench.py` →
  `docs/w6d-results.json` + `docs/w6d-performance.md`: D1 bytes +
  encode/decode medians + peak RSS per class; D2 in-memory C-ABI
  decode legs 1.0–3.4 ms). W6-E: the durable in-repo forms —
  `tests/w6e_conformance.rs` (all six formats sample-exact, incl.
  raw/zstd/embed RGB8 arms; CI-scope) + `integrations/` (the c-abi /
  vips / magick bridges with self-contained run gates) +
  `scripts/consume_release.sh` (fresh third-project build + run of
  the published crate).
- W5-D: the C ABI (`capi/`): `mnfx_decode` (any MNFX container → the
  full sample plane; size-query idiom; pixel_format/orientation
  outputs optional) and `mnfx_rewrite_chunk` (replace one chunk's
  payload preserving every other byte; CRC32C recomputed in-crate) —
  status-code contract mirroring `manifold-codec-capi`, never panics
  on caller data, no exported allocator (memory ownership stays
  caller-side). Gates: the cc smoke (real compile + link of a C
  program against the staticlib + header, in-process via the test
  binary) and the ABI mutation fuzzer `scripts/abi_fuzz.py` (measured
  180 trials / 0 crashes / accepted set = pure appends only — the
  audit rule holds across the ABI). CI: the matrix gains the i686
  32-bit type-check (usize-width-sensitive arithmetic) and the
  independent-consumer build against the vendored published engine
  source. The capi crate stays `publish = false` until its dependency
  bump decides otherwise.
- Lib surface for the ABI: `ChunkType::raw()` is public
  (re-serialization needs the wire u16), `Error` and `Chunk` are
  re-exported at the crate root. No behavior change.

# 0.1.0 — 2026-11-01

First public release: the MNFX v1 container, W0–W5, published to
[crates.io](https://crates.io/crates/manifold-frame).

- W0: the versioned container grammar (18-B header, 16-B chunk headers,
  CRC32C validated before parse, critical/optional chunk scopes,
  exactly-one pixel accounting per format) + the bounded parser;
  malformed input = `Error`, never panic.
- W1: the MNF8/MNF9 embed layer — embedded decode byte-identical to
  the standalone decode, +34 B framing exact across the 30-file corpus
  gate (`tests/corpus_embed.rs`).
- W2: the RGBA8/Gray8/GrayA8 plane vocabulary, stride-derived plane
  bounds, and the hardened zstd decode (mandatory checksum,
  window_log_max 23, take-limit bomb check).
- W3: RGB16/Gray16 hi/lo plane pairs with big-endian interleave (PNG
  wire order). W3-R1 rate control measured the split vs the
  interleaved-zstd control (interleaved wins the photo class ~44%;
  both shapes stay vocabulary, grammar rule 10).
- W4: the metadata surface — EXIF/XMP/ICC/CICP/ORNT preserved
  byte-exact with chunk order (duplicate known chunks reject), color
  precedence ICC > CICP, orientation EXIF tag > header field >
  default 1, boundary-only validation; the W4-C differential test.
- W5: the integrated encoder via the `mnfx` CLI (feature `cli`):
  automatic engine pick (MNF8 profile arm vs MNF9 whole-frame arm at
  complete-file size), decode/verify commands; the mutation fuzzer
  (`scripts/fuzz.py`, 340 trials / 0 panics / accepted set = pure
  appends only).
- Packaging: the packaged manifest pins `manifold-codec = "1.14"` and
  the registry-version build passed a fresh-project consumer smoke
  (serialize/parse/decode_pixels roundtrip, +34 B framing assert).

Wire-consistency note recorded with this release: zstd and embed
chunks declare raw_len = 0 (shipped shape since W1/W2); the field is
normative for raw chunks only (grammar rules 3–5, 9).