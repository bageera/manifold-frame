#!/usr/bin/env python3
"""W7-B1: attribute the zstd-19 encoder working set (measurement-only).

Extends the W7-A harness pattern to a parametric attribution matrix in
`tests/w7b1_probe.rs` (one leg per cargo-test process; constant
test-binary overhead cancels across legs):

  a. level ladder       l3 / l10 / l19 on the same synthesized 256x256
                        noise plane (level-independent floor visible as
                        the base, level-19 increment as ladder delta)
  b. window knob        level 19 with ZSTD_c_windowLog 23 / 20 / 16
                        (RSS AND bytes both recorded; the leg self-
                        roundtrips its frame so the knob is verified)
  c. plane-size scale   level 19 on 64x64 / 128x128 / 256x256
  d. engine anchor      the W7-A l1 replication in-run (drift check,
                        not an identity oracle)

Every zstd_leg row also prints the analytic ZSTD_sizeof_CCtx() — the
zstd-internal context bytes for that (level, window) — at no extra
allocation (params are pre-set on the same CCtx the stream uses).

Per leg: fresh python wrapper per run (perf_counter + RUSAGE_CHILDREN
ru_maxrss, bytes -> //1024), 1 warmup + 3 timed, medians; run-to-run
byte identity via sha256 over the 4 kept artifacts. Matrix results +
analysis are written to docs/w7b1-results.json and printed.
"""

import hashlib
import json
import os
import platform
import statistics
import subprocess
import sys
import time
from pathlib import Path

FRAME = Path(__file__).resolve().parent.parent
OUT = Path("/tmp/w7b1")

# (leg name, cargo-test filter name, env extras) — the matrix.
LEGS = [
    ("a_null_base", "null_leg", {}),
    ("a_l3", "zstd_leg", {"W7B1_LEVEL": "3"}),
    ("a_l10", "zstd_leg", {"W7B1_LEVEL": "10"}),
    ("a_l19", "zstd_leg", {"W7B1_LEVEL": "19"}),
    ("b_l19_w23", "zstd_leg", {"W7B1_LEVEL": "19", "W7B1_WINDOW": "23"}),
    ("b_l19_w20", "zstd_leg", {"W7B1_LEVEL": "19", "W7B1_WINDOW": "20"}),
    ("b_l19_w16", "zstd_leg", {"W7B1_LEVEL": "19", "W7B1_WINDOW": "16"}),
    ("b2_l19_pledge", "zstd_leg", {"W7B1_LEVEL": "19", "W7B1_PLEDGE": "1"}),
    ("c_l19_s64x64", "zstd_leg", {"W7B1_LEVEL": "19", "W7B1_WIDTH": "64", "W7B1_HEIGHT": "64"}),
    ("c_l19_s128x128", "zstd_leg", {"W7B1_LEVEL": "19", "W7B1_WIDTH": "128", "W7B1_HEIGHT": "128"}),
    ("d_anchor_engine", "d_anchor_engine", {}),
]

WRAP = (
    "import subprocess,resource,sys,time,json;"
    "t=time.perf_counter();"
    "p=subprocess.run(sys.argv[1:],capture_output=True,text=True);"
    "r=resource.getrusage(resource.RUSAGE_CHILDREN);"
    "out=[l for l in p.stdout.splitlines() if l.startswith('{')];"
    "print(json.dumps({'rc':p.returncode,'s':time.perf_counter()-t,"
    "'kb':r.ru_maxrss//1024,'leg_json':out[-1] if out else None,"
    "'tail':p.stdout.splitlines()[-1] if p.stdout else '',"
    "'err':p.stderr[-200:]}))"
)

ENV = dict(os.environ)
ENV["PATH"] = "/opt/homebrew/opt/rustup/bin:" + ENV["PATH"]
ENV["RUSTUP_HOME"] = os.path.expanduser("~/.rustup")
ENV["W7B1_OUT"] = str(OUT)


def sh(cmd, check=True):
    p = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, env=ENV)
    if check and p.returncode != 0:
        raise RuntimeError(f"{cmd} rc={p.returncode}: {p.stderr[-500:]}")
    return p


def run_once(leg, fn, extras, tag):
    """One cargo-test process under the fresh RSS wrapper."""
    cmd = ["cargo", "test", "--release", "-p", "manifold-frame",
           "--features", "cli", "--test", "w7b1_probe", fn, "--",
           "--exact", "--nocapture"]
    env = dict(ENV, W7B1_TAG=tag, **extras)
    p = subprocess.run([sys.executable, "-c", WRAP, *cmd],
                       capture_output=True, text=True, env=env)
    if not p.stdout.strip():
        raise RuntimeError(f"{leg} wrapper produced no output: {p.stderr[-200:]}")
    r = json.loads(p.stdout.strip().splitlines()[-1])
    arts = sorted(OUT.glob(f"{tag}_*"))
    r["artifacts"] = {a.name: a.stat().st_size for a in arts}
    r["sha256"] = [hashlib.sha256(a.read_bytes()).hexdigest()[:12] for a in arts]
    return r


def collect(leg, fn, extras, n_timed=3):
    """1 warmup + n timed runs; artifact from every run is kept."""
    runs = []
    for i in range(n_timed + 1):
        tag = f"{leg}_r{i}"
        r = run_once(leg, fn, extras, tag)
        if r["rc"] != 0:
            raise RuntimeError(f"{leg} rc={r['rc']}: {r.get('tail','')} {r.get('err','')}")
        runs.append(r)
    timed = runs[1:]
    leg_json = [json.loads(r["leg_json"]) for r in timed if r["leg_json"]]
    return {
        "median_s": round(statistics.median([r["s"] for r in timed]), 3),
        "rss_median_kb": int(statistics.median([r["kb"] for r in timed])),
        "rss_all_kb": [r["kb"] for r in timed],
        "warmup_s": round(runs[0]["s"], 3),
        "bytes": leg_json[-1] if leg_json else None,
        "identity": {
            "identical": len({s for r in timed for s in r["sha256"]}) == 1,
            "sha256": sorted({s for r in timed for s in r["sha256"]})[0]
            if timed else None,
        },
    }


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    print("== build ==", flush=True)
    sh(["cargo", "test", "--release", "-p", "manifold-frame",
        "--features", "cli", "--test", "w7b1_probe", "--no-run"])

    env_ = {
        "date_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "uname": platform.uname().system + " " + platform.uname().release
                 + " " + platform.uname().machine,
        "cpu": sh(["sysctl", "-n", "machdep.cpu.brand_string"]).stdout.strip(),
        "rustc": sh(["rustc", "-vV"], False).stdout.splitlines()[0],
        "engine_tree": sh(["git", "-C", str(FRAME.parent / "manifold"),
                           "log", "--oneline", "-1"]).stdout.strip(),
        "frame_rev": sh(["git", "-C", str(FRAME), "rev-parse", "HEAD"]).stdout.strip()[:12],
        "zstd_crate": "zstd 0.13.3 / zstd-safe 7.3.0 (zstd-sys 2.1.2+zstd.1.5.7, Cargo.lock)",
        "method": "one cargo-test process per leg (release, --features cli); fresh-python "
                  "wrapper per run: perf_counter + RUSAGE_CHILDREN.ru_maxrss (KB, bytes/1024); "
                  "1 warmup + 3 timed medians; constant test-binary overhead cancels across "
                  "legs; per-leg analytic ZSTD_sizeof_CCtx() printed by the same CCtx the "
                  "stream uses (no second context, RSS column stays clean); legs a-c "
                  "synthesize the plane (no engine code), leg d = engine img::load + zstd-19 "
                  "anchor on the current tree (drift check, not identity oracle)",
    }
    print(json.dumps(env_, indent=1), flush=True)
    results = {"env": env_, "legs": {}}
    for leg, fn, extras in LEGS:
        cell = collect(leg, fn, extras)
        results["legs"][leg] = cell
        b = cell["bytes"] or {}
        print(f"[{time.strftime('%H:%M:%S')}] {leg}: "
              f"{cell['median_s']}s {cell['rss_median_kb']} KB "
              f"bytes={b.get('len')} sizeof_cctx={b.get('sizeof_cctx')} "
              f"ident={cell['identity']['identical']}", flush=True)
    dst = FRAME / "docs" / "w7b1-results.json"
    dst.write_text(json.dumps(results, indent=1))
    print(f"wrote {dst}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
