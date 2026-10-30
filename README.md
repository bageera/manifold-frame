# manifold-frame

**MNFX** — a versioned lossless image container that wraps the
[Manifold](https://github.com/bageera/manifold) compression streams
(the `manifold-codec` crate = the MNF5–MNF9 encoder/decoder engines).
The wrapper adds image semantics at the format boundary — alpha,
16-bit samples, color profiles, metadata, orientation — without
modifying the engines: an MNF8/MNF9 payload embedded under MNFX decodes
byte-identically to its standalone decode. Design per ADR-009 and its
review in the manifold repo (`docs/adr-009-mnfx-review.md`).

Current status: **W1** — the MNF8/MNF9 embed layer shipped
(`src/decode.rs`): pixel payloads are whole standalone engine files,
dispatched by the codec field with a magic-agreement check, decode
routed to `manifold::mnf8::decode_mnf8` / `manifold::mnf9::decode_mnf9`
(manifold-codec 1.13.0, path dep), embed dims cross-checked against the
container header. Raw pixel layers checked against the header-derived
plane bound; zstd pixel decode reserves for W2. 22 unit tests green
incl. the embedded==standalone differential gate on real engine files
(both MNF8 profiles + MNF9) and the +34-B framing assert.

Corpus gate (`tests/corpus_embed.rs`): all 30 corpus files — embedded
decode == standalone decode == source pixels, framing exactly +34 B per
file, measured table in the test log (maximum profile; MNF9 picks solid
at 36 B, MNF8 the other 29).

W2 next: RGBA8/Gray8/GrayA8 with hidden-RGB alpha preservation (zstd
pixel-plane decode lands here with its producer). W3: 16-bit high/low
plane layers. W4: EXIF/XMP/ICC/orientation metadata. W5: C ABI + CLI +
fuzz + registry consumer.