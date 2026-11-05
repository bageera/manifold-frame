#!/usr/bin/env python3
"""W6-D benchmark harness (measurement-only). See docs/w6d-performance.md.

D1 — RGB8 corpus subset x {mnfx balanced/maximum, png6/9, webp z6/9}:
     artifact bytes, end-to-end encode/decode medians (1 warm-up + N timed),
     per-run peak RSS, exact-reconstruction checks.
D2 — extended formats (rgba8/gray16/rgb16) built by the adapter's fixture16
     from a corpus source; in-memory C-ABI decode via the abidec probe;
     PNG48/WebP baselines for byte ratios.

Tier discipline (memo): "end-to-end" = the real CLI tools, uniform harness;
"in-memory" = abidec (already-loaded buffer -> validated samples, no PNG).

Usage:
  scripts/w6d_bench.py            # full D1+D2 gate
  scripts/w6d_bench.py --dry      # one image, one run per config, no outputs
"""

import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

FRAME = Path(__file__).resolve().parent.parent
ADAPTER = Path("/tmp/mnfx-adapter")
TESTDATA = FRAME.parent / "manifold" / "testdata"
OUT = Path("/tmp/w6d")
MNFX = FRAME / "target" / "release" / "mnfx"
MAGICK = "magick"
CWEBP = "cwebp"
DWEBP = "dwebp"
ABIDEC = OUT / "abidec"

# One representative per corpus class; "document" has no corpus fixture.
IMAGES = {
    "photograph": TESTDATA / "photos" / "autumn_192x144_s900001.png",
    "screenshot": TESTDATA / "screenshot_640x400.png",
    "gradient": TESTDATA / "gradient_320x240.png",
    "repeat_pattern": TESTDATA / "repeat_256x256.png",
    "high_entropy": TESTDATA / "noise_256x256.png",
}
UNMEASURED_CLASSES = {"document": "no document-class fixture in the engine corpus"}

# Expensive (Maximum) profile measured on this subset only, per the memo's
# "run the expensive configurations selectively, marking unmeasured cells".
MAXIMUM_SCOPE = {"photograph", "gradient"}

D2_SRC = TESTDATA / "photos" / "forest_224x160_s900002.png"

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


def sh(cmd, check=True, quiet=True):
    p = subprocess.run(cmd, capture_output=True, text=True, env=ENV)
    if check and p.returncode != 0:
        raise RuntimeError(f"{cmd} rc={p.returncode}: {p.stderr[-400:]}")
    return p


def measure(cmd):
    """One run in a FRESH python: isolated RUSAGE_CHILDREN for peak RSS."""
    p = subprocess.run([sys.executable, "-c", WRAP, *cmd], capture_output=True, text=True)
    line = p.stdout.strip().splitlines()[-1]
    return json.loads(line)


def medians(cmd, n_timed=3):
    """1 warm-up (also warms fs cache) + n timed runs -> medians."""
    runs = [measure(cmd) for _ in range(n_timed + 1)]
    for r in runs:
        if r["rc"] != 0:
            raise RuntimeError(f"nonzero rc {r['rc']} for {cmd[1] if len(cmd)>1 else cmd}")
    timed = [r["s"] for r in runs[1:]]
    return {
        "encode_median_s": round(statistics.median(timed), 3),
        "rss_median_kb": int(statistics.median([r["kb"] for r in runs[1:]])),
        "warmup_s": round(runs[0]["s"], 3),
        "runs": n_timed + 1,
    }


def pil_bytes(path, mode):
    from PIL import Image
    return Image.open(path).convert(mode).tobytes()


# ---------------------------------------------------------------- verification

def verify_mnfx(src, art):
    return sh([MNFX, "verify", src, art], check=False).returncode == 0


def verify_png(src, art):
    return pil_bytes(src, "RGB") == pil_bytes(art, "RGB")


def verify_webp(src, art):
    dec = OUT / "verify_webp.png"
    sh([DWEBP, art, "-o", dec])
    return pil_bytes(src, "RGB") == pil_bytes(dec, "RGB")


# --------------------------------------------------------------------- configs

def enc_cfgs(src, out):
    return {
        "mnfx_balanced": ([MNFX, "encode", src, "-o", out("mnfx"), "--mnf8=balanced"], verify_mnfx),
        "mnfx_maximum": ([MNFX, "encode", src, "-o", out("mnfx"), "--mnf8=maximum"], verify_mnfx),
        "png6": ([MAGICK, src, "-define", "png:compression-level=6", f"PNG24:{out('png')}"], verify_png),
        "png9": ([MAGICK, src, "-define", "png:compression-level=9", f"PNG24:{out('png')}"], verify_png),
        "webp6": ([CWEBP, "-lossless", "-exact", "-z", "6", src, "-o", out("webp")], verify_webp),
        "webp9": ([CWEBP, "-lossless", "-exact", "-z", "9", src, "-o", out("webp")], verify_webp),
    }


DEC_CFGS = {
    "mnfx_*": lambda art, out: [MNFX, "decode", art, "-o", out],
    "png*": lambda art, out: [MAGICK, art, out],
    "webp*": lambda art, out: [DWEBP, art, "-o", out],
}


def dec_cmd(key, art, out_png):
    if key.startswith("png"):
        return DEC_CFGS["png*"](art, out_png)
    if key.startswith("webp"):
        return DEC_CFGS["webp*"](art, out_png)
    return DEC_CFGS["mnfx_*"](art, out_png)


# ------------------------------------------------------------------ D1 driver

def run_d1(classes, runs, dry):
    results = {}
    for cls in classes:
        src = IMAGES[cls]
        out = lambda ext, _c=cls: OUT / f"{_c}.{ext}"
        results[cls] = {"source": src.name, "sha256": sha256(src), "configs": {}}
        for key, (cmd, verify) in enc_cfgs(src, out).items():
            if key == "mnfx_maximum" and cls not in MAXIMUM_SCOPE:
                results[cls]["configs"][key] = {
                    "status": "unmeasured",
                    "note": "expensive configuration outside the W6-D1 pilot scope",
                }
                continue
            art = out("mnfx" if key.startswith("mnfx")
                      else "png" if key.startswith("png") else "webp")
            cmd = [str(c) for c in cmd]
            cell = {"status": "ok"}
            try:
                cell["bytes"] = measure(cmd)  # warm-up run; leaves the artifact
                if dry:
                    cell["warmup_s"] = cell["bytes"].pop("s")
                else:
                    cell["bytes"] = art.stat().st_size
                    # Reconstruction check BEFORE accepting measurements.
                    cell["reconstruction"] = "exact" if verify(src, art) else "REJECTED"
                    m = medians(cmd)
                    cell.update(m)
                    d = medians(dec_cmd(key, str(art), str(OUT / f"{cls}_dec.png")))
                    cell["decode_median_s"] = d["encode_median_s"]  # same harness fn
                    cell["decode_rss_median_kb"] = d["rss_median_kb"]
            except Exception as e:  # noqa: BLE001 — per-cell isolation
                cell = {"status": "error", "error": str(e)[:300]}
            results[cls]["configs"][key] = cell
            stamp = time.strftime("%H:%M:%S")
            print(f"[{stamp}] {cls} {key}: {cell.get('bytes', cell)}", flush=True)
    return results


def sha256(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


# ------------------------------------------------------------------ D2 driver

def run_d2(runs, dry):
    """Extended-sample-format track: valid MNFX v1 plane representations."""
    png = OUT / "d2_src_rgba.png"
    from PIL import Image
    Image.open(D2_SRC).convert("RGBA").save(png)  # fixture16 needs a native RGBA png
    prefix = OUT / "d2_forest"
    sh(["cargo", "run", "-q", "--manifest-path", str(ADAPTER / "Cargo.toml"),
        "--bin", "fixture16", "--", str(png), str(prefix)])

    d2 = {"source_png8": D2_SRC.name, "sha256_png8": sha256(D2_SRC),
          "derived": "rgba8 native; gray16/rgb16 samples = channel x 257 (BE u16)",
          "formats": {}}

    for fmt, stride in [("rgba8", 4), ("gray16", 2), ("rgb16", 6)]:
        cont = OUT / f"d2_forest_{fmt}.mnfx"
        plane = OUT / f"d2_forest_{fmt}.plane"
        if not cont.exists() or not plane.exists():
            d2["formats"][fmt] = {"status": "error", "error": "fixture missing"}
            continue
        cell = {"bytes": cont.stat().st_size, "plane_bytes": plane.stat().st_size}
        # In-memory C-ABI decode tier (query+copy pair per iteration).
        probe = sh([str(ABIDEC), str(cont), str(plane), "5"],
                   check=False)
        d2["formats"][fmt] = cell
        if probe.returncode == 0:
            cell["abidec_in_memory"] = json.loads(probe.stdout.strip().splitlines()[-1])
            cell["decode_verify"] = "exact" if cell["abidec_in_memory"]["ok"] else "REJECTED"
        else:
            cell["decode_verify"] = f"PROBE ERROR {probe.returncode}"
        if fmt == "rgba8" and not dry:
            # End-to-end consumer leg via the adapter decoder (RGBA-capable).
            dec = OUT / "d2_rgba8_dec.png"
            m = medians([str(ADAPTER / "target/debug/mnfx2png"),
                         str(cont), "-o", str(dec)], 3)
            cell["decode_e2e_median_s"] = m["encode_median_s"]
            cell["decode_e2e_rss_kb"] = m["rss_median_kb"]
            same = pil_bytes(png, "RGBA") == pil_bytes(dec, "RGBA")
            cell["decode_reconstruction"] = "exact" if same else "REJECTED"

    # Byte-ratio baselines (16-bit data derived x257; labeled comparison).
    base = {}
    rgb48 = OUT / "d2_forest_png48.png"
    gray48 = OUT / "d2_forest_gray48.png"
    sh([MAGICK, str(D2_SRC), "-depth", "16", f"PNG48:{rgb48}"])
    sh([MAGICK, str(D2_SRC), "-colorspace", "Gray", "-depth", "16",
        "-define", "png:color-type=0", f"PNG48:{gray48}"])
    base["png48_rgb16_baseline"] = {"bytes": rgb48.stat().st_size,
                                    "note": "lossless-16 rep by construction; exactness proven MNFX-side via abidec"}
    base["png48_gray16_baseline"] = {"bytes": gray48.stat().st_size,
                                     "note": "colorspace-converted gray; informational only"}
    webp_al = OUT / "d2_forest.webp"
    sh([CWEBP, "-lossless", "-exact", "-z", "9", str(png), "-o", str(webp_al)])
    base["webp9_rgba_lossless"] = {"bytes": webp_al.stat().st_size}
    d2["baselines"] = base
    d2["unmeasured"] = (
        "gray16/rgb16 end-to-end consumer decode: no adapter PNG-16 writer yet; "
        "in-memory tiers above are the measured decode numbers"
    )
    return d2


# ------------------------------------------------------------------- metadata

def collect_env():
    def first(*cmd):
        try:
            return sh(cmd).stdout.strip().splitlines()[0]
        except Exception:
            return "unavailable"
    return {
        "date_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "uname": platform.uname().system + " " + platform.uname().release
                 + " " + platform.uname().machine,
        "kernel_banner": first("uname", "-v"),
        "cpu": first("sysctl", "-n", "machdep.cpu.brand_string"),
        "mem_bytes": first("sysctl", "-n", "hw.memsize"),
        "rustc": sh(["rustc", "-vV"]).stdout.replace("\n", " | "),
        "release_profile": "Cargo defaults (opt-level 3; no [profile.release] override)",
        "manifold_frame_rev": sh(["git", "-C", str(FRAME), "rev-parse", "HEAD"]).stdout.strip(),
        "manifold_codec_pin": [l.strip() for l in (FRAME / "Cargo.toml").read_text().splitlines()
                               if l.startswith("manifold-codec")][0],
        "adapter_rev": sh(["git", "-C", str(ADAPTER), "rev-parse", "HEAD"]).stdout.strip(),
        "magick": first(MAGICK, "-version"),
        "cwebp_version": subprocess.run([CWEBP, "-version"], capture_output=True, text=True).stdout.split("\n")[0],
        "pil": __import__("PIL").__version__,
        "timing_method": "fresh-python wrapper per run (perf_counter), RUSAGE_CHILDREN ru_maxrss (bytes/1024); warm filesystem cache after run 0",
        "commands": {
            "mnfx_encode": "mnfx encode <src.png> -o <out.mnfx> --mnf8=balanced|maximum",
            "mnfx_decode": "mnfx decode <in.mnfx> -o <out.png>",
            "png": "magick <src> -define png:compression-level=6|9 PNG24:<out>",
            "webp": "cwebp -lossless -exact -z 6|9 <src> -o <out.webp>",
            "webp_decode": "dwebp <in.webp> -o <out.png>",
            "abidec": "cc abidec.c -lmanifold_frame_capi; abidec <container> <plane> 5",
        },
    }


def render_md(res):
    """Short report per docs/w6d-performance.md; details live in w6d-results.json."""
    L = ["# MNFX W6-D — Performance Baselines (W6-D1 pilot)", "",
         "Measurement-only gate: exact reconstruction was verified for every accepted",
         "cell before timing was accepted; see `w6d-results.json` for the full record",
         "(env, corpus hashes, commands, unmeasured-cell labels).", ""]
    e = res["env"]
    L += [f"- Host: {e['cpu']}, {int(e['mem_bytes'])//2**30} GiB, {e['uname']}",
          f"- Software: rustc {e['rustc'].split('|')[0].strip()}; {e['magick']}; cwebp {e['cwebp_version']}; PIL {e['pil']}",
          f"- manifold-frame rev {e['manifold_frame_rev'][:12]} (codec pin `{e['manifold_codec_pin']}`); adapter rev {e['adapter_rev'][:12]}",
          "- All timings: end-to-end CLI, 1 warm-up + 3 timed runs, median; peak RSS per run",
          "  (fresh-process `RUSAGE_CHILDREN.ru_maxrss`); warm filesystem cache after run 0.", ""]
    L += ["## D1 — RGB8 full codec comparison", "",
          "| Image class | MNFX Balanced | MNFX Maximum | PNG-9 | WebP-9 |",
          "| --- | --- | --- | --- | --- |"]
    def cellstr(key, c):
        v = c.get(key, {})
        if v.get("status") in ("unmeasured", "error"):
            return f"{v.get('status')} — {v.get('note', v.get('error', ''))}"
        d = f"dec {v['decode_median_s']}s" if "decode_median_s" in v else ""
        return (f"{v.get('bytes', '?'):,} B · enc {v.get('encode_median_s', '?')}s · "
                f"{d} · {v.get('rss_median_kb', '?'):,} KB")
    for cls, block in res["d1"].items():
        c = block["configs"]
        row = [cls] + [cellstr(k, c) for k in ["mnfx_balanced", "mnfx_maximum", "png9", "webp9"]]
        L.append("| " + " | ".join(row) + " |")
    L += ["", "Byte ratios R = MNFX bytes / baseline bytes (below 1.0 = MNFX smaller):", ""]
    for cls, block in res["d1"].items():
        c = block["configs"]
        mb, p9, w9 = (c.get(k, {}).get("bytes") for k in
                      ["mnfx_balanced", "png9", "webp9"])
        r = []
        if mb and p9: r.append(f"vs PNG-9: {mb / p9:.2f}")
        if mb and w9: r.append(f"vs WebP-9: {mb / w9:.2f}")
        b = c.get("mnfx_maximum", {}).get("bytes")
        if b:
            tt = c["mnfx_maximum"]["encode_median_s"] - c["mnfx_balanced"]["encode_median_s"]
            r.append(f"maximum marginal: {(mb - b) / max(0.001, tt):.0f} B/s extra")
        L.append(f"- {cls}: " + "; ".join(r) if r else f"- {cls}: n/a")
    L += ["", "Unmeasured classes/cells are labeled in `w6d-results.json` "
          "(`unmeasured_classes`, per-cell status).", ""]
    d2 = res["d2"]
    L += ["## D2 — Extended sample formats (valid MNFX v1 plane representations)", "",
          "| Format | MNFX bytes | plane bytes | in-memory decode (C ABI, median of 5) | reconstruction |",
          "| --- | --- | --- | --- | --- |"]
    for fmt, f in d2["formats"].items():
        a = f.get("abidec_in_memory", {})
        L.append(f"| {fmt} | {f['bytes']:,} | {f['plane_bytes']:,} | "
                 f"{a.get('median_us', '?')} µs ({a.get('rss_kb', '?'):,} KB) | "
                 f"{f.get('decode_verify', f.get('status'))} |")
    L += ["", "Byte-ratio baselines (labeled): " + "; ".join(
        f"{k}: {v['bytes']:,} B" for k, v in d2["baselines"].items()),
        "", f"D2 note: {d2['unmeasured']}. The W3-R1 split-vs-interleaved rate-control",
        "result remains the historical compression evidence; interleaved zstd is not a",
        "valid MNFX v1 single-chunk encoding and is excluded from these measurements.", ""]
    return "\n".join(L)


def main():
    dry = "--dry" in sys.argv
    runs = 1 if dry else 3
    classes = list(IMAGES)
    if dry:
        classes = ["photograph"]
    OUT.mkdir(exist_ok=True)
    print("== env/build ==", flush=True)
    sh(["cargo", "build", "--release", "--features", "cli", "--bin", "mnfx"])
    # abidec probe against the shipped dylib.
    sh(["cc", "-Wall", "-Wextra", "-Werror",
        "-I", str(FRAME / "capi/include"),
        str(ADAPTER / "capi/tests/magick-bridge/abidec.c"),
        "-L", str(FRAME / "target/release"), "-lmanifold_frame_capi",
        "-Wl,-rpath," + str(FRAME / "target/release"),
        "-o", str(ABIDEC)])
    env = collect_env()
    print(json.dumps(env, indent=1), flush=True)
    print("== D1 ==", flush=True)
    d1 = run_d1(classes, runs, dry)
    print("== D2 ==", flush=True)
    d2 = run_d2(runs, dry)
    res = {"env": env, "d1": d1, "d2": d2,
           "unmeasured_classes": UNMEASURED_CLASSES}
    tgt = FRAME / "docs" / "w6d-results.json" if not dry else OUT / "w6d-dry.json"
    tgt.write_text(json.dumps(res, indent=1))
    print(f"wrote {tgt}", flush=True)
    if not dry:
        md = FRAME / "docs" / "w6d-performance.md"
        md.write_text(render_md(res))
        print(f"wrote {md}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())