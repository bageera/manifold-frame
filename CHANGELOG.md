# Changelog

All notable changes to `manifold-frame` (the MNFX container). The
format-grammar normative reference is `docs/mnfx-grammar.md` (W0-frozen;
wire-visible additions are listed there per milestone).

# Unreleased

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