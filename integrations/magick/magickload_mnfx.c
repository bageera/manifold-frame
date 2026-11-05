/*
 * magickload_mnfx.c (W6-C): load an MNFX container through the SHIPPED
 * C ABI (dylib + mnfx.h) and import the validated plane into a
 * MagickCore image. No Manifold Rust internals on this side.
 *
 * Gates per the W6-C memo:
 *   2/3. exact sample values + channel order + alpha, 8-bit and
 *        16-bit legs independently (IM import -> export -> memcmp),
 *   4.   a genuine IM operation (thumbnail) over MNFX pixels,
 *   5.   corrupt MNFX -> controlled error (run_w6c.sh asserts rc==1,
 *        never a signal),
 *   6.   metadata/interpretation documented in the bridge README:
 *        pixel_format decides sRGB/GRAY interpretation; embedded
 *        ICC/EXIF chunks are NOT consumed and no profile is invented.
 */
#include "mnfx.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <MagickCore/MagickCore.h>

static int failures = 0;

#define EXPECT(cond, what)                                                     \
    do {                                                                       \
        if (!(cond)) {                                                         \
            fprintf(stderr, "FAIL: %s\n", what);                               \
            failures++;                                                        \
        }                                                                      \
    } while (0)

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

/* BE (MNFX wire) <-> LE (host ShortPixel) for the 16-bit legs. */
static void swap16(uint8_t *p, size_t len) {
    for (size_t i = 0; i + 1 < len; i += 2) {
        uint8_t t = p[i];
        p[i] = p[i + 1];
        p[i + 1] = t;
    }
}

/* One leg: decode -> import -> export -> memcmp. Returns the Image
 * (owned by caller) or NULL on controlled failure. */
/* Geometry from the CONTAINER (never the call site): bands/map/depth/
 * interp derive from the wire pixel format; the caller names the leg. */
static Image *mnfx_leg(const char *name, const uint8_t *buf, size_t len,
                       ExceptionInfo *e) {
    size_t need = 0;
    uint32_t w = 0, h = 0;
    uint8_t pformat = 0, orient = 0;
    int32_t st = mnfx_decode(buf, len, NULL, &need, &w, &h, &pformat, &orient);
    if (st != MNFX_STATUS_BUF_TOO_SMALL) {
        fprintf(stderr, "[%s] decode status %d\n", name, st);
        failures++;
        return NULL;
    }
    uint8_t *plane = malloc(need);
    size_t got = need;
    st = mnfx_decode(buf, len, plane, &got, &w, &h, &pformat, &orient);
    if (st != MNFX_STATUS_OK || got != need) {
        fprintf(stderr, "[%s] copy pass status %d\n", name, st);
        failures++;
        free(plane);
        return NULL;
    }

    /* The import geometry the container itself declares. */
    const char *map;
    int short_px = 0, want_alpha = 0, bands = 0;
    ColorspaceType interp;
    switch (pformat) {
    case MNFX_PIXFMT_RGB8:
        map = "RGB"; bands = 3; interp = sRGBColorspace; break;
    case MNFX_PIXFMT_RGBA8:
        map = "RGBA"; bands = 4; want_alpha = 1; interp = sRGBColorspace; break;
    case MNFX_PIXFMT_GRAY8:
        map = "I"; bands = 1; interp = GRAYColorspace; break;
    case MNFX_PIXFMT_GRAY16:
        map = "I"; bands = 1; short_px = 1; interp = GRAYColorspace; break;
    case MNFX_PIXFMT_RGB16:
        map = "RGB"; bands = 3; short_px = 1; interp = sRGBColorspace; break;
    default: /* RGBA16 not in the W3 vocabulary; GrayA16 likewise absent */
        fprintf(stderr, "[%s] wire format %d unsupported by the IM importer\n",
                name, pformat);
        failures++;
        free(plane);
        return NULL;
    }
    {
        /* exact plane bound vs geometry (defense in depth; the ABI
         * already guarantees need == w*h*stride) */
        size_t bound = (size_t)w * h * bands * (short_px ? 2 : 1);
        if (need != bound) {
            fprintf(stderr, "[%s] plane %zu != geometry %zu\n", name, need, bound);
            failures++;
            free(plane);
            return NULL;
        }
    }
    (void)want_alpha;
    if (short_px) swap16(plane, need); /* wire BE -> host LE */

    ImageInfo *ii = CloneImageInfo(NULL);
    Image *img = AcquireImage(ii, e);
    strcpy(img->magick, "PNG"); /* in-memory images have no format hint */
    img->columns = w;
    img->rows = h;
    img->depth = short_px ? 16 : 8;
    if (want_alpha) img->alpha_trait = BlendPixelTrait;
    SetImageColorspace(img, interp, e); /* from the container's format */

    StorageType stype = short_px ? ShortPixel : CharPixel;
    if (!ImportImagePixels(img, 0, 0, w, h, map, stype, plane, e)) {
        fprintf(stderr, "[%s] import failed: %s\n", name, e->reason ? e->reason : "?");
        DestroyImage(img);
        DestroyImageInfo(ii);
        free(plane);
        return NULL;
    }
    uint8_t *back = malloc(need);
    if (!ExportImagePixels(img, 0, 0, w, h, map, stype, back, e)) {
        fprintf(stderr, "[%s] export failed: %s\n", name, e->reason ? e->reason : "?");
        free(back);
        DestroyImage(img);
        DestroyImageInfo(ii);
        free(plane);
        return NULL;
    }
    if (short_px) swap16(back, need); /* host LE -> wire BE */
    EXPECT(memcmp(back, plane, need) == 0, "IM storage preserves exact samples + order");
    free(back);
    free(plane);
    DestroyImageInfo(ii);
    printf("[%s] leg ok (%ux%u)\n", name, w, h);
    return img;
}

/* IM writes resolve the path from IMAGE->filename (empty -> stdout);
 * set it + the info mirror, then write. */
static MagickBooleanType write_png(Image *img, const char *path, ExceptionInfo *e) {
    strcpy(img->filename, path);
    ImageInfo *wi = CloneImageInfo(NULL);
    strcpy(wi->filename, path);
    MagickBooleanType ok = WriteImage(wi, img, e);
    DestroyImageInfo(wi);
    return ok;
}

int main(int argc, char **argv) {
    (void)argc;
    MagickCoreGenesis(argv[0], MagickFalse);
    ExceptionInfo *e = AcquireExceptionInfo();

    /* --- 8-bit RGBA leg (transparency) from the adapter-produced
     * container (png2mnfx output of a PIL RGBA source). --- */
    size_t len8 = 0;
    uint8_t *buf8 = slurp(argv[1], &len8);
    if (!buf8) return 2;
    Image *rgba = mnfx_leg("rgba8 (argv1)", buf8, len8, e);
    free(buf8);
    EXPECT(rgba != NULL, "rgba8 leg decoded");
    if (rgba) {
        /* Real IM operation over MNFX pixels (32 wide, explicit rows:
         * ThumbnailImage has no auto-aspect in MagickCore). */
        size_t trows = (size_t)rgba->rows * 32 / (size_t)rgba->columns;
        Image *thumb = ThumbnailImage(rgba, 32, trows, e);
        if (thumb) {
            EXPECT(write_png(thumb, "/tmp/w6c_rgba8_thumb.png", e),
                   "rgba8 thumbnail write");
            DestroyImage(thumb);
        } else {
            EXPECT(0, "thumbnail image failed");
        }
        EXPECT(write_png(rgba, "/tmp/w6c_rgba8_out.png", e), "rgba8 full-res write");
        DestroyImage(rgba);
    }

    /* --- 16-bit legs, independently --- */
    size_t len16 = 0;
    uint8_t *buf16 = slurp(argv[2], &len16);
    if (!buf16) return 2;
    Image *g16 = mnfx_leg("gray16 (argv2)", buf16, len16, e);
    free(buf16);
    EXPECT(g16 != NULL, "gray16 leg decoded");
    if (g16) {
        EXPECT(write_png(g16, "/tmp/w6c_gray16_out.png", e), "gray16 write");
        DestroyImage(g16);
    }

    size_t lenr16 = 0;
    uint8_t *bufr16 = slurp(argv[3], &lenr16);
    if (!bufr16) return 2;
    Image *r16 = mnfx_leg("rgb16 (argv3)", bufr16, lenr16, e);
    free(bufr16);
    EXPECT(r16 != NULL, "rgb16 leg decoded");
    if (r16) {
        EXPECT(write_png(r16, "/tmp/w6c_rgb16_out.png", e), "rgb16 write");
        DestroyImage(r16);
    }

    DestroyExceptionInfo(e);
    MagickCoreTerminus();
    printf("magick gate: failures=%d\n", failures);
    return failures == 0 ? 0 : 1;
}