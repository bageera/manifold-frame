#!/usr/bin/env python3
"""W8-B1: vendor the LOCK-PINNED manifold-codec engine source.

Replaces the CI `cargo search | head -1` gamble: the exact engine version
comes from the committed Cargo.lock (the same resolution the local and
release builds use), the .crate is downloaded from crates.io, its sha256 is
verified BOTH against the API-published checksum and the transferred bytes,
and the source tree extracts into --into (CI: ../manifold).

Never clobbers a real git clone (a local dev sibling) without --force.
Prints one provenance JSON line on stdout: version, sha256, source, time.
"""
from __future__ import annotations  # py3.9 compat: defer the `str | None` annotations

import argparse
import datetime
import hashlib
import json
import re
import shutil
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

LOCK = "Cargo.lock"
CRATE = "manifold-codec"


def pinned_version() -> tuple[str, str | None]:
    """(version, requirement) from the lockfile and the manifest.

    The lock's resolved version is the deterministic anchor; the manifest's
    caret requirement must admit it (a stale lock vs manifest is a build
    provenance bug, caught here rather than downstream).
    """
    try:
        import tomllib

        with open(LOCK, "rb") as f:
            lock = tomllib.load(f)
        pkgs = [
            p["version"] for p in lock.get("package", []) if p.get("name") == CRATE
        ]
        if len(pkgs) != 1:
            sys.exit(f"expected exactly one {CRATE} entry in {LOCK}, got {len(pkgs)}")
        ver = pkgs[0]
    except ImportError:  # python < 3.11: regex-fallback tomllib-free parse
        text = Path(LOCK).read_text()
        blocks = text.split("[[package]]")
        hits = [
            b
            for b in blocks
            if re.search(r'^name = "' + CRATE + r'"$', b, re.M)
        ]
        if len(hits) != 1:
            sys.exit(f"expected exactly one {CRATE} entry in {LOCK}, got {len(hits)}")
        m = re.search(r'^version = "([^"]+)"$', hits[0], re.M)
        if not m:
            sys.exit(f"{CRATE} block carries no version in {LOCK}")
        ver = m.group(1)

    req_m = re.search(r"^manifold-codec = .*version = .(\d+\.\d+)", Path("Cargo.toml").read_text(), re.M)
    if not req_m:
        sys.exit("Cargo.toml carries no manifold-codec version requirement")
    lo_major, lo_minor = (int(x) for x in req_m.group(1).split("."))
    if ver.startswith(f"{lo_major}.") and int(ver.split(".")[1]) >= lo_minor:
        return ver, req_m.group(1)
    sys.exit(f"lock pins {CRATE} {ver}, outside the manifest caret {req_m.group(1)}")


def api_checksum(version: str) -> tuple[str, str]:
    """(checksum, url) from the crates.io API (the provenance anchor)."""
    url = f"https://crates.io/api/v1/crates/{CRATE}/{version}"
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "mnfx-vendor"}), timeout=60) as r:
        v = json.load(r)["version"]
    if v.get("yanked"):
        sys.exit(f"{CRATE} {version} is yanked — refuse to vendor")
    return v["checksum"], url


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--into", default="../manifold", help="extract target dir")
    ap.add_argument("--force", action="store_true", help="replace even a git clone")
    args = ap.parse_args()

    ver, req = pinned_version()
    print(f"lock pins {CRATE} {ver} (manifest requirement {req}, caret)", file=sys.stderr)

    want, api_url = api_checksum(ver)
    dl = f"{api_url}/download"
    with tempfile.TemporaryDirectory() as td:
        crate_file = Path(td) / f"{CRATE}-{ver}.crate"
        with urllib.request.urlopen(
            urllib.request.Request(dl, headers={"User-Agent": "mnfx-vendor"}), timeout=300
        ) as r:
            crate_file.write_bytes(r.read())
        got = hashlib.sha256(crate_file.read_bytes()).hexdigest()
        if got != want:
            sys.exit(f"sha256 MISMATCH: {got} != published {want} — provenance broken")
        with tarfile.open(crate_file) as t:
            try:
                t.extractall(td, filter="data")  # 3.12+: refuse unsafe members
            except TypeError:  # 3.9: no filter kwarg
                t.extractall(td)
        src_dir = Path(td) / f"{CRATE}-{ver}"

        into = Path(args.into)
        if into.exists():
            if (into / ".git").is_dir() and not args.force:
                sys.exit(f"refusing to clobber the git clone at {into} (use --force)")
            shutil.rmtree(into)
        shutil.move(str(src_dir), str(into))

    provenance = {
        "engine_version": ver,
        "engine_sha256": got,
        "engine_api": api_url,
        "download": dl,
        "extracted_to": str(Path(args.into).resolve()),
        "verified_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    }
    print(json.dumps(provenance, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
