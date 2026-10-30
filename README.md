# manifold-frame

**MNFX** — a versioned lossless image container wrapping Manifold
compression streams: alpha, 16-bit samples, color profiles, metadata,
and orientation, added at the format boundary without modifying the
MNF8/MNF9 compression engines (ADR-009 + its review in the manifold
repo, `docs/adr-009-mnfx-review.md`).

Current status: **W0** — the grammar frozen (docs/mnfx-grammar.md)
and the bounded parser shipped (18-byte header, 16-byte chunks,
CRC32C-before-parse, bounds from the file length, unknown-critical
reject / unknown-optional skip, errors never panics). 11 unit tests
green incl. the RFC 3720 CRC vectors.

W1 (next): embed MNF8/MNF9 pixel layers via manifold-codec and prove
embedded decode byte-identical to standalone on the 30-file corpus.
W2: RGBA8/Gray8/GrayA8 with hidden-RGB alpha preservation. W3: 16-bit
high/low plane layers. W4: EXIF/XMP/ICC/orientation metadata. W5:
C ABI + CLI + fuzz + registry consumer.