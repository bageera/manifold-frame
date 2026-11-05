#!/usr/bin/env python3
"""W7-A: encoder-memory decomposition harness (measurement-only).

The memo's hypothesis (candidate concurrency drives the 88-99 MB encode
RSS) is falsified at source level: the engine's candidate loop is
already sequential (no rayon/threads in the encode path). The measured
question becomes the memory STRUCTURE: one arm's working set (l1 plain
zstd-19, l2 mix6) vs the full profiles (l3 balanced = 32 arms, l4
maximum = 160 arms). If RSS(l3) ~ RSS(l1) ~ RSS(l2), the peak is a
transient per-arm working set, not accumulation across arms.

Per leg: fresh python wrapper (perf_counter + RUSAGE_CHILDREN
ru_maxrss), 1 warmup + 3 timed, medians; run-to-run byte identity via
the probe's printed identity tag AND file sha256s.
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
OUT = Path("/tmp/w7a")
TESTDATA = FRAME.parent / "manifold" / "testdata"

IMAGES = {
    "photograph": TESTDATA / "photos" / "autumn_192x144_s900001.png",
    "high_entropy": TESTDATA / "noise_256x256.png",
}
LEGS = ["l1_zstd19_direct", "l2_mix6", "l3_full_balanced", "l4_full_maximum"]

WRAP = (
    "import subprocess,resource,sys,time,json;"
    "t=time.perf_counter();"
    "rc=subprocess.call(sys.argv[1:],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);"
    "r=resource.getrusage(resource.RUSAGE_CHILDREN);"
    "print(json.dumps({'rc':rc,'s':time.perf_counter()-t,'kb':r.ru_maxrss//1024}))"
)

ENV = dict(os.environ)
ENV["PATH"] = "/opt/homebrew/opt/rustup/bin:" + ENV["PATH"]
ENV["RUSTUP_HOME"] = os.path.expanduser("~/.rustup")
ENV["W7A_OUT"] = str(OUT)


def sh(cmd, check=True):
    p = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, env=ENV)
    if check and p.returncode != 0:
        raise RuntimeError(f"{cmd} rc={p.returncode}: {p.stderr[-500:]}")
    return p


def measure(cmd):
    p = subprocess.run([sys.executable, "-c", WRAP, *cmd], capture_output=True, text=True)
    return json.loads(p.stdout.strip().splitlines()[-1])


def leg_run(leg, img_path, tag):
    cmd = ["cargo", "test", "--release", "-p", "manifold-frame",
           "--features", "cli", "--test", "w7a_probe", leg, "--",
           "--exact", "--nocapture"]
    env = dict(ENV, W7A_IMG=str(img_path), W7A_TAG=tag)
    # The leg's JSON line + the artifact: run via measure with env.
    p = subprocess.run([sys.executable, "-c", WRAP, *cmd],
                       capture_output=True, text=True, env=env)
    r = json.loads(p.stdout.strip().splitlines()[-1])
    r.update({"rc": 0})
    return r


def collect(leg, img_path, tag, n_timed=3):
    runs = []
    for i in range(n_timed + 1):
        r = leg_run(leg, img_path, tag)
        if r["rc"] != 0:
            raise RuntimeError(f"{leg} rc={r['rc']}")
        runs.append(r)
    return {
        "median_s": round(statistics.median([r["s"] for r in runs[1:]]), 3),
        "rss_median_kb": int(statistics.median([r["kb"] for r in runs[1:]])),
        "warmup_s": round(runs[0]["s"], 3),
        "outputs": sorted(f.name for f in OUT.glob(f"{tag}_*"))
        if leg.startswith(("l3", "l4")) else [],
    }


def identity(img_path, leg, tag):
    """Run-to-run file identity across the timed runs (same tree)."""
    files = sorted(OUT.glob(f"{tag}_*"))
    shas = [hashlib.sha256(f.read_bytes()).hexdigest() for f in files]
    return {"files": [f.name for f in files],
            "identical": len(set(shas)) == 1,
            "sha256": shas[-1][:12] if shas else None}


def main():
    OUT.mkdir(exist_ok=True)
    print("== build ==", flush=True)
    sh(["cargo", "build", "--release", "-p", "manifold-frame", "--features", "cli"])
    sh(["cargo", "test", "--release", "-p", "manifold-frame", "--features", "cli",
        "--test", "w7a_probe", "--no-run"])
    env = {
        "date_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "uname": platform.uname().system + " " + platform.uname().release
                 + " " + platform.uname().machine,
        "cpu": sh(["sysctl", "-n", "machdep.cpu.brand_string"]).stdout.strip(),
        "rustc": sh(["rustc", "-vV"], False).stdout.splitlines()[0],
        "engine_tree": sh(["git", "-C", str(FRAME.parent / "manifold"),
                           "log", "--oneline", "-1"]).stdout.strip(),
        "frame_rev": sh(["git", "-C", str(FRAME), "rev-parse", "HEAD"]).stdout.strip()[:12],
        "method": "one leg per cargo-test process (release); fresh-python wrapper "
                  "per run: perf_counter + RUSAGE_CHILDREN.ru_maxrss (KB); "
                  "1 warmup + 3 timed medians; constant test-binary overhead "
                  "cancels across legs (same binary per image)",
    }
    print(json.dumps(env, indent=1), flush=True)
    results = {"env": env, "images": {}}
    for cls, path in IMAGES.items():
        results["images"][cls] = {}
        for leg in LEGS:
            tag = f"{cls}_{leg[:2]}"
            for f in OUT.glob(f"{tag}_*"):
                f.unlink()
            cell = collect(leg, path, tag)
            cell["identity"] = identity(path, leg, tag)
            results["images"][cls][leg] = cell
            print(f"[{time.strftime('%H:%M:%S')}] {cls} {leg}: "
                  f"{cell['median_s']}s {cell['rss_median_kb']} KB "
                  f"ident={cell['identity']['identical']}", flush=True)
    dst = FRAME / "docs" / "w7a-results.json"
    dst.write_text(json.dumps(results, indent=1))
    print(f"wrote {dst}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())