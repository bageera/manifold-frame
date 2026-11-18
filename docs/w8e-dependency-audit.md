# W8-E4 — dependency audit disposition (recorded negative result)

Date recorded: from the E-phase working tree (commit class W8-E3/E4).
Commands (run on the workspace at the audited revision):

```
cargo tree -p manifold-frame -e features
cargo tree -p manifold-frame -i zstd
cargo tree -p manifold-frame -i zstd-sys
```

## Verdict: OPTIONAL FRAME-LEVEL ZSTD IS NOT WORTHITS COMPLEXITY

The inverted dependency trees settle it decisively:

```
zstd v0.13.3
├── manifold-codec v1.16.0   ← the ENGINE already pulls the same native
│                              dependency (default features)
│   └── manifold-frame
└── manifold-frame           ← the frame's own direct use (W1 zstd layer)

zstd-sys v2.1.3+zstd.1.5.7
└── zstd-safe v7.3.0
    ├── manifold-codec v1.16.0
    └── zstd v0.13.3 (→ both roots above)
```

`manifold-codec` requires `zstd` (and therefore `zstd-sys`) TRANSITIVELY
under its default feature set. Every consumer linking the frame's
default engine dependency already carries the same native object.

Consequences recorded (not assumed):

1. A `[features] zstd = []` split would remove, at most, the frame's
   own CALL sites — the native link cost and build surface stay for any
   default-feature consumer. The complexity (a second resolution path,
   a cfg-split reader, and a feature whose absence still compiles the
   engine) buys no meaningful footprint or security reduction.
2. The full-featured frame is what the B3 binary-release contract
   ships (the engine is vendored lock-pinned); a reduced-feature
   configuration is not part of any verified distribution contract.
3. No feature flags are introduced. If a future W9+ moves the frame
   off the engine dependency (or the engines drop zstd), this
   disposition should be re-audited THEN — not speculatively.

Audit commands and tree evidence above are reproducible at any revision;
the trees shown come from the audited working set.
