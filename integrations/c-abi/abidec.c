/*
 * abidec (W6-D): in-memory C-ABI decode benchmark — the "already-loaded
 * compressed buffer" tier: mnfx_decode query+copy pair timed per
 * iteration, no PNG re-encode, no process overhead. Also verifies the
 * decoded plane against a sidecar truth dump on the first iteration.
 *
 * usage: abidec <container.mnfx> <expected.plane> <iterations>
 * stdout: JSON {"ok", "median_us", "min_us", "rss_kb"}
 */
#include "mnfx.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/resource.h>

static uint8_t *slurp(const char *path, size_t *n) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long len = ftell(f);
    fseek(f, 0, SEEK_SET);
    uint8_t *b = malloc((size_t)len);
    if (!b || fread(b, 1, (size_t)len, f) != (size_t)len) {
        free(b);
        b = NULL;
    }
    fclose(f);
    *n = (size_t)len;
    return b;
}

static double now_us(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e6 + (double)ts.tv_nsec / 1e3;
}

int cmp_double(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return x < y ? -1 : x > y ? 1 : 0;
}

int main(int argc, char **argv) {
    if (argc < 4) return 2;
    int iters = atoi(argv[3]);
    if (iters < 1) return 2;
    size_t clen = 0, elen = 0;
    uint8_t *cont = slurp(argv[1], &clen);
    uint8_t *exp = slurp(argv[2], &elen);
    if (!cont || !exp) return 2;

    size_t need = 0;
    uint32_t w = 0, h = 0;
    uint8_t pf = 0, or = 0;
    int32_t st = mnfx_decode(cont, clen, NULL, &need, &w, &h, &pf, &or);
    if (st != MNFX_STATUS_BUF_TOO_SMALL) return 2;
    uint8_t *plane = malloc(need);

    double *times = malloc(sizeof(double) * (size_t)iters);
    int ok = 1;
    for (int i = 0; i < iters; i++) {
        double t0 = now_us();
        st = mnfx_decode(cont, clen, NULL, &need, &w, &h, &pf, &or);
        size_t got = need;
        if (st == MNFX_STATUS_BUF_TOO_SMALL)
            st = mnfx_decode(cont, clen, plane, &got, &w, &h, &pf, &or);
        double t1 = now_us();
        times[i] = t1 - t0;
        if (st != MNFX_STATUS_OK || got != need) {
            printf("{\"ok\": false, \"status\": %d}\n", st);
            ok = 0;
            break;
        }
        if (i == 0 && (got != elen || memcmp(plane, exp, got) != 0)) {
            printf("{\"ok\": false, \"error\": \"plane != expected truth\"}\n");
            ok = 0;
            break;
        }
    }
    if (ok) {
        qsort(times, (size_t)iters, sizeof(double), cmp_double);
        struct rusage ru;
        getrusage(RUSAGE_SELF, &ru);
        printf(
            "{\"ok\": true, \"w\": %u, \"h\": %u, \"pixfmt\": %d, "
            "\"median_us\": %.1f, \"min_us\": %.1f, \"rss_kb\": %zu}\n",
            w, h, pf, times[iters / 2], times[0], (size_t)(ru.ru_maxrss / 1024));
    }
    free(times);
    free(plane);
    free(exp);
    free(cont);
    return ok ? 0 : 1;
}