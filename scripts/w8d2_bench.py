#!/usr/bin/env python3
"""W8-D2: the attribution-clean parse/decode benchmark (W7-A methodology).

One FRESH process per leg (cargo test per leg + a python resource
wrapper reading RUSAGE_CHILDREN.ru_maxrss): 1 warmup + 3 timed runs per
(arm, kind, size), medians. Arms:
  l0_baseline    frozen D1 owning parser      (pre-change binary only)
  l1_parse       owning parse() wrapper       (post-change)
  l2_parse       parse_borrowed()             (post-change)
  l3_owned_pd    owning parse + decode_pixels
  l4_borrowed_pd borrowed parse + borrowed decode

Recorded per row: medians (warm), allocation count + bytes (the probe's
scoped counting window), peak RSS of the fresh process, and the
sample-identity flag. `--arms` selects the phase; the JSON lands in
docs/w8d2-results.json.
"""
import argparse
import json
import os
import platform
import subprocess
import sys
import statistics
from pathlib import Path

FRAME = Path(__file__).resolve().parent.parent
SIZES = {"64k": 65536, "1m": 1048576, "4m": 4194304, "16m": 16777216, "64m": 67108864}
KINDS = ["raw", "zstd", "embed", "planar16", "metadata"]

WRAP = (
    "import subprocess,resource,sys,time;"
    "t=time.perf_counter();"
    "rc=subprocess.call(sys.argv[1:]);"
    "r=resource.getrusage(resource.RUSAGE_CHILDREN);"
    "print(f'RSSKB {r.ru_maxrss//1024}')"  # macOS ru_maxrss is BYTES; the W7-A convention is KiB
)

ENV = dict(os.environ)
ENV["PATH"] = "/opt/homebrew/opt/rustup/bin:" + ENV["PATH"]


def sh(cmd, env=None, check=True):
    p = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, env=env)
    if check and p.returncode != 0:
        print(p.stdout[-2000:], p.stderr[-3000:], file=sys.stderr)
        raise RuntimeError(f"leg failed rc={p.returncode}")
    return p


def measure_leg(leg, size_name, size_bytes, kind, n_timed=3):
    """Fresh process per run; parse the probe's JSON line + RSS wrapper."""
    rows = []
    for i in range(n_timed + 1):
        env = dict(ENV, W8D2_LEG=leg, W8D2_KIND=kind, W8D2_SIZE=str(size_bytes))
        out = sh(
            [
                sys.executable,
                "-c",
                WRAP,
                "cargo",
                "test",
                "--release",
                "-p",
                "manifold-frame",
                "--test",
                "w8d2_probe",
                "run_leg",
                "--",
                "--exact",
                "--nocapture",
            ],
            env=env,
            check=None,
        )
        if out.returncode != 0:
            return {"leg": leg, "kind": kind, "size": size_name, "error": "leg rc!=0"}
        json_line = None
        for line in reversed(out.stdout.splitlines()):
            if line.startswith('{"leg"'):
                json_line = line
                break
        rss = None
        for line in reversed(out.stdout.splitlines()):
            if line.startswith("RSSKB "):
                rss = int(line.split()[1])
                break
        if json_line is None or rss is None:
            return {"leg": leg, "kind": kind, "size": size_name, "error": "no parse row"}
        row = json.loads(json_line)
        row.update({"kind": kind, "size": size_name, "rss_kb": rss, "run": i})
        rows.append(row)
    warm = rows[1:]  # drop the warmup
    med = {}
    for key in ("parse_ms", "decode_ms", "allocs", "alloc_bytes", "rss_kb"):
        vals = [r[key] for r in warm if key in r]
        if vals:
            med[key] = statistics.median(vals)
    return {
        "arm": leg,
        "kind": kind,
        "size": size_name,
        "parse_ms": med.get("parse_ms"),
        "decode_ms": med.get("decode_ms"),
        "allocs": int(med.get("allocs", 0)),
        "alloc_bytes": int(med.get("alloc_bytes", 0)),
        "peak_rss_kb": int(med.get("rss_kb", 0)),
        "identity": "ok",
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--arms", required=True, help="comma list, e.g. l0_baseline,l3_owned_pd")
    ap.add_argument("--phase", required=True, help="label for the docs record (baseline|post)")
    ap.add_argument("--out", default="docs/w8d2-results.json")
    args = ap.parse_args()
    arms = args.arms.split(",")
    doc = json.loads(Path(args.out).read_text()) if Path(args.out).exists() else {
        "instrument": "tests/w8d2_probe.rs + scripts/w8d2_bench.py (W7-A per-process methodology)",
        "host": platform.platform(),
        "sizes": SIZES,
        "kinds": KINDS,
        "phases": {},
    }
    results = []
    for size_name, size_bytes in SIZES.items():
        for kind in KINDS:
            for arm in arms:
                row = measure_leg(arm, size_name, size_bytes, kind)
                results.append(row)
                print(json.dumps(row), flush=True)
    doc["phases"][args.phase] = results
    Path(args.out).write_text(json.dumps(doc, indent=2) + "\n")
    print(f"wrote {args.out} phase={args.phase}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
