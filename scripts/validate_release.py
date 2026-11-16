#!/usr/bin/env python3
"""W8-B3: validate the RELEASE ARCHIVE as a consumer would (test the
shipped files, not the build tree).

Extracts to a clean temp dir, re-verifies the internal SHA256SUMS,
checks the packaged binaries' architecture against the archive's target
label, links a real C consumer (integrations/c-abi/abidec.c) against
BOTH the packaged dynamic and static libraries, decodes all six
published conformance fixtures through the packaged library, runs the
packaged `mnfx` CLI (encode -> decode -> verify) from OUTSIDE the
build/install tree, exercises a hostile container (controlled error,
never a crash), and cross-checks the packaged header's ABI symbols
against the library's exports. Exits 1 at the first contract break;
prints one PASS line per leg.
"""
from __future__ import annotations

import argparse
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
WANTED_SYMBOLS = ("mnfx_decode", "mnfx_rewrite_chunk")


def die(msg: str) -> None:
    print(f"VALIDATE FAIL: {msg}")
    raise SystemExit(1)


def run(cmd: list, cwd: Path | None = None, want_rc: int | None = 0):
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if want_rc is not None and p.returncode != want_rc:
        die(f"{' '.join(cmd[:3])}... rc={p.returncode}: {p.stderr[-300:]}")
    return p


def png_chunk(tag: bytes, body: bytes) -> bytes:
    return struct.pack(">I", len(body)) + tag + body + struct.pack(">I", zlib.crc32(tag + body))


def synthetic_png(path: Path) -> None:
    w = h = 8
    raw = b"".join(
        b"\x00" + b"".join(bytes(((x * 37) % 256, (y * 53) % 256, ((x + y) * 11) % 256)) for x in range(w))
        for y in range(h)
    )
    png = (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + png_chunk(b"IDAT", zlib.compress(raw))
        + png_chunk(b"IEND", b"")
    )
    path.write_bytes(png)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--archive", required=True)
    ap.add_argument("--fixtures-dir", required=True, help="regenerated w6e fixtures (.mnfx + .plane)")
    ap.add_argument("--expect-arch", required=True, choices=["arm64", "x86_64"])
    args = ap.parse_args()

    archive = Path(args.archive).resolve()
    fixtures = Path(args.fixtures_dir)
    if not archive.is_file():
        die(f"missing archive {archive}")
    for fmt in ("rgb8", "rgba8", "gray8", "graya8", "rgb16", "gray16"):
        if not (fixtures / f"{fmt}.mnfx").is_file() or not (fixtures / f"{fmt}.plane").is_file():
            die(f"fixture set incomplete at {fixtures} (missing {fmt})")

    with tempfile.TemporaryDirectory(prefix="mnfx-validate-") as td:
        root = Path(td)
        run(["tar", "-xzf", str(archive)], cwd=root)
        tree = root / archive.name[: -len(".tar.gz")]
        if not tree.is_dir():
            die(f"archive did not extract to {tree.name}")
        root_tree = tree

        # Archive integrity: the internal checksums (macOS runner: shasum
        # when present; the fallback runs only if the first binary is
        # missing — a real mismatch fails BOTH so it cannot hide).
        run(["sh", "-c", f"""cd '{tree}' && (shasum -a 256 -c SHA256SUMS --status 2>/dev/null || sha256sum -c SHA256SUMS --status)"""])
        print("PASS archive-integrity (internal SHA256SUMS)")

        # Architecture: the label vs the actual binaries. The .a is an
        # `ar` container ('file' cannot see through it): its architecture
        # is proven by the static-linkage leg below (a wrong-arch .a
        # cannot link), so only the dylib and the CLI are probed here.
        is_mac = "-apple-darwin" in tree.name
        lib = tree / "lib" / f"libmanifold_frame_capi.{'dylib' if is_mac else 'so'}"
        st = tree / "lib" / "libmanifold_frame_capi.a"
        mnfx = tree / "bin" / "mnfx"
        for bin_path in (lib, mnfx):
            f = run(["file", str(bin_path)]).stdout
            want = "arm64" if args.expect_arch == "arm64" else "x86_64"
            if want not in f:
                die(f"{bin_path.name}: arch mismatch (file says: {f.strip()})")
        print(f"PASS architecture ({args.expect_arch} matches dylib/static/CLI)")

        # Header compatibility: packaged header's ABI symbols vs exports.
        header = (tree / "include" / "mnfx.h").read_text()
        declared = [s for s in WANTED_SYMBOLS if s in header]
        exports = run(
            ["nm", "-D", "--defined-only", str(lib)] if not is_mac else ["nm", "-gU", str(lib)]
        ).stdout
        missing = [s for s in WANTED_SYMBOLS if s in declared and s not in exports]
        if missing:
            die(f"header declares {missing} absent from the library exports")
        print("PASS header-compatibility (mnfx.h symbols exported)")

        # Dynamic linkage: a real C consumer against the PACKAGED dylib,
        # from a neutral rpath prefix.
        dyn_bin = root / "abidec_dyn"
        cc_dyn = ["cc", "-Wall", "-Wextra", "-Werror",
                  "-I", str(tree / "include"), str(REPO / "integrations" / "c-abi" / "abidec.c"),
                  "-L", str(tree / "lib"), "-lmanifold_frame_capi",
                  "-Wl,-rpath," + str(tree / "lib"), "-o", str(dyn_bin)]
        run(cc_dyn)
        print("PASS dynamic-linkage (C consumer links the packaged dylib)")

        # Static linkage: same consumer, packaged .a only.
        static_bin = root / "abidec_static"
        cc_static = ["cc", "-Wall", "-Wextra", "-Werror",
                     "-I", str(tree / "include"), str(REPO / "integrations" / "c-abi" / "abidec.c"),
                     str(st), "-o", str(static_bin)]
        if not is_mac:
            cc_static += ["-lpthread", "-ldl", "-lm"]
        run(cc_static)
        print("PASS static-linkage (C consumer links the packaged .a)")

        # Pixel correctness: all six formats, exact, through the dyn lib.
        for fmt in ("rgb8", "rgba8", "gray8", "graya8", "rgb16", "gray16"):
            run([str(dyn_bin), str(fixtures / f"{fmt}.mnfx"), str(fixtures / f"{fmt}.plane"), "3"])
        print("PASS pixel-correctness (6/6 formats reconstruct exactly)")

        # Static consumer proves the same (one representative leg).
        run([str(static_bin), str(fixtures / "rgb8.mnfx"), str(fixtures / "rgb8.plane"), "3"])
        print("PASS static pixel-correctness (rgb8 via the .a)")

        # Hostile input: corrupted container, controlled error only.
        corrupted = root / "corrupted.mnfx"
        b = bytearray((fixtures / "rgb8.mnfx").read_bytes())
        b[31] ^= 0xFF
        corrupted.write_bytes(bytes(b))
        h = run([str(dyn_bin), str(corrupted), str(fixtures / "rgb8.plane"), "1"], want_rc=None)
        if not (1 <= h.returncode <= 2):
            die(f"hostile leg rc={h.returncode} (controlled errors are rc 1..2, never a signal)")
        print(f"PASS hostile-input (rc={h.returncode}, controlled)")

        # CLI execution + pixel correctness, from OUTSIDE the tree, on a
        # synthetic PNG the packaged binary itself wrote/verified.
        work = root / "cli"
        work.mkdir()
        synthetic_png(work / "input.png")
        enc = run([str(mnfx), "encode", str(work / "input.png"), "-o", str(work / "out.mnfx"),
                   "--mnf8=maximum"], cwd=work)
        if " FAIL" in enc.stdout:
            die(f"CLI encode leg failed: {enc.stdout[-200:]}")
        run([str(mnfx), "decode", str(work / "out.mnfx"), "-o", str(work / "back.png")], cwd=work)
        run([str(mnfx), "verify", str(work / "input.png"), str(work / "out.mnfx")], cwd=work)
        print("PASS cli-execution (packaged mnfx encodes/decodes/verifies exact; CWD outside the tree)")

        # Runtime dependencies: recorded in provenance (otool -L / ldd),
        # so the consumer documentation is testable: the field must exist
        # and name the engine's runtime surface.
        prov = (tree / "provenance.json")
        if not prov.is_file():
            die("provenance.json missing from the archive")
        p = __import__("json").loads(prov.read_text())
        for field in ("package_version", "wire_version", "git_commit", "git_tag", "target",
                      "runner_architecture", "rustc_version", "cargo_version", "engine_version",
                      "engine_checksum", "cargo_lock_sha256", "build_profile", "features",
                      "runtime_shared_libraries"):
            if field not in p:
                die(f"provenance.json missing required field: {field}")
        if len(p["runtime_shared_libraries"]) == 0:
            die("runtime_shared_libraries empty — shared-library documentation missing")
        print("PASS runtime-dependencies (otool -L / ldd lists recorded in provenance.json)")

    print("VALIDATE: ALL LEGS PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
