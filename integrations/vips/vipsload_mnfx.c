/*
 * vipsload_mnfx.c (W6-B): load an MNFX container into a live libvips
 * pipeline through the SHIPPED C ABI (libmanifold_frame_capi.dylib =
 * the artifact an adopter links; mnfx.h = the header they include),
 * then vips_image_new_from_memory wraps the decoded plane.
 *
 * Gates (the "unknown format" point of the exercise — no MNFX
 * internals appear here, only mnfx.h's two symbols):
 *   1. pipeline pixel equality vs the source PNG
 *      (vips_avg(vips_absdiff) == 0),
 *   2. a real vips operation over the MNFX pixels (thumbnail),
 *   3. write the decoded plane out through vips.
 *
 * Build: see run_w6b.sh (pkg-config glib-2.0 gobject-2.0 + vips).
 */
#include "mnfx.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <vips/vips.h>

static int failures = 0;

#define EXPECT(cond, what)                                                     \
    do {                                                                       \
        if (!(cond)) {                                                         \
            fprintf(stderr, "FAIL: %s\n", what);                               \
            failures++;                                                        \
        }                                                                      \
    } while (0)

/* Read a whole file; NULL on failure. */
static uint8_t *slurp(const char *path, size_t *len_out) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *buf = malloc((size_t)n);
    if (!buf || fread(buf, 1, (size_t)n, f) != (size_t)n) {
        fclose(f);
        free(buf);
        return NULL;
    }
    fclose(f);
    *len_out = (size_t)n;
    return buf;
}

/* Plane-lifetime registry: vips_image_new_from_memory BORROWS the
 * buffer, so each loaded plane is parked here and freed after the
 * pipeline is torn down.
 * ponytail: fixed 64-slot array for the gate; a real loader plugs the
 * plane into a VipsOperation or copies via vips_image_new_memory. */
static uint8_t *g_planes[64];
static int g_nplanes = 0;

/*
 * vips_mnfx_load_buffer: the W6-B primitive an application ships —
 * MNFX bytes in, VipsImage out. NULL on any failure (status printed).
 */
static VipsImage *vips_mnfx_load_buffer(const uint8_t *buf, size_t len) {
    size_t need = 0;
    uint32_t w = 0, h = 0;
    uint8_t pformat = 0, orient = 0;
    int32_t st = mnfx_decode(buf, len, NULL, &need, &w, &h, &pformat, &orient);
    if (st != MNFX_STATUS_BUF_TOO_SMALL) {
        fprintf(stderr, "vips_mnfx_load: mnfx_decode status %d\n", st);
        return NULL;
    }
    uint8_t *plane = malloc(need);
    if (!plane) return NULL;
    size_t got = need;
    st = mnfx_decode(buf, len, plane, &got, &w, &h, &pformat, &orient);
    if (st != MNFX_STATUS_OK) {
        free(plane);
        fprintf(stderr, "vips_mnfx_load: copy pass status %d\n", st);
        return NULL;
    }
    /* Wire format -> vips band geometry (the W6-B 8-bit scope). */
    int bands;
    int interp;
    switch (pformat) {
    case MNFX_PIXFMT_RGB8:
        bands = 3;
        interp = VIPS_INTERPRETATION_sRGB;
        break;
    case MNFX_PIXFMT_RGBA8:
        bands = 4;
        interp = VIPS_INTERPRETATION_sRGB;
        break;
    case MNFX_PIXFMT_GRAY8:
        bands = 1;
        interp = VIPS_INTERPRETATION_B_W;
        break;
    case MNFX_PIXFMT_GRAYA8:
        bands = 2;
        interp = VIPS_INTERPRETATION_B_W;
        break;
    default:
        free(plane);
        fprintf(stderr, "vips_mnfx_load: pixfmt %d out of the W6-B 8-bit gate\n", pformat);
        return NULL;
    }
    VipsImage *mem = vips_image_new_from_memory(
        plane, need, (int)w, (int)h, bands, VIPS_FORMAT_UCHAR);
    if (!mem) return NULL;
    /* Correct interpretation for downstream ops (thumbnail, resize). */
    VipsImage *proper = NULL;
    if (vips_copy(mem, &proper, "interpretation", interp, NULL)) {
        g_object_unref(mem);
        return NULL;
    }
    g_object_unref(mem);
    g_planes[g_nplanes++] = plane;
    vips_image_set_int(proper, "mnfx-orientation", orient);
    return proper;
}

int main(int argc, char **argv) {
    if (VIPS_INIT(argv[0])) vips_error_exit(NULL);
    if (argc < 3) {
        fprintf(stderr, "usage: %s <in.mnfx> <src.png>\n", argv[0]);
        return 2;
    }
    const char *mnfx_path = argv[1];
    const char *src_png = argv[2];

    size_t mnfx_len = 0;
    uint8_t *mnfx_buf = slurp(mnfx_path, &mnfx_len);
    if (!mnfx_buf) {
        fprintf(stderr, "cannot read %s\n", mnfx_path);
        return 2;
    }
    VipsImage *in = vips_mnfx_load_buffer(mnfx_buf, mnfx_len);
    free(mnfx_buf);
    if (!in) vips_error_exit(NULL);

    /* Gate 1: pipeline pixel equality vs the source PNG. */
    VipsImage *src = vips_image_new_from_file(src_png, "access", VIPS_ACCESS_RANDOM, NULL);
    if (!src) vips_error_exit(NULL);
    /* |in - src| via subtract + abs (the available arith pair). */
    VipsImage *sub = NULL, *ab = NULL;
    if (vips_subtract(in, src, &sub, NULL)) vips_error_exit(NULL);
    if (vips_abs(sub, &ab, NULL)) vips_error_exit(NULL);
    double avg = -1.0;
    if (vips_avg(ab, &avg, NULL)) vips_error_exit(NULL);
    EXPECT(avg == 0.0, "pipeline pixels == source (avg(|in-src|) == 0)");
    g_object_unref(ab);
    g_object_unref(sub);

    /* Gate 2: a real vips operation over the MNFX pixels. */
    VipsImage *thumb = NULL;
    if (vips_thumbnail_image(in, &thumb, 32, NULL)) vips_error_exit(NULL);
    if (vips_image_write_to_file(thumb, "/tmp/w6b_thumb.png", NULL)) vips_error_exit(NULL);
    printf("gate2: thumbnail %dx%d bands=%d\n",
           vips_image_get_width(thumb),
           vips_image_get_height(thumb),
           vips_image_get_bands(thumb));
    g_object_unref(thumb);

    /* Gate 3: write the decoded plane out through vips. */
    if (vips_image_write_to_file(in, "/tmp/w6b_out.png", NULL)) vips_error_exit(NULL);
    struct stat sb;
    EXPECT(stat("/tmp/w6b_out.png", &sb) == 0, "vips wrote /tmp/w6b_out.png");

    printf("vips gate: failures=%d (avg=%.4f)\n", failures, avg);
    g_object_unref(src);
    g_object_unref(in);
    for (int i = 0; i < g_nplanes; i++) free(g_planes[i]);
    vips_shutdown();
    return failures == 0 ? 0 : 1;
}