/* The W5-D C-ABI smoke: a real cc compile + link against the
 * manifold_frame_capi staticlib, exercising every status branch:
 * query pass, copy pass, exactness, lossless rewrite, malformed
 * rejected as FORMAT_INVALID — never a panic (a Rust panic across
 * the FFI boundary aborts the process, so any crash fails the gate).
 * Companion: capi/tests/c-smoke/crates/mnfx-ffi-smoke (cargo test). */
#include "mnfx.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures = 0;

#define EXPECT(cond, what)                                                    \
    do {                                                                      \
        if (!(cond)) {                                                        \
            fprintf(stderr, "FAIL: %s\n", what);                              \
            failures++;                                                       \
        }                                                                     \
    } while (0)

/* CRC32C (Castagnoli), table-driven, over the stored payload bytes —
 * the smoke mirrors the format contract without a Rust dependency. */
static uint32_t crc32c(const uint8_t *p, size_t n) {
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < n; i++) {
        crc ^= p[i];
        for (int k = 0; k < 8; k++)
            crc = (crc >> 1) ^ (0x82F63B78u & (0u - (crc & 1)));
    }
    return crc ^ 0xFFFFFFFFu;
}

static void put_le32(uint8_t *dst, uint32_t v) {
    dst[0] = (uint8_t)v;
    dst[1] = (uint8_t)(v >> 8);
    dst[2] = (uint8_t)(v >> 16);
    dst[3] = (uint8_t)(v >> 24);
}

/* Build a valid 8x8 RGB8 raw-PIXEL MNFX container. */
static uint8_t *build_container(size_t *total_out) {
    size_t plane_len = 8 * 8 * 3;
    size_t total = 18 + 16 + plane_len;
    uint8_t *cont = calloc(total, 1);
    memcpy(cont, "MNFX", 4);
    cont[4] = 1;               /* version LE = 1 */
    cont[6] = MNFX_PIXFMT_RGB8;
    cont[7] = 1;               /* orientation 1 */
    cont[8] = 1;               /* chunk_count LE = 1 */
    cont[10] = 8;              /* width LE */
    cont[14] = 8;              /* height LE */
    cont[18] = 1;              /* chunk type u16 LE = PIXEL */
    cont[20] = 0;              /* codec 0 = raw */
    /* CRC + payload filled by the caller */
    *total_out = total;
    return cont;
}

/* Build as a function (not main): the cc crate links this INTO the
 * Rust test binary, which has its own main. The Rust test drives
 * mnfx_smoke_main() via FFI. */
int mnfx_smoke_main(void) {
    size_t plane_len = 8 * 8 * 3;
    size_t total = 0;
    uint8_t *cont = build_container(&total);
    put_le32(cont + 22, (uint32_t)plane_len); /* raw_len */
    put_le32(cont + 26, (uint32_t)plane_len); /* stored_len */
    uint8_t *payload = cont + 34;             /* 18 + 16 */
    for (size_t i = 0; i < plane_len; i++)
        payload[i] = (uint8_t)(i * 7 % 256);
    put_le32(cont + 30, crc32c(payload, plane_len));

    /* ---- decode: query pass ---- */
    size_t need = 0;
    uint32_t w = 0, h = 0;
    uint8_t pf = 0, orient = 0;
    int32_t st = mnfx_decode(cont, total, NULL, &need, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_BUF_TOO_SMALL, "decode query -> BUF_TOO_SMALL");
    EXPECT(need == plane_len, "query reports plane bound");
    EXPECT(w == 8 && h == 8, "query reports dims");
    EXPECT(pf == MNFX_PIXFMT_RGB8, "query reports pixfmt");
    EXPECT(orient == 1, "query reports orientation");

    /* ---- decode: copy pass, byte-exact ---- */
    uint8_t *plane = malloc(need);
    size_t got = need;
    st = mnfx_decode(cont, total, plane, &got, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_OK, "decode copy -> OK");
    for (size_t i = 0; i < plane_len; i++) {
        EXPECT(plane[i] == (uint8_t)(i * 7 % 256), "plane byte-exact");
        if (plane[i] != (uint8_t)(i * 7 % 256)) break;
    }
    free(plane);

    /* ---- malformed: flipped magic / truncation / empty ---- */
    cont[0] = 'X';
    st = mnfx_decode(cont, total, NULL, &need, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_FORMAT_INVALID, "bad magic -> FORMAT_INVALID");
    cont[0] = 'M';
    st = mnfx_decode(cont, 10, NULL, &need, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_FORMAT_INVALID, "truncated -> FORMAT_INVALID");
    st = mnfx_decode(cont, 0, NULL, &need, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_FORMAT_INVALID, "empty -> FORMAT_INVALID");
    /* a flipped payload byte: CRC catches it BEFORE parse */
    payload[3] ^= 0xFF;
    st = mnfx_decode(cont, total, NULL, &need, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_FORMAT_INVALID, "payload flip (CRC) -> FORMAT_INVALID");
    payload[3] ^= 0xFF;
    /* bogus stored_len past the file end */
    {
        uint8_t *lying = malloc(total);
        memcpy(lying, cont, total);
        put_le32(lying + 26, 0xFFFFFFu);
        st = mnfx_decode(lying, total, NULL, &need, &w, &h, &pf, &orient);
        EXPECT(st == MNFX_STATUS_FORMAT_INVALID, "lying stored_len -> FORMAT_INVALID");
        free(lying);
    }

    /* ---- null args ---- */
    st = mnfx_decode(NULL, 10, NULL, &need, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_NULL_ARG, "null buf -> NULL_ARG");
    st = mnfx_decode(cont, total, NULL, NULL, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_NULL_ARG, "null plane_len -> NULL_ARG");

    /* ---- rewrite: SAME-length payload replace = lossless rewrite ---- */
    uint8_t *newp = malloc(plane_len);
    for (size_t i = 0; i < plane_len; i++)
        newp[i] = (uint8_t)(255 - i % 251);
    size_t out_need = 0;
    st = mnfx_rewrite_chunk(cont, total, 0, newp, plane_len, NULL, &out_need);
    EXPECT(st == MNFX_STATUS_BUF_TOO_SMALL, "rewrite query -> BUF_TOO_SMALL");
    EXPECT(out_need == total, "same-length rewrite leaves size unchanged");
    uint8_t *rewritten = malloc(out_need);
    size_t got2 = out_need;
    st = mnfx_rewrite_chunk(cont, total, 0, newp, plane_len, rewritten, &got2);
    EXPECT(st == MNFX_STATUS_OK, "rewrite copy -> OK");
    /* every preserved byte: container header + chunk header through
     * stored_len (30 B). The CRC field (30..34) necessarily differs —
     * it now covers the NEW payload (proved by the decode below). */
    EXPECT(memcmp(rewritten, cont, 30) == 0, "headers through stored_len preserved");
    {
        uint8_t want_crc[4];
        put_le32(want_crc, crc32c(newp, plane_len));
        EXPECT(memcmp(rewritten + 30, want_crc, 4) == 0, "CRC recomputed over the new payload");
    }
    EXPECT(memcmp(rewritten + 34, newp, plane_len) == 0, "payload = new bytes");
    /* the rewritten container still decodes, to the NEW plane */
    need = 0;
    st = mnfx_decode(rewritten, got2, NULL, &need, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_BUF_TOO_SMALL, "rewritten decodes (CRC fixed up)");
    EXPECT(need == plane_len, "rewritten bound unchanged");
    plane = malloc(need);
    got = need;
    st = mnfx_decode(rewritten, got2, plane, &got, &w, &h, &pf, &orient);
    EXPECT(st == MNFX_STATUS_OK, "rewritten decode copy -> OK");
    EXPECT(memcmp(plane, newp, plane_len) == 0, "decoded plane = new payload");
    free(plane);

    /* ---- rewrite: different-length payload = size shift, CRC kept valid;
     * the resulting container is shape-inconsistent for THIS header
     * (raw plane != bound), so decode reports FORMAT_INVALID — the
     * memory-safety property, not a rewrite failure. ---- */
    {
        uint8_t small[5] = {9, 9, 9, 9, 9};
        size_t s_need = 0;
        st = mnfx_rewrite_chunk(cont, total, 0, small, 5, NULL, &s_need);
        EXPECT(st == MNFX_STATUS_BUF_TOO_SMALL, "resize rewrite query -> BUF_TOO_SMALL");
        EXPECT(s_need == total - plane_len + 5, "resize query reports delta");
        uint8_t *rs = malloc(s_need);
        size_t rs_len = s_need;
        st = mnfx_rewrite_chunk(cont, total, 0, small, 5, rs, &rs_len);
        EXPECT(st == MNFX_STATUS_OK, "resize rewrite copy -> OK");
        EXPECT(rs_len == total - plane_len + 5, "resize copy size");
        need = 0;
        st = mnfx_decode(rs, rs_len, NULL, &need, &w, &h, &pf, &orient);
        EXPECT(st == MNFX_STATUS_FORMAT_INVALID, "shape-inconsistent resize decodes as FORMAT_INVALID");
        free(rs);
    }

    /* ---- rewrite: bad args ---- */
    st = mnfx_rewrite_chunk(NULL, total, 0, newp, plane_len, NULL, &out_need);
    EXPECT(st == MNFX_STATUS_NULL_ARG, "rewrite null container -> NULL_ARG");
    /* (NULL, 0) is the documented EMPTY replacement */
    st = mnfx_rewrite_chunk(cont, total, 0, NULL, 0, NULL, &out_need);
    EXPECT(st == MNFX_STATUS_BUF_TOO_SMALL, "empty-replacement query -> BUF_TOO_SMALL");
    EXPECT(out_need == total - plane_len, "empty replacement shrinks by the payload");
    cont[25] = 42; /* corrupt stored_len under a hand CRC — the lib rejects */
    st = mnfx_rewrite_chunk(cont, total, 0, newp, plane_len, NULL, &out_need);
    cont[25] = 0;
    EXPECT(st == MNFX_STATUS_FORMAT_INVALID, "rewrite on lying container");

    free(newp);
    free(rewritten);
    free(cont);
    if (failures == 0) {
        printf("c-smoke: all checks passed\n");
        return 0;
    }
    printf("c-smoke: %d failure(s)\n", failures);
    return 1;
}