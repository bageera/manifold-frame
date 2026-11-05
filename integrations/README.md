# integrations — MNFX interoperability (W6-E)

Application-facing integration evidence for the MNFX v1 container.
Every gate below is regenerable in this repo with no external fixture
repos; each `run.sh` first dumps the published conformance fixtures
(`MNFX_WRITE_FIXTURES=1 cargo test -p manifold-frame --test
w6e_conformance fixtures_write` -> /tmp/w6e/fixtures), then builds and
runs its bridge against the shipped C ABI
(`target/release/libmanifold_frame_capi.dylib` + `capi/include/mnfx.h`).

## Conformance fixtures (tests/w6e_conformance.rs)

Synthetic deterministic sources (8x6, not solid fills) for all six v1
pixel formats; each round-trips serialize -> parse -> decode_pixels
byte-exactly, including the full-codec RGB8 arms (raw + zstd + embedded
MNF8). The fixture dump emits per format: `.mnfx` (RAW-charset
container: the wire-vocabulary subset no external code needs zstd to
rebuild), `.plane` (canonical decoded-sample truth) and `.pam`
(Netpbm P7 source image).

## The three adapters

| Integration | File | Gate |
| --- | --- | --- |
| C ABI (raw decode tier) | `c-abi/abidec.c` | all six formats decode to byte-exact planes; single-byte header/payload corruption rejected with a controlled status, never a signal |
| libvips | `vips/vipsload_mnfx.c` | MNFX -> `vips_image_new_from_memory` -> in-pipeline pixel equality (`avg(|in-src|) == 0`) + a real thumbnail op; 8-bit scopes (rgb8/rgba8) |
| ImageMagick | `magick/magickload_mnfx.c` | MagickCore import/export byte-identical to the wire plane for rgba8/gray16/rgb16 (16-bit: explicit BE->LE swap both directions), thumbnail + PNG export cross-checked at the PIL byte level |

Run: `integrations/<name>/run.sh` (requires libvips / ImageMagick
installed for the two bridges; the c-abi gate needs neither).

The same integration logic is exercised out-of-tree by the W6-A..C
adapter repo (commits 42ae501 / b9d2cdc / df30348) against the
published crates.io crate; the in-repo copies are the durable,
self-contained forms.

## Known limitations (documented, not silent)

- The `mnfx` CLI encodes RGB8 via engine embed (MNF8/MNF9) and decodes
  RGB8 PNGs only. RGBA and 16-bit containers are produced by the
  serializer paths (non-embedded codecs: raw/zstd; embedded = RGB8 by
  design) and consumed via the C ABI bridges.
- Adapter metadata behavior: bridges set interpretation from the wire
  pixel format (sRGB / GRAY); MNFX's ICC/EXIF/XMP chunks are preserved
  by the format but NOT consumed by the adapters, and no color profile
  is attached to exports. Nothing silently reinterprets samples.
- Decode: adapters reject formats outside the supported shape set with
  a controlled status (and the magick importer refuses unknown/16-bit
  alpha formats) rather than converting.
- Encoding performance/memory (W6-D): balanced encode holds ~88-99 MB
  peak RSS (concurrent backend-candidate evaluation), vs PNG ~6-8 MB
  and cwebp ~20-41 MB. Decode-side RSS stays under ~4 MB at the
  measured sizes.
- Unmeasured W6-D cells are labeled in `docs/w6d-results.json`
  (Document class has no corpus fixture; Maximum profile measured on
  {photograph, gradient} only).
- Byte-level PNG roundtrips differ (encoder settings, no tIME chunk);
  the lossless contract is at the pixel plane.

## Wire contract

None of this changed the MNFX v1 wire grammar or the published v0.1.0
baseline (crates.io `manifold-frame 0.1.0`, tag `v0.1.0`). Adapter work
consumes the container; codec research stays in `manifold-codec`.
