# MNFX v1 grammar — W0-frozen (2026-10-29)

The container grammar per the ADR-009 review's corrections
(../manifold/docs/adr-009-mnfx-review.md). Implemented in
`src/parser.rs` (crate `manifold-frame`); this document = the
normative reference.

## Container header — 18 bytes

| offset | size | field      | notes                                   |
| ------ | ---- | ---------- | --------------------------------------- |
| 0      | 4    | magic      | `MNFX`                                  |
| 4      | 2    | version    | u16 LE; v1 = 1                          |
| 6      | 1    | pix fmt    | v1 = 1 (RGB8)                           |
| 7      | 1    | orient     | 1..=8; default 1 (samples unrotated)    |
| 8      | 2    | chunk n    | u16 LE                                  |
| 10     | 4    | width      | u32 LE                                  |
| 14     | 4    | height     | u32 LE                                  |

## Chunk header — 16 bytes

| offset | size | field      | notes                                            |
| ------ | ---- | ---------- | ------------------------------------------------ |
| 0      | 2    | type       | u16 LE; values below; 0x8000 = critical scope    |
| 2      | 1    | codec      | 0 raw, 4 MNF8 embed, 5 MNF9 embed, 6 zstd        |
| 3      | 1    | flags      | reserved, must be 0                              |
| 4      | 4    | raw_len    | decoded length (the decode bound)                |
| 8      | 4    | stored_len | wire payload length                              |
| 12     | 4    | crc32c     | over the STORED bytes, validated BEFORE parse    |

Chunk types: 1 PIXEL, 2 EXIF, 3 XMP, 4 ICC, 5 CICP, 6 ORNT;
unknown >= 0x8000 reject, unknown < 0x8000 skip (lossless rewrite).

## Normative rules

1. All bounds derive from the file length (stored_len vs remaining;
   chunk_count vs file capacity).
2. CRC32C (Castagnoli) validated before any payload interpretation.
3. Exactly one PIXEL chunk. Codec 5 (MNF9 embed) = whole-frame: one
   such layer, no other pixel layers; embed chunks carry raw_len = 0.
4. Raw codec: raw_len == stored_len == payload length (checked).
5. Zstd codec: the decompression bound enforced at decode (W1).
6. Malformed input = `Error`, never panic.
7. Plain RGB8 without wrapper semantics stays standalone
   MNF8/MNF9 (the +34-B framing rule: never double a small file).
8. Lossless contracts: metadata payloads byte-for-byte; alpha
   hidden-RGB preserved at W2; 16-bit hi/lo planes exact at W3.

## Framing accounting (v1)

Minimum = 18 + 16 = 34 B (one pixel chunk, one MNFX). The W0
implementation's `serialize` + `parse` roundtrip on these numbers is
asserted in the unit tests (11/11, incl. the CRC32C RFC 3720
vectors).