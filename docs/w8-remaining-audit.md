# Pre-W8-F CLI audit — retained items (not yet resolved)

These are carried explicitly so they cannot be assumed fixed. The W8-C
slice closed the producer-serializer migration and the ICC-contract
comment; the items below remain open audit rows for the pre-release
pass (W8-F).

| # | Item | Source | Status |
| --- | --- | --- | --- |
| 1 | Bare `--mnf8` flag is accepted and silently ignored (equal to the default balanced profile but never documented as a no-op) | W8-C review thread | open |
| 2 | Help behavior: usage text prints to stderr with exit 2 on a missing command — the help-and-error split (exit codes for `--help`/bad args) is unpinned by tests | W8-C review thread | open |
| 3 | Pixel-count arithmetic in the CLI's reported `(+N framing)` line and the width×height overflow reporting path is untested at the u32 boundary (dims at/near the parse-header checked_mul ceiling) | W8-C review thread | open |
| 4 | Metadata pass-through remains DOCUMENTED-AS-ABSENT (W8-C comment correction): pixels-only CLI is the v1 contract; full lossless metadata conversion is a feature, not an omission | W8-C review thread | contract, not a pending bug |
