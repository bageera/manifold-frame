#!/bin/bash
# W6-E: ImageMagick bridge gate over the published conformance fixtures.
# Bridge argv (the W6-C shape): <rgba8.mnfx> <gray16.mnfx> <rgb16.mnfx>.
# Exact-sample legs (import -> export -> memcmp), a real thumbnail op,
# PNG export cross-checked at the PIL byte level against the PAM source,
# and the hostile leg (rc == 1, never a signal).
set -e
cd "$(dirname "$0")/../.."
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
export RUSTUP_HOME="$HOME/.rustup"
D=/tmp/w6e/fixtures
MNFX_WRITE_FIXTURES=$D cargo test -q -p manifold-frame --test w6e_conformance fixtures_write >/dev/null
cargo build -q -p manifold-frame-capi --release
cc -Wall -Wextra -Werror -I capi/include integrations/magick/magickload_mnfx.c \
   $(pkg-config --cflags MagickCore) \
   -L target/release -lmanifold_frame_capi -Wl,-rpath,"$PWD/target/release" \
   $(pkg-config --libs MagickCore) -o /tmp/w6e/magickload

# Hostile leg FIRST (the W6-C crash-polarity property, now on the
# fixture corpus: rc must be 1 = controlled, never a signal).
cp $D/rgba8.mnfx /tmp/w6e/corrupted.mnfx
python3 - <<'EOF'
b = bytearray(open("/tmp/w6e/corrupted.mnfx", "rb").read())
b[31] ^= 0xFF
open("/tmp/w6e/corrupted.mnfx", "wb").write(b)
EOF
set +e
/tmp/w6e/magickload /tmp/w6e/corrupted.mnfx $D/gray16.mnfx $D/rgb16.mnfx >/dev/null 2>&1
RC=$?
set -e
echo "hostile rc: $RC (want 1)"
[ "$RC" -eq 1 ] || { echo "FAIL: unexpected rc $RC"; exit 1; }

# The three exact-sample legs + thumbnail + exports.
/tmp/w6e/magickload $D/rgba8.mnfx $D/gray16.mnfx $D/rgb16.mnfx

# PIL-level cross-check: IM's exported RGB8-container pixels == the
# canonical .plane truth (frombytes; no image-format dependency).
python3 - "$D" <<'EOF'
import sys
from PIL import Image
src = Image.frombytes("RGBA", (8, 6), open(sys.argv[1] + "/rgba8.plane", "rb").read())
out = Image.open("/tmp/w6c_rgba8_out.png").convert("RGBA")
assert src.tobytes() == out.tobytes(), "PIL equality vs plane truth failed"
print("pillow: IM export == canonical plane (channels + alpha)", len(src.tobytes()), "B")
EOF
echo "magick gate: PASS"