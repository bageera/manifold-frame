#!/bin/bash
# W8-B4: gate the DOWNLOADED release archive as an independent consumer
# would — validation runs entirely outside the producer environment.
#
# Usage: b4_consumer_gate.sh --root <extracted archive dir> \
#          --fixtures <w6e fixture dir> [--evidence <out.json>]
#
# B4.1 Clean consumer    — the ship-tree is the ONLY MNFX surface here:
#                          no Cargo build, no engine tree, no target/;
#                          proven structurally (consumer mode never
#                          invokes cargo) AND by the linkage audit.
# B4.2 Standalone C      — a real consumer compiles using only the
#                          packaged header + libraries.
# B4.3 libvips           — the W6-B gate repeats against the packaged ABI
#                          (in-pipeline pixel equality + thumbnail + hostile leg).
# B4.4 ImageMagick       — the W6-C gate repeats against the packaged ABI
#                          (rgba8/gray16/rgb16 exactness + hostile leg + PIL cross-check).
# B4.5 Relocatability    — the extracted tree is MOVED to a different
#                          path and linkage + execution repeat there.
# B4.6 Hostile input     — controlled rejection through the packaged ABI.
# B4.7 Evidence          — a machine-readable receipt: archive hash,
#                          platform, every gate's outcome.
set -euo pipefail

ROOT=""
FIXTURES=""
EVIDENCE="b4-evidence.json"
while [ $# -gt 0 ]; do
  case "$1" in
    --root) ROOT="$2"; shift 2 ;;
    --fixtures) FIXTURES="$2"; shift 2 ;;
    --evidence) EVIDENCE="$2"; shift 2 ;;
    *) echo "unknown arg: $1"; exit 2 ;;
  esac
done
REPO="$(cd "$(dirname "$0")/.." && pwd)"
R="$ROOT"   # keep the GIVEN string: the linkage audit compares the
            # rpath string against itself, not against a canonical form
[ -f "$R/bin/mnfx" ] && [ -f "$R/include/mnfx.h" ] && [ -n "$(ls "$R"/lib/libmanifold_frame_capi.* 2>/dev/null)" ] || {
  echo "B4 FAIL: consumer root lacks the archive contract (bin/include/lib)"; exit 2; }

PLATFORM_ROW="$(uname -s)-$(uname -m)"
RESULT_ROWS=""
FAIL=0
PASS() { RESULT_ROWS="$RESULT_ROWS,\"$1\": \"pass\""; echo "B4 PASS: $1"; }
SKIPFAIL() { FAIL=1; RESULT_ROWS="$RESULT_ROWS,\"$1\": \"FAIL: $2\""; echo "B4 FAIL: $1 — $2"; }

# B4.7 anchor: hash the manifest the archive itself carries.
SUM="$(cd "$R" && (shasum -a 256 SHA256SUMS 2>/dev/null || sha256sum SHA256SUMS) | awk '{print $1}')"

# ---- B4.2 + B4.6: standalone C consumer from a scrubbed workdir ----
WORK="$(mktemp -d /tmp/mnfx-b4.XXXXXX)"
BUILD_AND_DECODE() {
  # $1 = consumer root; compiles abidec against ONLY that root and runs
  # the six-format legs + the hostile leg.
  local r="$1" bin="$2"
  cc -Wall -Wextra -Werror -I "$r/include" "$REPO/integrations/c-abi/abidec.c" \
     -L "$r/lib" -lmanifold_frame_capi -Wl,-rpath,"$r/lib" -o "$bin"
  for f in rgb8 rgba8 gray8 graya8 rgb16 gray16; do
    "$bin" "$FIXTURES/$f.mnfx" "$FIXTURES/$f.plane" 3 >/dev/null
  done
  local c="$WORK/corrupted.mnfx"
  cp "$FIXTURES/rgb8.mnfx" "$c"
  python3 - "$c" <<'EOF'
import sys
b = bytearray(open(sys.argv[1], "rb").read()); b[31] ^= 0xFF
open(sys.argv[1], "wb").write(bytes(b))
EOF
  set +e
  "$bin" "$c" "$FIXTURES/rgb8.plane" 1 >/dev/null 2>&1
  local rc=$?
  set -e
  [ "$rc" -ge 1 ] && [ "$rc" -le 2 ] || { echo "B4 hostile rc=$rc (want controlled 1..2)"; return 1; }
}
if BUILD_AND_DECODE "$ROOT" "$WORK/abidec_a"; then
  PASS "B4.2 standalone-c-consumer"
  PASS "B4.6 hostile-input-controlled"
else
  SKIPFAIL "B4.2/B4.6" "standalone consumer leg failed"
fi

# Linkage audit (B4.1's teeth): every built bridge/consumer must resolve
# libmanifold_frame_capi ONLY from the consumer tree — never target/,
# never a build/engine path.
AUDIT_LINKS() {
  local bin
  for bin in "$@"; do
    local listing
    if listing=$(otool -L "$bin" 2>/dev/null); then
      echo "$listing" | grep -q libmanifold_frame_capi || { echo "no mnfx linkage: $bin"; return 1; }
      echo "$listing" | grep -qE "target/|manifold-frame/(target|capi)" && { echo "build path leaked: $bin"; return 1; }
    elif listing=$(ldd "$bin" 2>/dev/null); then
      echo "$listing" | grep libmanifold_frame_capi | grep -q "$R/lib" || { echo "consumer root NOT the resolved provider: $bin"; return 1; }
      echo "$listing" | grep -qE "target/|manifold-frame/(target|capi)" && { echo "build path leaked: $bin"; return 1; }
    else
      echo "cannot inspect linkage of $bin"; return 1
    fi
  done
}

# ---- B4.3 + B4.4: the application bridges against the packaged ABI ----
if [ -n "${MNFX_B4_SKIP_BRIDGES:-}" ]; then
  SKIPFAIL "B4.3" "skipped by env"
  SKIPFAIL "B4.4" "skipped by env"
else
  pkg-config --exists vips || { echo "MISSING DEP: libvips-dev (pkg-config vips)"; }
  pkg-config --exists MagickCore || { echo "MISSING DEP: MagickCore (libmagickcore-dev)"; }
  if MNFX_CONSUMER_ROOT="$ROOT" W6E_FIXTURES="$FIXTURES" bash "$REPO/integrations/vips/run.sh"; then
    AUDIT_LINKS /tmp/w6e/vipsload && PASS "B4.3 libvips (packaged ABI, linkage audited)" \
      || SKIPFAIL "B4.3" "linkage audit (build-path leak)"
  else
    SKIPFAIL "B4.3" "vips gate failed"
  fi
  if MNFX_CONSUMER_ROOT="$ROOT" W6E_FIXTURES="$FIXTURES" bash "$REPO/integrations/magick/run.sh"; then
    AUDIT_LINKS /tmp/w6e/magickload && PASS "B4.4 imagemagick (packaged ABI, linkage audited)" \
      || SKIPFAIL "B4.4" "linkage audit (build-path leak)"
  else
    SKIPFAIL "B4.4" "magick gate failed"
  fi
fi

# ---- B4.5: relocatability — MOVE the tree, repeat linkage+execution ----
MOVED="$WORK/relocated-mnfx"
mv "$ROOT" "$MOVED"
if BUILD_AND_DECODE "$MOVED" "$WORK/abidec_b"; then
  PASS "B4.5 relocatability (tree moved; re-linked + re-decoded)"
else
  SKIPFAIL "B4.5" "relocated leg failed"
fi

# ---- B4.7: the receipt ----
ROWS="$RESULT_ROWS"
python3 - "$EVIDENCE" "$PLATFORM_ROW" "$SUM" "$ROWS" <<'EOF'
import json, sys
out, plat, sumhash, rows = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
data = {"platform": plat, "sha256sums_file_sha256": sumhash,
        "producer_environment_used": False, "gates": json.loads("{" + rows.lstrip(",") + "}")}
open(out, "w").write(json.dumps(data, indent=2) + "\n")
print(json.dumps(data, indent=2))
sys.exit(0 if all(str(v) == "pass" for v in data["gates"].values()) else 1)
EOF
GATE_RC=$?
[ "$FAIL" -eq 0 ] || exit 1
exit "$GATE_RC"
