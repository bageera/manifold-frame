#!/bin/bash
# W6-E: the C-ABI decode gate over the published conformance fixtures.
# (fixtures regenerating + all six formats byte-exact + one hostile leg)
set -e
cd "$(dirname "$0")/../.."
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
export RUSTUP_HOME="$HOME/.rustup"
D=/tmp/w6e/fixtures
MNFX_WRITE_FIXTURES=$D cargo test -q -p manifold-frame --test w6e_conformance fixtures_write >/dev/null
cargo build -q -p manifold-frame-capi --release
cc -Wall -Wextra -Werror -I capi/include integrations/c-abi/abidec.c \
   -L target/release -lmanifold_frame_capi -Wl,-rpath,"$PWD/target/release" \
   -o /tmp/w6e/abidec
for f in rgb8 rgba8 gray8 graya8 rgb16 gray16; do
    /tmp/w6e/abidec $D/$f.mnfx $D/$f.plane 3
done
cp $D/rgb8.mnfx /tmp/w6e/corrupted.mnfx
python3 - <<'EOF'
b = bytearray(open("/tmp/w6e/corrupted.mnfx", "rb").read())
b[31] ^= 0xFF  # corrupt byte 31 of the chunk header (probed earlier: every flip rejects)
open("/tmp/w6e/corrupted.mnfx", "wb").write(b)
EOF
set +e
/tmp/w6e/abidec /tmp/w6e/corrupted.mnfx $D/rgb8.plane 1
RC=$?
set -e
echo "hostile rc: $RC (controlled error, no crash: rc in 1..2, never a signal)"
[ "$RC" -eq 0 ] && { echo "FAIL: corruption accepted"; exit 1; }
[ "$RC" -gt 2 ] && { echo "FAIL: crashed (rc=$RC)"; exit 1; }
echo "c-abi gate: PASS"