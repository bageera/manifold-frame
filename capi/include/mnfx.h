// mnfx.h — the C ABI for the MNFX container (manifold-frame 0.1.x).
//
// Contract (mirrors the manifold-codec-capi contract, per sibling
// precedent): never panics on caller data; every fallible call returns a
// MNFX_STATUS_* code; buffers are caller-owned with caller-supplied
// lengths; the size-query idiom (call with out=NULL first) reports the
// required size. No global state, no callbacks, no allocations on the
// caller's behalf.
#ifndef MNFX_H
#define MNFX_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* status codes (stable: values fixed by this header) */
#define MNFX_STATUS_OK              0
#define MNFX_STATUS_NULL_ARG       -1
#define MNFX_STATUS_BUF_TOO_SMALL  -2
#define MNFX_STATUS_FORMAT_INVALID -3
#define MNFX_STATUS_INTERNAL       -7

/* pixel formats (the wire values from docs/mnfx-grammar.md) */
#define MNFX_PIXFMT_RGB8   1
#define MNFX_PIXFMT_RGBA8  2
#define MNFX_PIXFMT_GRAY8  3
#define MNFX_PIXFMT_GRAYA8 4
#define MNFX_PIXFMT_RGB16  5
#define MNFX_PIXFMT_GRAY16 6

/*
 * Decode any MNFX container to its full interleaved sample plane.
 *
 * buf/len: the container bytes. plane: caller buffer sized *plane_len.
 * Query pass: call with plane = NULL; *plane_len receives the required
 * size, width/height/pixel_format/orientation always receive the header
 * values (on OK and on BUF_TOO_SMALL). pixel_format/orientation
 * pointers may be NULL to skip those outputs. Orientation: the
 * container's rule-12 resolved value (EXIF tag > header field) is NOT
 * provided here; the raw header field is (rule 12 consumers read the
 * EXIF chunk themselves). The plane is never rotated.
 *
 * Returns MNFX_STATUS_OK, MNFX_STATUS_BUF_TOO_SMALL (a normal outcome:
 * fill the buffer and call again), or MNFX_STATUS_FORMAT_INVALID.
 */
int32_t mnfx_decode(const uint8_t *buf, size_t len,
                    uint8_t *plane, size_t *plane_len,
                    uint32_t *width, uint32_t *height,
                    uint8_t *pixel_format, uint8_t *orientation);

/*
 * Rewrite one chunk's payload inside an existing MNFX container,
 * preserving every other byte (the frame lib's serialize/parse
 * lossless-rewrite contract; CRC32C over the stored bytes is
 * recomputed here — no caller CRC input). chunk_index is 0-based in
 * wire order. The byte length of the result is len + delta where
 * delta = new_len - old_len (query pass: new=NULL reports the exact
 * delta). Stored payload length is bounded by 0xFFFFFFFF.
 *
 * Returns OK / BUF_TOO_SMALL (query) / FORMAT_INVALID (bad container,
 * index at/after the chunk count, or delta would exceed u32 limits).
 */
int32_t mnfx_rewrite_chunk(const uint8_t *buf, size_t len,
                           uint32_t chunk_index,
                           const uint8_t *new_payload, size_t new_len,
                           uint8_t *out, size_t *out_len);

#ifdef __cplusplus
}
#endif
#endif /* MNFX_H */