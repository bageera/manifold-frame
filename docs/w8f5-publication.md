# W8-F5 — publication log (F5 GO received)

## Sequence state (live)

| Step | Status |
| --- | --- |
| 1. Freeze clean checkout of `02a4c9a` | DONE — `../manifold-frame-release` worktree at `02a4c9accd6f…`; packaged `.crate` sha `852e26841972192d8132329dd19455bc27602e54b6f56288886fe515613c6a2d` |
| 2. Publish `manifold-frame 0.2.0` | **BLOCKED ON CREDENTIAL** — no crates.io token on this host; the user runs `cargo login` interactively (token never in chat) |
| 3. Registry availability + checksum | pending 2 |
| 4. `manifold-frame-capi` publish | DISPOSED-BY-DESIGN: `publish = false` verified in F3 (the archives are its distribution; not modified — that would change the approved candidate) |
| 5. Independent registry consumers | pending 2 (consume_release.sh at 0.2.0) |
| 6. Tag `v0.2.0` | First cut at `02a4c9a` ABORTED by the publish gate (see incident); re-cut at the machine-fix revision after the registry step, per the approved order |
| 7. Production binary workflow | run 44421511637: builds/validates/consumers ALL GREEN; publish job REFUSED by the tag-version guard — incident below; re-run after the fix |
| 8. Draft verification | pending |
| 9. Public release + downloads | pending |

## Incident (caught by the designed guard, zero publication side effects)

The production tag run (44421511637) passed every build/validate/consumer
job; the publish job then REFUSED: `duplicate package version across
archives: 0.2.0`. Root cause: the tag→version consistency script's
duplicate check keyed on the version alone, while the contract ships ONE
version across TWO platform archives (false positive). Dry runs skip the
publish job entirely, so this was the FIRST-ever execution of that check —
the refusal was correct (nothing published, no draft created; verified:
only the 0.1.x releases exist).

Fix at `62535b4` (build machinery ONLY):

- duplicate check keys on (version, target), one archive per platform;
- the drafted release notes now carry the two standing qualifications
  verbatim (MNF9 resource exposure; fuzz-parity coverage scope).

**Packaged-inputs requalification check:** `git diff 02a4c9a 62535b4`
over the packaged set (Cargo.toml/lock, src/, capi sources+headers,
grammar, README, LICENSE) = **0 changed lines** — the candidate's
shipped content is byte-for-byte unchanged; only the publish gate
script + release notes changed. Requalification = the fresh tag run
(build + ten-leg validation + both consumer stacks + the corrected
publish), executed AFTER the registry publication per the approved
order.

## Hold

The public GitHub release stays unpublished until steps 2–5 complete
(registry publish, availability + checksum read-back, consumer smoke at
0.2.0) — the draft staging exists precisely so the publication cannot
go out before the registry line is verified.
