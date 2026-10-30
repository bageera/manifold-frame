# manifold-frame

**MNFX** — a versioned lossless image container that wraps the
[Manifold](https://github.com/bageera/manifold) compression streams
(the `manifold-codec` crate = the MNF5–MNF9 encoder/decoder engines).
The wrapper adds image semantics at the format boundary — alpha,
16-bit samples, color profiles, metadata, orientation — without
modifying the engines: an MNF8/MNF9 payload embedded under MNFX decodes
byte-identically to its standalone decode. Design per ADR-009 and its
review in the manifold repo (`docs/adr-009-mnfx-review.md`).

Current status: **W3** — the W1 embed layer (`src/decode.rs`: pixel
payloads are whole standalone engine files dispatched by the codec
field with a magic-agreement check, decode routed to
`manifold::mnf8::decode_mnf8` / `manifold::mnf9::decode_mnf9`,
embed dims cross-checked), the W2 plane vocabulary (RGBA8/Gray8/GrayA8
join RGB8 as new enum values; the pixel-plane bound derives from the
format's stride 3/4/1/2; zstd plane decode hardened per the manifold
MNF7/MNF8 precedent — checksum mandatory, window_log_max 23, bound+1
take-limit bomb check; hidden RGB preserved byte-for-byte), and the W3
16-bit hi/lo plane shape: pixel formats RGB16 (5) / Gray16 (6) carry
their samples as one PIX16-HI + one PIX16-LO chunk pair (types 7/8,
each half the plane bound, its own raw/zstd codec), never a Pixel
chunk; decode joins big-endian interleaved (PNG wire order:
sample[n] = hi[n]<<8 | lo[n]) — the alphabet-relayout arm, with the
interleaved-zstd control as the mandatory rate comparison. Embed
codecs are RGB8-only. 32 unit tests green incl. the
embedded==standalone differential gate and the +34-B framing assert.

Corpus gate (`tests/corpus_embed.rs`): all 30 corpus files — embedded
decode == standalone decode == source pixels, framing exactly +34 B per
file, measured table in the test log (maximum profile; MNF9 picks solid
at 36 B, MNF8 the other 29).

W4 next: EXIF/XMP/ICC/orientation metadata (preserved-bytes chunks get
their first real consumers). W5: C ABI + CLI + fuzz + registry
consumer.