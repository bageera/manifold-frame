# manifold-frame

**MNFX** — a versioned lossless image container that wraps the
[Manifold](https://github.com/bageera/manifold) compression streams
(the `manifold-codec` crate = the MNF5–MNF9 encoder/decoder engines).
The wrapper adds image semantics at the format boundary — alpha,
16-bit samples, color profiles, metadata, orientation — without
modifying the engines: an MNF8/MNF9 payload embedded under MNFX decodes
byte-identically to its standalone decode. Design per ADR-009 and its
review in the manifold repo (`docs/adr-009-mnfx-review.md`).

## Status: W6 complete (interoperability), W7-A closed

Verification board:

| Gate | Evidence |
| --- | --- |
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
`docs/w7a-candidate-concurrency.md`.

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
reject; color precedence ICC > CICP; orientation = EXIF 0x0112 >
header field > 1; samples never rotated at decode.

## Versioning

The wire grammar is frozen for v1; the container publishes under the
`0.1.x` line while the interoperability story hardens. `CHANGELOG.md`
carries the per-gate history. Engine work (including any memory-tuned
profile) stays in `manifold-codec`; the frame depends on it by
semver-compatible version pin, currently `1.14`.
