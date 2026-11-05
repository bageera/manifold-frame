#!/bin/bash
# W6-E: libvips loader gate over the published conformance fixtures.
# The bridge decodes MNFX via the shipped C ABI and asserts in-pipeline
# pixel equality vs the fixture's PAM source, then runs a thumbnail op.
set -e
cd "$(dirname "$0")/../.."
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
export RUSTUP_HOME="$HOME/.rustup"
D=/tmp/w6e/fixtures
MNFX_WRITE_FIXTURES=$D cargo test -q -p manifold-frame --test w6e_conformance fixtures_write >/dev/null
cargo build -q -p manifold-frame-capi --release
cc -Wall -Wextra -Werror -I capi/include integrations/vips/vipsload_mnfx.c \
   $(pkg-config --cflags glib-2.0 gobject-2.0 vips) \
   -L target/release -lmanifold_frame_capi -Wl,-rpath,"$PWD/target/release" \
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