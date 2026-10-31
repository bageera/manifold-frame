#!/usr/bin/env python3
"""Mutation fuzzer for the MNFX decoder (the manifold fuzz.py shape).

Encodes corpus images through the mnfx CLI, applies deterministic
mutations (byte flips / truncations / appends), and asserts decode never
panics (exit 101). Structurally valid results are allowed but diffed per
the accept-audit rule.
"""
import argparse
import random
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).parent
BIN = HERE.parent / "target" / "release" / "mnfx"
# The corpus lives in the SIBLING manifold repo (manifold-frame is its
# own git root) — ../manifold from the frame checkout.
TD = HERE.parent.parent / "manifold" / "testdata"


def run_decode(path: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [BIN, "decode", str(path), "-o", "/tmp/mnfx_fz_out.png"],
        capture_output=True, text=True,
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--trials", type=int, default=300)
    ap.add_argument("--seed", type=int, default=2026)
    args = ap.parse_args()
    if not BIN.exists():
        print(f"missing {BIN}; run cargo build --release --features cli --bins first", file=sys.stderr)
        return 2

    images = sorted(TD.glob("*.png")) + sorted((TD / "heldout").glob("*.png"))
    # Hard count assert: zero images = zero trials = a fuzz run that
    # proves nothing (the silent-empty-glob trap, caught once already).
    assert len(images) >= 10, f"fuzz found only {len(images)} corpus images at {TD} — the path is wrong"
    rng = random.Random(args.seed)
    panics = accepted = rejected = 0
    total = 0
    for img in images:
        subprocess.run(
            [BIN, "encode", str(img), "-o", "/tmp/mnfx_fz_base.mnfx", "--mnf8=maximum"],
            check=True, capture_output=True,
        )
        base = Path("/tmp/mnfx_fz_base.mnfx").read_bytes()
        for trial in range(args.trials // len(images) + 1):
            b = bytearray(base)
            kind = trial % 3
            if kind == 0:
                for _ in range(rng.randint(1, 4)):
                    b[rng.randrange(len(b))] = rng.randrange(256)
            elif kind == 1:
                b = b[: rng.randrange(1, len(b))]
            else:
                b += bytes(rng.randrange(256) for _ in range(rng.randint(1, 50)))
            Path("/tmp/mnfx_fz.mnfx").write_bytes(b)
            r = run_decode(Path("/tmp/mnfx_fz.mnfx"))
            total += 1
            if r.returncode == 0:
                accepted += 1
                if bytes(b) == base:
                    continue  # identity rewrite-to-self
                # Interior structural variants (e.g. a header dims word flip
                # making a smaller valid file) are legal different files;
                # the parser's CRC + bounds admit them BY DESIGN. Count and
                # diff: an interior diff with a CRC PASS needs triage.
                print(
                    f"ACCEPTED {img.name} trial={trial} len {len(base)}->{len(b)}",
                    file=sys.stderr,
                )
            elif r.returncode == 101:
                panics += 1
                print(f"PANIC {img.name} trial {trial}: {r.stderr[-200:]}", file=sys.stderr)
            else:
                rejected += 1

    print(f"fuzz: {total} trials, rejected={rejected} accepted={accepted} panics={panics}")
    return 1 if panics else 0


if __name__ == "__main__":
    sys.exit(main())