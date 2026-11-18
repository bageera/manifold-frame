# W8-F3 — version and package reconciliation

At the release-candidate revision (F1 complete, F2 suite green).

## The three compatibility boundaries

| Boundary | Disposition |
| --- | --- |
| **MNFX wire specification** | **v1, unchanged.** No grammar, constant, or framing change anywhere in W8-A..F; the D/E phases were performance/safety/API-only. Corpus + framing-34-byte evidence stayed bit-identical across the phases. |
| **manifold-frame Rust package** | `0.1.1 → 0.2.0`. Registry state verified live before the bump: crates.io max = 0.1.1 (published 0.1.0, 0.1.1; 2026-11-05). 0.2.0 is the natural semver line for the additive volume since 0.1.0 (borrowed parsing + limits + producer + the F1 CLI pin): every 0.1.x calling contract (owning `parse`, `decode_image`, `Chunk`, `serialize_checked`, the `capi` feature) is intact — the additive surface only ADDS (`parse_borrowed*`, `DecodeLimits` readers, `encode::zstd_plane_*`). |
| **manifold-frame-capi** | `publish = false` (verified in capi/Cargo.toml) — the binary archives are its distribution. Package version `0.1.0 → 0.2.0` in lockstep. The C surface is UNCHANGED: signatures, status values (0/1/2), caller-owned memory, and the header contract (`capi/include/mnfx.h`) are byte-for-byte the W6 contract. |

## Executed verification (recorded evidence)

1. **cargo package** — `cargo package -p manifold-frame --features cli
   --no-verify --allow-dirty` succeeds; the packaged manifest
   (`manifold-frame-0.2.0.crate`) carries exactly
   `[dependencies.manifold-codec] version = "1.14"` and
   `[dependencies.zstd] version = "0.13"` (the path is a local
   convenience: path+version means registry consumers resolve the
   published engine line), plus the `cli` feature and the lib/bin in
   the 62-file crate.
2. **Exported symbols vs the C header** — `nm -gU` on the built dylib
   lists exactly `_mnfx_decode` and `_mnfx_rewrite_chunk`;
   `capi/include/mnfx.h` declares exactly those two entry points.
   (The 0.x ABI keeps the two-function surface; no status values
   changed. The E2 early-out is a resource behavior inside the
   documented status contract — pinned in capi/README.md.)
3. **Registry consumer smoke** — `scripts/consume_release.sh` (default
   0.1.1): a fresh consumer crate pulls manifold-frame 0.1.1 FROM THE
   REGISTRY, encodes and decodes a 16×16 PNG, and passes exact
   reconstruction (`CONSUMER RUN OK`). The published line remains
   consumable — publication of 0.2.0 is F5, not this gate.
4. **Lockfile** — `Cargo.lock` carries `manifold-frame 0.2.0` +
   `manifold-frame-capi 0.2.0`;
   `cargo build --release --locked` is clean across both workspace
   members.

## C ABI documentation currency

`capi/include/mnfx.h` matches the implementation exports; the
consumer-facing behavioral pin (size query ≠ reconstruction
certification; E2 resource guard) is documented in capi/README.md
(shipped as `share/mnfx/capi-README.md` since the B3 packaging).

## Documentation + provenance currency

README verification board carries the W8 rows; CHANGELOG is stamped
[0.2.0] as the release-candidate content; `docs/w8b1-provenance.json`
pins the engine vendoring (1.16.0, sha-verified both ways) still in
force for this revision.
