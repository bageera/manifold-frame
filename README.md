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

W5 next: integrated encoder (PNG/TIFF→MNFX choosing valid methods),
C ABI, fuzz, registry consumer — the reviewer's central acceptance
requirement: real image in, complete image info (pixels + metadata +
color + orientation) back out, nothing unintended lost.