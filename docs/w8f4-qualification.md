# W8-F4 — binary distribution qualification (release candidate)

## The qualified revision

```
commit: 02a4c9accd6fe4f7e4ebb422eb32fe81cfd0eea4
branch: main (origin/main == HEAD)
CHANGELOG stamp: [0.2.0] — release candidate rc1
```

## Workflow evidence

Release/consumer workflow `release.yml`, dry-run dispatch:

```
run 44415539917 (qualification run, pinned to 02a4c9a): SUCCESS
  matrix_setup:                                  success
  build  linux x86_64-unknown-linux-gnu:         success
  build  macos aarch64-apple-darwin:             success
  validate (all ten legs, both platforms):       success
  consumer-b4 (C + libvips + ImageMagick, both): success
  publish:                                       SKIPPED  (guard intact)
run 44415474750 (same tree at dce8c43 + one doc commit delta): SUCCESS
  (independent data point; every artifact/consumer gate green)
```

Regular CI at the qualified revision (dce8c43..02a4c9a push):

```
ci.yml @ 02a4c9a: SUCCESS — macos / linux / msrv 1.91 / i686
(i686 = the W8-A3 hostile-length regression on a REAL 32-bit process,
debug + release)
```

## Artifact verification (downloaded and re-verified outside the runner)

| Archive | SHA-256 |
| --- | --- |
| manifold-frame-v0.2.0-x86_64-unknown-linux-gnu.tar.gz | `9b331f219a72cda11789f048169d80b66a671edce94473f5ade2c03cb616ea38` |
| manifold-frame-v0.2.0-aarch64-apple-darwin.tar.gz | `4ad296cdba96a3cddb2367811febcccd54982ca2db6a92b4288e95d18924b18b` |

Independent checks performed locally on the extracted archives:

- Layout complete on both: `bin/mnfx`, `include/mnfx.h`,
  `lib/libmanifold_frame_capi.{so|dylib,a}`,
  `share/mnfx/{mnfx-grammar.md,capi-README.md}`, `LICENSE`, `README.md`,
  `provenance.json`, internal `SHA256SUMS`.
- Internal checksums: **9/9 OK, 0 failures** per archive.
- `provenance.json` pins `git_commit = 02a4c9accd6f` on BOTH archives
  and the vendored engine `manifold-codec 1.16.0` (lock-pinned, the
  B1 sha-verified vendor).
- The shipped `bin/mnfx` reports **v0.2.0** and carries the F1 usage
  pin (`--mnf8` selection semantics + the PIXELS-only contract) — the
  version and audit resolutions are IN the artifact, not just in the
  source tree.
- `share/mnfx/capi-README.md` (shipped) contains the W8-F1
  size-query-vs-reconstruction pin.

## Consumer gates (run by the workflow's consumer-b4 stage, both platforms)

Standalone C consumer (six formats byte-exact), libvips
(pixel-equality + thumbnail + write-back + hostile), ImageMagick
(rgba8/gray16/rgb16 exact + PIL cross-check), linkage audit (no cargo,
no engine sources in the consumer env), relocatability (tree-move
re-link), hostile-input rc control, evidence receipts
(`b4-consumer-aarch64-apple-darwin`, `b4-consumer-x86_64-unknown-linux-gnu`
artifacts present in this run).

## Requalification rule (in force)

Any source change after this qualification triggers a new rc stamp and
a fresh `release.yml` dry-run at the new revision before any
publication action (F5).

## F5 status

UNTOUCHED — publication held pending the explicit F5 authorization; no
tag, no release, the `publish` job skipped in every run.
