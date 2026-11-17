#!/bin/bash
# W6-E: libvips loader gate over the published conformance fixtures.
# The bridge decodes MNFX via the shipped C ABI and asserts in-pipeline
# pixel equality vs the fixture's PAM source, then runs a thumbnail op.
#
# W8-B4 consumer mode: MNFX_CONSUMER_ROOT=<extracted archive dir> links
# the bridge against the DOWNLOADED library only — no cargo, no engine
# tree, no build tree. In that mode fixtures come from W6E_FIXTURES
# (default /tmp/w6e/fixtures); the producer regenerates them.
set -e
cd "$(dirname "$0")/../.."
if [ -n "${MNFX_CONSUMER_ROOT:-}" ]; then
  [ -d "$MNFX_CONSUMER_ROOT/lib" ] || { echo "MNFX_CONSUMER_ROOT has no lib/"; exit 2; }
  D="${W6E_FIXTURES:-/tmp/w6e/fixtures}"
  INCL="$MNFX_CONSUMER_ROOT/include"
  LIBDIR="$MNFX_CONSUMER_ROOT/lib"
else
  command -v cargo >/dev/null 2>&1 || \
    export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
  D=/tmp/w6e/fixtures
  MNFX_WRITE_FIXTURES=$D cargo test -q -p manifold-frame --test w6e_conformance fixtures_write >/dev/null
  cargo build -q -p manifold-frame-capi --release
  INCL=capi/include
  LIBDIR="$PWD/target/release"
fi
mkdir -p /tmp/w6e   # consumer jobs have no producer fixture dump; the bridge output dir must exist
cc -Wall -Wextra -Werror -I "$INCL" integrations/vips/vipsload_mnfx.c \
   $(pkg-config --cflags glib-2.0 gobject-2.0 vips) \
   -L "$LIBDIR" -lmanifold_frame_capi -Wl,-rpath,"$LIBDIR" \
   $(pkg-config --libs gobject-2.0 glib-2.0 vips) -o /tmp/w6e/vipsload
# The W6-B 8-bit gate: rgb8 + rgba8 fixtures (16-bit is documented out).
for f in rgb8 rgba8; do /tmp/w6e/vipsload $D/$f.mnfx $D/$f.pam; done
cp $D/rgb8.mnfx /tmp/w6e/corrupted.mnfx
python3 - <<'EOF'
b = bytearray(open("/tmp/w6e/corrupted.mnfx", "rb").read())
b[31] ^= 0xFF
open("/tmp/w6e/corrupted.mnfx", "wb").write(b)
EOF
set +e
/tmp/w6e/vipsload /tmp/w6e/corrupted.mnfx $D/rgb8.pam >/dev/null 2>&1
RC=$?
set -e
echo "hostile rc: $RC (want 1 = controlled, never a signal)"
[ "$RC" -eq 0 ] && { echo "FAIL: corruption accepted"; exit 1; }
[ "$RC" -gt 1 ] && { echo "FAIL: crashed (rc=$RC)"; exit 1; }
echo "vips gate: PASS"
