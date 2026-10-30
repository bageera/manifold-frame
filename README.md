# manifold-frame

**MNFX** — a versioned lossless image container that wraps the
[Manifold](https://github.com/bageera/manifold) compression streams
(the `manifold-codec` crate = the MNF5–MNF9 encoder/decoder engines).
The wrapper adds image semantics at the format boundary — alpha,
16-bit samples, color profiles, metadata, orientation — without
modifying the engines: an MNF8/MNF9 payload embedded under MNFX decodes
byte-identically to its standalone decode. Design per ADR-009 and its
review in the manifold repo (`docs/adr-009-mnfx-review.md`).

Current status: **W2** — the W1 embed layer (`src/decode.rs`: pixel
payloads are whole standalone engine files dispatched by the codec
field with a magic-agreement check, decode routed to
`manifold::mnf8::decode_mnf8` / `manifold::mnf9::decode_mnf9`,
embed dims cross-checked) plus the W2 plane vocabulary: pixel formats
RGBA8/Gray8/GrayA8 join RGB8 (new enum values, v1-compatible), the
pixel-plane bound derives from the format's stride (3/4/1/2 B per
pixel), and zstd plane decode is hardened per the manifold MNF7/MNF8
precedent — checksum mandatory, window_log_max 23, bound+1 take-limit
bomb check. Hidden RGB is preserved byte-for-byte (grammar rule 8):
all four RGBA channels come back exactly as stored. Embed codecs are
RGB8-only; plane formats ride raw/zstd. 25 unit tests green incl. the
embedded==standalone differential gate and the +34-B framing assert.

Corpus gate (`tests/corpus_embed.rs`): all 30 corpus files — embedded
decode == standalone decode == source pixels, framing exactly +34 B per
file, measured table in the test log (maximum profile; MNF9 picks solid
at 36 B, MNF8 the other 29).

W3 next: 16-bit high/low byte-plane layers (exactness gate against the
interleaved control). W4: EXIF/XMP/ICC/orientation metadata. W5: C ABI +
CLI + fuzz + registry consumer.