# manifold-frame

**MNFX** — a versioned lossless image container that wraps the
[Manifold](https://github.com/bageera/manifold) compression streams
(the `manifold-codec` crate = the MNF5–MNF9 encoder/decoder engines).
The wrapper adds image semantics at the format boundary — alpha,
16-bit samples, color profiles, metadata, orientation — without
modifying the engines: an MNF8/MNF9 payload embedded under MNFX decodes
byte-identically to its standalone decode. Design per ADR-009 and its
review in the manifold repo (`docs/adr-009-mnfx-review.md`).

## Status: W8-A complete (format hardening); W8-B1 reproducibility landed

Verification board:

| Gate | Evidence |
| --- | --- |
| Reproducibility (W8-B1) | `Cargo.lock` committed (engine pinned 1.16.0); CI vendors exactly the lock-pinned engine via `scripts/vendor_engine.py` (sha256-verified) and runs `--locked`; build provenance in `docs/w8b1-provenance.json` + per-CI-run artifact |
| Native CI + MSRV (W8-B2) | macOS runner job (vendored lock-pinned engine, `--locked` gates, cc smoke against the shipped `.dylib` via `integrations/c-abi/run.sh`) + a rustc 1.91 MSRV job over the committed lockfile; runner architecture recorded in the provenance artifact |
| Parser arithmetic (W8-A3) | Chunk-walk offset+length sums computed in u64 before any usize slice (`src/parser.rs`); hostile stored_len regression (`hostile_stored_len_cannot_wrap_the_bounds_check`) executed on a real 32-bit process in CI, debug + release |
| Binary distribution (W8-B3) | Verified on the runners (dry run 44077695255, commit b71e476): Linux x86-64 + macOS ARM64 build and pass all ten validator legs; `consumer-b4` green; artifacts downloadable; publish skipped. Intel leg opt-in (`include_intel_x86=true`) — UNAVAILABLE on the first dry run (macos-13 queued 9 h, no runner) |
| Independent consumer (W8-B4) | Executed green on both required platforms (same run): standalone C consumer (6 formats), libvips/ImageMagick W6 gates against the packaged ABI (linkage audit clean), relocatability, hostile leg, `b4-evidence-<target>` receipts with `producer_environment_used: false`; `consumer-b4` stage runs from the downloadable artifacts — no Rust build, no engine tree |
| CRC32C (W8-D1) | slicing-by-16 promoted (16 KiB const tables, no deps/unsafe): CRC 0.45 → 2.9–3.5 GiB/s; complete parse 58 → 4.8 ms at 16 MiB (CRC was ~90% of parse); bitwise oracle retained under test configuration; candidate matrix + attribution rows in `docs/w8d1-results.json` |
| Producer + audit (W8-E) | checked zstd plane producer (`encode::zstd_plane_chunks`/`zstd_plane_container`: bound-checked sample length, checksum enabled, raw_len zero, rule-13-proven, serialized through `serialize_checked`) — exact reconstruction on all six supported shapes; dependency audit recorded NEGATIVE (zstd already transitive via the engine; no feature flags) in `docs/w8e-dependency-audit.md` |
| Safety + limits (W8-E) | core crate `#![forbid(unsafe_code)]` + C-ABI crate `#![deny(unsafe_op_in_unsafe_fn)]` with every raw-pointer op explicitly scoped+SAFETY-documented; additive `DecodeLimits` enforced BEFORE the matching allocations (header fields, pixel count, chunk count, metadata aggregate, plane bound, per-embed payload budget) with MNF8 engine ceilings passed through and the MNF9 limitation recorded; C-ABI size query doubles as the resource guard (BUF_TOO_SMALL before any plane materialization) |
| Borrowed parsing (W8-D2) | `parse_borrowed`/`BorrowedChunk` (the ONE validation walk; `parse()` = the materializing wrapper) + generic decode core: 64 MiB raw parse 44 allocs/~470 MB → 9 allocs/~2 KB; raw planes decode zero-copy; metadata-heavy 64 MiB full-path RSS −25% (264.5 → 198.9 MB); parity suite proves acceptance/error/pointer identity incl. pure-append mutations; fuzz distributions byte-identical to pre-D2 vintages; rows in `docs/w8d2-results.json` |
| Container conformance (v1) | `tests/w6e_conformance.rs`: all six pixel formats (RGB8, RGBA8, Gray8, GrayA8, RGB16, Gray16) sample-exact through `serialize → parse → decode_pixels`, incl. raw/zstd/embed RGB8 arms; CI-runnable, no testdata |
| CLI + acceptance | `mnfx encode` (engine pick: MNF8/MNF9 race, smallest file ships), `decode`, `verify`; `tests/w5_acceptance.rs` + corruption/fuzz (`scripts/fuzz.py`: 340 trials/0 panics) |
| Corpus | `tests/corpus_embed.rs`: 30/30 files, embedded == standalone == source, +34 B framing exact |
| C ABI | `capi/` (`mnfx_decode`, `mnfx_rewrite_chunk`), status-code contract, ABI fuzz 180/0, `integrations/c-abi/run.sh` |
| libvips adapter | `integrations/vips/run.sh`: in-pipeline pixel equality, thumbnail, hostile leg controlled |
| ImageMagick adapter | `integrations/magick/run.sh`: rgba8/gray16/rgb16 import→export→memcmp == wire plane, thumbnail + PIL cross-check, hostile leg controlled |
| Release verification | `scripts/consume_release.sh`: fresh third-project build/run of the **published** crate (registry-only deps); result in `docs/w6e-verification.md` |
| Performance baseline | W6-D measurement-only: `scripts/w6d_bench.py` → `docs/w6d-results.json` + `docs/w6d-performance.md` (bytes, encode/decode medians, peak RSS, per class + profile) |

W6-D highlights: balanced beats PNG-9 on every structured class
(0.15–0.34) and ties noise; decode 4–8 ms CLI / 1.0–3.4 ms in-memory;
encode RSS ≈ one zstd-19 arm's working set (~86 MB; mix6 arm 37 MB) —
W7-A `REJECTED: concurrency as RSS cause`, details in
`docs/w7a-candidate-concurrency.md`; W7-B1 attributed that working set
to the level-19 match-table workspace (chainLog 24; pledged-src-size
the measured lever, bounded-window a negative —
`docs/w7b1-rss-attribution.md`).

## Use

Rust (container-only crate; the engines come from `manifold-codec`):

```rust
let (chunk, stats, backend) = manifold_codec::mnf8::encode_mnf8(
    &pixels, dims, false,
    manifold_codec::mnf8::Profile::Balanced,
);
let buf = manifold_frame::serialize(
    &header,
    &[(ChunkType::Pixel, Codec::MnF8, &chunk)],
)?;
// ... parse/decode with manifold_frame::parse + decode_pixels
```

CLI (feature `cli`): `mnfx <encode|decode|verify>` — encode a PNG into
an MNF8/MNF9-based MNFX file, decode back (RGB8 via the engines),
verify exact reconstruction. `--mnf8=balanced|maximum` picks the
search profile.

C: `capi/` ships the ABI crate (`manifold-frame-capi`, in-repo; the
shipped dylib + `capi/include/mnfx.h` are the integration contract —
see `integrations/README.md` for the adapter set and their known
limitations).

Metadata: EXIF/XMP/ICC/CICP/OpenEXR orientation preserved
byte-exact with chunk order; duplicates of known metadata chunks
reject; metadata chunks are raw-coded in v1 (grammar rule 13); color
precedence ICC > CICP; orientation = EXIF 0x0112 > ORNT chunk >
header field > 1, independent of chunk order; samples never rotated
at decode.

## Versioning

The wire grammar is frozen for v1; the container publishes under the
`0.1.x` line while the interoperability story hardens. `CHANGELOG.md`
carries the per-gate history. Engine work (including any memory-tuned
profile) stays in `manifold-codec`; the frame depends on it by
semver-compatible version pin, currently `1.14`.
