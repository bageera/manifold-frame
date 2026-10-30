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
| 6      | 1    | pix fmt    | 1 RGB8, 2 RGBA8, 3 Gray8, 4 GrayA8,      |
|        |      |            | 5 RGB16, 6 Gray16                        |
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

Chunk types: 1 PIXEL, 2 EXIF, 3 XMP, 4 ICC, 5 CICP, 6 ORNT, 7 PIX16-HI,
8 PIX16-LO (W3); unknown >= 0x8000 reject, unknown < 0x8000 skip
(lossless rewrite).

## Normative rules

1. All bounds derive from the file length (stored_len vs remaining;
   chunk_count vs file capacity).
2. CRC32C (Castagnoli) validated before any payload interpretation.
3. Pixel accounting is format-conditional: 8-bit formats (RGB8, RGBA8,
   Gray8, GrayA8) carry the samples in exactly ONE PIXEL chunk; the
   16-bit formats (RGB16, Gray16, W3) carry them as exactly one
   PIX16-HI + one PIX16-LO chunk pair and NEVER a PIXEL chunk (no
   double-booking). Codec 5 (MNF9 embed) = whole-frame, RGB8-only;
   embed chunks carry raw_len = 0.
4. Raw codec: raw_len == stored_len == payload length (checked).
5. Zstd codec: the decompression bound enforced at decode; checksum
   mandatory, window_log_max 23, size bound+1 take-limit bomb check
   (the manifold MNF7/MNF8 hardening, landed W2).
6. Malformed input = `Error`, never panic.
7. Plain RGB8 without wrapper semantics stays standalone
   MNF8/MNF9 (the +34-B framing rule: never double a small file).
8. Lossless contracts: metadata payloads byte-for-byte; alpha
   hidden-RGB preserved byte-for-byte (the RGBA plane's color
   channels are never premultiplied, stripped or re-derived); 16-bit
   hi/lo planes exact at W3.
9. Pixel plane bound = width × height × stride(pixel_format):
   stride 3 RGB8, 4 RGBA8, 1 Gray8, 2 GrayA8, 6 RGB16, 2 Gray16 (W3).
   Raw planes must equal the bound exactly; zstd planes decode TO the
   bound exactly. Embed codecs (4/5) require pixel_format 1 (RGB8) —
   the engines' native shape.
10. 16-bit container shape (W3): pixel_format 5 or 6 requires exactly
   one PIX16-HI and one PIX16-LO; plane chunks must NOT carry embed
   codecs; each plane's payload = half the plane bound, per its own
   codec. Decode joins them big-endian interleaved (PNG wire order):
   sample[n] = hi[n] << 8 | lo[n]. The hi/lo split is the
   alphabet-relayout arm; an interleaved-zstd control (the same bytes
   as one interleaved plane) is the mandatory comparison in the
   promotion gate when either arm is measured for rate.

## Framing accounting (v1)

Minimum = 18 + 16 = 34 B (one pixel chunk, one MNFX). The W0
implementation's `serialize` + `parse` roundtrip on these numbers is
asserted in the unit tests (incl. the CRC32C RFC 3720 vectors). W1
added the embedded==standalone differential gate (22 tests) and the
30-file corpus gate (framing +34 B exact per file). W2 added the
plane-format vocabulary (2/3/4), the stride-derived plane bound, and
the hardened zstd plane decode. W3 added the 16-bit hi/lo plane shape
(formats 5/6, chunk types 7/8, big-endian interleave) — 32 unit tests.