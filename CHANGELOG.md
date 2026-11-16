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

# Unreleased

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
  deps installed runner-side; evidence uploaded per leg). The publish
  stage additionally stages a DRAFT release, uploads every asset,
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
  validator legs).
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