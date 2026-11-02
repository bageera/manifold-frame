#!/usr/bin/env python3
"""W5-D ABI mutation fuzzer (scripts/fuzz.py's shape, over the C ABI).

Encodes the manifold corpus with the mnfx CLI, applies deterministic
mutations, then runs target/release/abi_fuzz_runner ONCE over the
inputs: every decode is in-process, so any panic/abort/segfault is a
nonzero runner exit = the alarm. The accepted set is diffed per the
accept-audit rule: only byte-identity and pure appends may decode.
"""
import argparse
import random
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).parent
ROOT = HERE.parent
RUNNER = ROOT / "target" / "release" / "abi_fuzz_runner"
MNFX = ROOT / "target" / "release" / "mnfx"
TD = ROOT.parent / "manifold" / "testdata"
OUT = Path("/tmp/mnfx_abi_fuzz")

STATUS_OK = 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--trials", type=int, default=340)
    ap.add_argument("--seed", type=int, default=2026)
    ap.add_argument("--images", type=int, default=0, help="cap the corpus (0 = all)")
    args = ap.parse_args()
    for tool, hint in (
        (RUNNER, "cargo build -p manifold-frame-capi --release --bins"),
        (MNFX, "cargo build --release --features cli (the mnfx bin)"),
    ):
        if not tool.exists():
            print(f"missing {tool}; run {hint} first", file=sys.stderr)
            return 2
    images = sorted(TD.glob("*.png")) + sorted((TD / "heldout").glob("*.png"))
    # Hard count assert: zero images = zero trials = a run proving nothing.
    assert len(images) >= 10, f"fuzz found only {len(images)} corpus images at {TD} — the path is wrong"
    if args.images:
        images = images[: args.images]

    inputs = OUT / "inputs"
    inputs.mkdir(parents=True, exist_ok=True)
    for old in inputs.glob("*.mnfx"):
        old.unlink()
    rng = random.Random(args.seed)
    per_img = args.trials // len(images) + 1
    bases: dict[str, str] = {}
    made = 0
    for img in images:
        base = OUT / f"{img.stem}.mnfx"
        if not base.exists():  # resumable: reuse previously encoded bases
            subprocess.run(
                [str(MNFX), "encode", str(img), "-o", str(base), "--mnf8=balanced"],
                check=True,
                capture_output=True,
            )
        data = base.read_bytes()
        for trial in range(per_img):
            b = bytearray(data)
            kind = trial % 3
            if kind == 0:
                for _ in range(rng.randint(1, 4)):
                    b[rng.randrange(len(b))] = rng.randrange(256)
            elif kind == 1:
                b = b[: rng.randrange(1, len(b))]
            else:
                b += bytes(rng.randrange(256) for _ in range(rng.randint(1, 50)))
            name = f"{img.stem}_{trial}.mnfx"
            (inputs / name).write_bytes(bytes(b))
            bases[name] = img.stem
            made += 1

    p = subprocess.run([str(RUNNER), str(inputs)], capture_output=True, text=True, timeout=600)
    sys.stdout.write(p.stdout[-4000:])
    if p.stderr:
        sys.stderr.write(p.stderr[-2000:])
    if p.returncode != 0:
        print(
            f"\nABI FUZZ ALARM: runner exited {p.returncode} (crash/panic/segfault); "
            "the last stdout line pins the input (inputs in /tmp/mnfx_abi_fuzz/inputs)",
            file=sys.stderr,
        )
        return 1
    lines = [l for l in p.stdout.splitlines() if l and l.rsplit(None, 1)[-1].lstrip("-").isdigit()]
    if len(lines) != made:
        print(f"\nABI FUZZ ALARM: runner reported {len(lines)} of {made} inputs", file=sys.stderr)
        return 1
    accepted = 0
    appends = 0
    for line in lines:
        name, raw = line.rsplit(None, 1)
        if int(raw) != STATUS_OK:
            continue
        accepted += 1
        mutant = (inputs / name).read_bytes()
        base = (OUT / f"{bases[name]}.mnfx").read_bytes()
        # Accept-audit rule: identity or pure append = benign.
        if mutant == base or mutant.startswith(base):
            if mutant != base:
                appends += 1
            continue
        print(f"ACCEPTED-SET HOLE: {name} decodes and is neither the base nor a pure append", file=sys.stderr)
        return 1
    print(
        f"abi_fuzz: {made} trials, crash=0, rejected={made - accepted}, "
        f"accepted={accepted} (of which {appends} pure appends — all benign)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())