#!/usr/bin/env python3
"""W8-B3: assemble the per-target release archive (the distribution contract).

Native-built binaries in; a self-verifying install tree out, layout
identical across platforms except the dynamic-library extension:

  manifold-frame-v<ver>-<target>/
    bin/mnfx
    include/mnfx.h
    lib/libmanifold_frame_capi.(dylib|so) + libmanifold_frame_capi.a
    share/mnfx/{mnfx-grammar.md, capi-README.md}
    LICENSE, README.md
    provenance.json   (W8-B3 schema on the B1 provenance machinery — the
                       SAME locked-engine evidence, never a second resolver)
    SHA256SUMS        (over every packaged file)

Produces manifold-frame-v<ver>-<target>.tar.gz. macOS dylibs get
install_name @rpath so the archive links from any consumer prefix; the
recorded shared-library lists (otool -L / ldd) document the runtime
surface. The git tag and the runner architecture are recorded, never
assumed: a native build means target == runner arch is CHECKED, not
hoped for.
"""
from __future__ import annotations

import argparse
import datetime
import gzip
import hashlib
import io
import json
import platform
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import write_provenance as prov  # B1 helpers: engine pin, lock hash, toolchain

REPO = Path(__file__).resolve().parent.parent
WIRE_VERSION = 1


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def dylib_ext(target: str) -> str:
    if target.endswith("apple-darwin"):
        return "dylib"
    if "linux" in target:
        return "so"
    sys.exit(f"unsupported distribution target: {target}")


def arch_of(target: str) -> str:
    if target.startswith("x86_64"):
        return "x86_64"
    if target.startswith("aarch64"):
        return "arm64"
    sys.exit(f"unsupported distribution arch: {target}")


def native_build_check(target: str) -> None:
    """A distribution archive is produced by a NATIVE build only: the
    runner architecture must equal the target's — recorded separately
    in provenance, enforced here (never 'hope the labels line up')."""
    host = platform.machine()
    host_arch = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x86_64", "amd64": "x86_64"}[host]
    if host_arch != arch_of(target):
        sys.exit(f"runner arch {host} != target {target}: cross-compiled archives are not produced")


def build_cmd() -> None:
    subprocess.run(
        ["cargo", "build", "--locked", "--release", "-p", "manifold-frame-capi"],
        cwd=REPO,
        check=True,
    )
    subprocess.run(
        ["cargo", "build", "--locked", "--release", "-p", "manifold-frame", "--features", "cli", "--bins"],
        cwd=REPO,
        check=True,
    )


def stage_tree(target: str, version: str) -> tuple[Path, Path]:
    name = f"manifold-frame-v{version}-{target}"
    stage = REPO / "dist" / name
    if stage.exists():
        shutil.rmtree(stage)
    target_out = REPO / "target" / "release"
    (stage / "bin").mkdir(parents=True)
    (stage / "include").mkdir()
    (stage / "lib").mkdir()
    (stage / "share" / "mnfx").mkdir(parents=True)

    shutil.copy2(target_out / "mnfx", stage / "bin" / "mnfx")
    shutil.copy2(REPO / "capi" / "include" / "mnfx.h", stage / "include" / "mnfx.h")
    ext = dylib_ext(target)
    dy = target_out / f"libmanifold_frame_capi.{ext}"
    st = target_out / "libmanifold_frame_capi.a"
    if not dy.exists() or not st.exists():
        sys.exit(f"expected built libraries missing under {target_out}: {dy.name}, {st.name}")
    shutil.copy2(dy, stage / "lib" / dy.name)
    shutil.copy2(st, stage / "lib" / st.name)
    shutil.copy2(REPO / "docs" / "mnfx-grammar.md", stage / "share" / "mnfx" / "mnfx-grammar.md")
    shutil.copy2(REPO / "capi" / "README.md", stage / "share" / "mnfx" / "capi-README.md")
    shutil.copy2(REPO / "LICENSE", stage / "LICENSE")
    shutil.copy2(REPO / "README.md", stage / "README.md")
    return stage, stage.parent


def fix_macos_install_name(dylib: Path) -> str | None:
    """Consumers link via -Wl,-rpath,<libdir>; the id must be @rpath-based
    or the archive links only inside its build tree. Recorded as a
    provenance field, never assumed."""
    if platform.system() != "Darwin":
        return None
    want = f"@rpath/{dylib.name}"
    subprocess.run(["install_name_tool", "-id", want, str(dylib)], check=True)
    return want


def shared_libraries(paths: list[Path]) -> list[str]:
    """The runtime-deps documentation (layout stays fixed: this rides in
    provenance, not an extra file)."""
    lines: list[str] = []
    tool = (
        ["otool", "-L"]
        if platform.system() == "Darwin"
        else ["ldd", ""]
    )  # ldd takes the path as the last argv
    for p in paths:
        out = subprocess.run(tool[:-1] + [str(p)], capture_output=True, text=True, check=False)
        lines.append(f"# {p.name}")
        lines.extend(l.rstrip() for l in out.stdout.splitlines())
    return lines


def build_provenance(target: str, git_tag: str, version: str, install_name: str | None, stage: Path) -> dict:
    base = json.loads(subprocess.run(  # B1 evidence, invoked not duplicated
        ["python3", str(Path(__file__).parent / "write_provenance.py")],
        capture_output=True, text=True, check=True, cwd=REPO,
    ).stdout)
    return {
        "package_version": version,
        "wire_version": WIRE_VERSION,
        "git_commit": base["git"]["commit"],
        "git_tag": git_tag,
        "target": target,
        "runner_architecture": platform.machine(),
        "rustc_version": base["toolchain"]["rustc"],
        "cargo_version": base["toolchain"]["cargo"],
        "engine_version": base["engine"]["pinned_version"],
        "engine_checksum": base["engine"]["published_sha256"],
        "cargo_lock_sha256": base["lockfile"]["sha256"],
        "build_profile": "release",
        "features": ["cli"],
        # --- B1-reused base fields (same evidence, explicit) ---
        "generated_at": base["generated_at"],
        "crate": base["crate"],
        "engine_source": base["engine"]["source"],
        # --- B3 additions ---
        "macos_install_name": install_name,
        "runtime_shared_libraries": shared_libraries(
            [stage / "lib" / p.name for p in (stage / "lib").iterdir()] + [stage / "bin" / "mnfx"]
        ),
    }


def write_sha256sums(stage: Path) -> None:
    lines = []
    for p in sorted(stage.rglob("*")):
        if p.is_file() and p.name != "SHA256SUMS":
            lines.append(f"{sha256_file(p)}  {p.relative_to(stage).as_posix()}")
    (stage / "SHA256SUMS").write_text("\n".join(lines) + "\n")


def tar_gz(stage: Path, out_dir: Path) -> Path:
    name = stage.name
    out = out_dir / f"{name}.tar.gz"
    out.parent.mkdir(parents=True, exist_ok=True)
    with gzip.GzipFile(filename=str(out), mode="wb", mtime=0) as gz:  # deterministic bytes
        with tarfile.open(fileobj=gz, mode="w") as t:
            try:
                t.add(stage, arcname=name, filter=None)  # 3.12+: explicit, no unsafe-member filter
            except TypeError:  # 3.9: no filter kwarg
                t.add(stage, arcname=name)
    return out


def read_version() -> str:
    try:
        import tomllib

        return tomllib.loads((REPO / "Cargo.toml").read_text())["package"]["version"]
    except ImportError:  # 3.9: section parse via the B1 helpers
        v = prov.scalar("version", prov.toml_section((REPO / "Cargo.toml").read_text(), "package"))
        if not v:
            sys.exit("cannot read the package version from Cargo.toml")
        return v


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", required=True, help="Rust target triple this runner ACTUALLY is")
    ap.add_argument("--git-tag", default="", help="release tag (empty for dry runs)")
    ap.add_argument("--out-dir", default="dist")
    args = ap.parse_args()

    version = read_version()
    native_build_check(args.target)
    build_cmd()

    stage, out_dir = stage_tree(args.target, version)
    install_name = fix_macos_install_name(stage / "lib" / f"libmanifold_frame_capi.{dylib_ext(args.target)}")
    (stage / "provenance.json").write_text(
        json.dumps(build_provenance(args.target, args.git_tag, version, install_name, stage), indent=2) + "\n"
    )
    write_sha256sums(stage)
    archive = tar_gz(stage, REPO / args.out_dir)

    print(json.dumps({
        "archive": str(archive),
        "archive_sha256": sha256_file(archive),
        "target": args.target,
        "package_version": version,
        "git_tag": args.git_tag,
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
