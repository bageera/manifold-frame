# manifold-frame

**MNFX** — a versioned lossless image container that wraps the
[Manifold](https://github.com/bageera/manifold) compression streams
(the `manifold-codec` crate = the MNF5–MNF9 encoder/decoder engines).
The wrapper adds image semantics at the format boundary — alpha,
16-bit samples, color profiles, metadata, orientation — without
modifying the engines: an MNF8/MNF9 payload embedded under MNFX decodes
byte-identically to its standalone decode. Design per ADR-009 and its
review in the manifold repo (`docs/adr-009-mnfx-review.md`).

Current status: **W4** — W1–W3 shipped (the embed layer with codec-
field dispatch + magic-agreement; the RGBA8/Gray8/GrayA8 plane
vocabulary with stride-derived bounds and the hardened zstd decode;
the RGB16/Gray16 hi/lo plane pair with big-endian interleave), plus
the W4 metadata surface:

- W4-A preservation: EXIF/XMP/ICC/CICP/ORNT payloads roundtrip
  byte-exact with chunk ORDER preserved; duplicates of known metadata
  chunks reject (grammar rule 11); unknown optional chunks skip.
- W4-B semantics: color precedence ICC > CICP; orientation = EXIF
  tag 0x0112 > header field > default 1 (rule 12); samples are never
  rotated at decode. Boundary validation only (EXIF TIFF shell,
  CICP = 4 B, ORNT = 1 B in 1..=8) — the container preserves bytes
  and leaves full EXIF/ICC/XMP structure to consumers.
- W4-C differential (`tests/meta_differential.rs`): a real MNF8
  engine file + 4 metadata chunks decodes pixel-exact vs standalone vs
  source, metadata byte-exact, rewrite lossless.
- W3-R1 rate control (`tests/rate_control_16bit.rs`): the hi/lo split
  MEASURED vs the interleaved-zstd control at complete-wire size
  (photo fixture: 47,990 B vs 27,021 B — interleaved wins ~44%; the
  split stays vocabulary, per grammar rule 10's note).
- serialize hardening: u32 length fields reject overflow instead of
  silently wrapping. 37 unit tests green.

Corpus gate (`tests/corpus_embed.rs`): all 30 corpus files — embedded
decode == standalone decode == source pixels, framing exactly +34 B per
file, measured table in the test log.

Current status: **W5** — the integrated encoder + CLI shipped:

- `mnfx` CLI (`src/main.rs`, feature `cli` = the engines' image-io):
  `encode <png> -o <mnfx>` runs the automatic engine pick (MNF8 at the
  chosen profile competes with the MNF9 whole-frame arm at complete
  file size — the manifold CLI's flow); `decode <mnfx> -o <png>`
  restores the image; `verify <image> <mnfx>` prints PASS/FAIL
  (exit 0/1). Real-pipeline smoke: document 200×280 = 586 B (+34
  framing), PNG-plane byte-identical after decode.
- W5 acceptance test (`tests/w5_acceptance.rs`): real PNG → MNFX
  (engine pick + ICC chunk) → decode returns pixel-exact bytes + the
  preserved ICC; corrupted containers (byte flips at header/payload/
  CRC positions, truncations) reject or decode — never panic.
- Fuzz (`scripts/fuzz.py`, the manifold shape): encode every corpus
  image, mutate (flips/truncations/appends), decode must not panic —
  measured 340 trials, 0 panics, 0 structurally-corrupt accepts (the
  accepted set = pure appends only; all flips/truncations rejected by
  CRC/parser).
- The registry-consumer + C ABI gates stay the publication-decision
  items (C ABI opt-in per the freeze; `publish = false` until a
  consumer run exists).