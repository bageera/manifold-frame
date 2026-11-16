#!/usr/bin/env python3
"""W8-B1: record the build provenance (the reproducibility receipt).

Emits one JSON object pinning every factor the W8-B1 gate names:
- the exact manifold-codec version + its published sha256 (crates.io API,
  the same anchor scripts/vendor_engine.py verifies against);
- the dependency lockfile (path + sha256 of the committed Cargo.lock);
- the Rust toolchain (rustc -V, cargo -V) and the host target triple;
- the crate identity and the git revision the build ran from.
CI uploads the file as a workflow artifact; `--out` also persists it.
Runs on the system python too (tomllib when present, section regex else).
"""
import argparse
import datetime
import hashlib
import json
import re
import subprocess
import sys
import urllib.request
from pathlib import Path


def run(*cmd: str, timeout: int = 30) -> str:
    return subprocess.run(
        cmd, capture_output=True, text=True, timeout=timeout, check=True
    ).stdout.strip()


def toml_section(text: str, header: str) -> str:
    """The raw lines under `[header]` / `[[header]]` up to the next section."""
    out: list = []
    cur = False
    for line in text.splitlines():
        if line.lstrip().startswith("["):
            if cur:
                break
            cur = line.strip().startswith(f"[{header}]") or line.strip().startswith(
                f"[[{header}]]"
            )
            continue
        if cur:
            out.append(line)
    return "\n".join(out)


def scalar(field_in_section: str, text: str):
    m = re.search(rf'^{field_in_section} = "([^"]+)"\s*$', text, re.M)
    return m.group(1) if m else None


def manifest_package() -> dict:
    import tomllib  # python >= 3.11 (CI); 3.9 falls back to the section parse

    with open("Cargo.toml", "rb") as f:
        return tomllib.load(f)["package"]


def manifest_package_fallback() -> dict:
    sec = toml_section(Path("Cargo.toml").read_text(), "package")
    name, version = scalar("name", sec), scalar("version", sec)
    if not (name and version):
        sys.exit("cannot read [package] name/version from Cargo.toml")
    return {"name": name, "version": version}


def lock_engine_version() -> str:
    try:
        import tomllib

        with open("Cargo.lock", "rb") as f:
            lock = tomllib.load(f)
        pins = [p["version"] for p in lock.get("package", []) if p.get("name") == "manifold-codec"]
    except ImportError:
        text = Path("Cargo.lock").read_text()
        pins = [
            scalar("version", b)
            for b in text.split("[[package]]")
            if re.search(r'^name = "manifold-codec"$', b, re.M)
        ]
        pins = [p for p in pins if p]
    if len(pins) != 1:
        sys.exit(f"expected exactly one manifold-codec entry in Cargo.lock, got {pins}")
    return pins[0]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", help="also write the JSON here")
    args = ap.parse_args()

    try:
        package = manifest_package()
    except ImportError:
        package = manifest_package_fallback()
    engine_ver = lock_engine_version()
    lock_hash = hashlib.sha256(Path("Cargo.lock").read_bytes()).hexdigest()
    with urllib.request.urlopen(
        urllib.request.Request(
            f"https://crates.io/api/v1/crates/manifold-codec/{engine_ver}",
            headers={"User-Agent": "mnfx-provenance"},
        ),
        timeout=60,
    ) as r:
        engine_sha = json.load(r)["version"]["checksum"]

    req_m = re.search(
        r"^manifold-codec = .*version = .(\d+\.\d+)", Path("Cargo.toml").read_text(), re.M
    )
    host = re.search(r"^host: (\S+)$", run("rustc", "-vV"), re.M).group(1)
    dirty = run("git", "status", "--porcelain") != ""

    record = {
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "crate": {"name": package["name"], "version": package["version"]},
        "engine": {
            "pinned_version": engine_ver,
            "manifest_requirement": req_m.group(1) if req_m else None,
            "published_sha256": engine_sha,
            "source": "crates.io registry (API-published checksum)",
        },
        "lockfile": {"path": "Cargo.lock", "sha256": lock_hash},
        "toolchain": {"rustc": run("rustc", "-V"), "cargo": run("cargo", "-V")},
        "target": host,
        "git": {"commit": run("git", "rev-parse", "HEAD"), "tree_dirty": dirty},
    }
    text = json.dumps(record, indent=2)
    print(text)
    if args.out:
        Path(args.out).write_text(text + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
