# Pre-W8-F CLI audit — RESOLVED (W8-F1)

Carried through W8-C so they could not be assumed fixed; resolved under
W8-F1 with executed verification (`tests/cli_audit.rs`, spawned-binary
e2e, 3/3 green; clippy clean with the `cli` feature on).

| # | Item | Resolution | Evidence |
| --- | --- | --- | --- |
| 1 | Bare `--mnf8` silently ignored | **PINNED**: all three spellings (`--mnf8`, `--mnf8=balanced`, `--mnf8=maximum`) now FORCE the embedded payload to the MNF8 engine at that profile — the MNF9 smaller-stream arm stops competing, so the selection flag and the embedded magic can never disagree (src/main.rs `force_mnf8`; usage text documents the pin and the unsigned auto-pick) | e2e: pinned encode → parsed codec `MnF8`; unsigned encode → report label == parsed codec |
| 2 | Help/error exit split untested | **PINNED**: `--help`/`-h` print the usage to STDOUT with exit 0; argument errors print usage to STDERR with exit 2 (`die()`) | e2e: both flags (stdout, rc 0, stderr empty), `bogus-command` + no args (stderr, rc 2) |
| 3 | Pixel-count arithmetic at the u32 boundary | **FIXED**: the CLI report multiplies in u64 (a u32 multiply would wrap/panic near the grammar ceiling); the format machinery's u64 bounds remain verified by `tests/w8e_limits.rs` and the W8-A3 real-i686 CI regression | e2e: `8 px` on a 4×2 fixture; hostile stored lengths still reject in debug + release on i686 (CI) |
| 4 | Pixels-only CLI conversion documented-as-absent | **CONTRACT (not a bug)**: re-affirmed verbatim in the usage text ("PIXELS only… source ICC/EXIF/XMP are not carried") and the W8-C producer comment; full metadata conversion remains a future feature by design | e2e: usage contains "PIXELS only" |
| 5 | C ABI early-out semantics (F1 addition) | **DOC-PINNED**: capi/README.md (ships as `share/mnfx/capi-README.md`) — the BUF_TOO_SMALL size query establishes the required output length + header fields only, never payload reconstruction; corruption returns FORMAT_INVALID on the subsequent full call; the query path never materializes a plane or runs the engine | W8-E2 capi unit test (`too_small_buffer_returns_sizes_before_decoding`) + the README section |

Note for runners: the mnfx binary is `required-features = ["cli"]`
(the CLI feature activates the engine's image-io surface), so the
spawned-binary tests are themselves gated on the feature and are built
by `cargo test --features cli` (the local gate above and the
distribution pipeline, which always builds with the CLI).
