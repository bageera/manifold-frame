# MNFX W6-D — Performance Baselines (W6-D1 pilot)

Measurement-only gate: exact reconstruction was verified for every accepted
cell before timing was accepted; see `w6d-results.json` for the full record
(env, corpus hashes, commands, unmeasured-cell labels).

- Host: Apple M2 Pro, 16 GiB, Darwin 25.6.0 arm64
- Software: rustc rustc 1.99.1 (58cf8f9c3 2026-10-13); Version: ImageMagick 7.1.2-35 Q16-HDRI aarch64 acaf7e418:20261031 https://imagemagick.org; cwebp 1.6.0; PIL 12.2.0
- manifold-frame rev 30d02c480b71 (codec pin `manifold-codec = { path = "../manifold", version = "1.14" }`); adapter rev df303483bea7
- All timings: end-to-end CLI, 1 warm-up + 3 timed runs, median; peak RSS per run
  (fresh-process `RUSAGE_CHILDREN.ru_maxrss`); warm filesystem cache after run 0.

## D1 — RGB8 full codec comparison

| Image class | MNFX Balanced | MNFX Maximum | PNG-9 | WebP-9 |
| --- | --- | --- | --- | --- |
| photograph | 20,182 B · enc 5.208s · dec 0.004s · 88,624 KB | 18,818 B · enc 31.807s · dec 0.08s · 88,496 KB | 25,452 B · enc 0.065s · dec 0.026s · 5,680 KB | 19,878 B · enc 2.472s · dec 0.017s · 35,504 KB |
| screenshot | 701 B · enc 20.275s · dec 0.005s · 95,008 KB | unmeasured — expensive configuration outside the W6-D1 pilot scope | 2,035 B · enc 0.019s · dec 0.017s · 7,952 KB | 320 B · enc 1.45s · dec 0.01s · 40,944 KB |
| gradient | 288 B · enc 9.843s · dec 0.005s · 89,424 KB | 288 B · enc 69.029s · dec 0.007s · 89,760 KB | 1,884 B · enc 0.017s · dec 0.011s · 6,720 KB | 148 B · enc 1.417s · dec 0.007s · 19,792 KB |
| repeat_pattern | 335 B · enc 7.993s · dec 0.004s · 88,704 KB | unmeasured — expensive configuration outside the W6-D1 pilot scope | 1,785 B · enc 0.012s · dec 0.012s · 6,496 KB | 358 B · enc 0.276s · dec 0.006s · 15,408 KB |
| high_entropy | 196,946 B · enc 7.563s · dec 0.004s · 99,376 KB | unmeasured — expensive configuration outside the W6-D1 pilot scope | 197,419 B · enc 0.015s · dec 0.016s · 6,608 KB | 196,688 B · enc 3.232s · dec 0.011s · 26,816 KB |

Byte ratios R = MNFX bytes / baseline bytes (below 1.0 = MNFX smaller):

- photograph: vs PNG-9: 0.79; vs WebP-9: 1.02; maximum marginal: 51 B/s extra
- screenshot: vs PNG-9: 0.34; vs WebP-9: 2.19
- gradient: vs PNG-9: 0.15; vs WebP-9: 1.95; maximum marginal: 0 B/s extra
- repeat_pattern: vs PNG-9: 0.19; vs WebP-9: 0.94
- high_entropy: vs PNG-9: 1.00; vs WebP-9: 1.00

Unmeasured classes/cells are labeled in `w6d-results.json` (`unmeasured_classes`, per-cell status).

## D2 — Extended sample formats (valid MNFX v1 plane representations)

| Format | MNFX bytes | plane bytes | in-memory decode (C ABI, median of 5) | reconstruction |
| --- | --- | --- | --- | --- |
| rgba8 | 77,225 | 143,360 | 2697.0 µs (2,848 KB) | exact |
| gray16 | 47,990 | 71,680 | 1004.0 µs (2,800 KB) | exact |
| rgb16 | 145,854 | 215,040 | 3383.0 µs (3,904 KB) | exact |

Byte-ratio baselines (labeled): png48_rgb16_baseline: 63,155 B; png48_gray16_baseline: 62,002 B; webp9_rgba_lossless: 37,970 B

D2 note: gray16/rgb16 end-to-end consumer decode: no adapter PNG-16 writer yet; in-memory tiers above are the measured decode numbers. The W3-R1 split-vs-interleaved rate-control
result remains the historical compression evidence; interleaved zstd is not a
valid MNFX v1 single-chunk encoding and is excluded from these measurements.
