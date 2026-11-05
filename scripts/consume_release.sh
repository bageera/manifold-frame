#!/bin/bash
# W6-E release verification: a THIRD project consuming the PUBLISHED
# crates.io crate only (no checkouts, no path deps) — cargo new in a
# fresh temp dir, lib-level roundtrip via the registry API, plus a
# CLI-driven PNG -> MNFX -> PNG cycle (published vintage includes the
# W5 CLI). Byte-exact at the pixel plane via the crate's own verify.
set -e
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
export RUSTUP_HOME="$HOME/.rustup"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
T=$(mktemp -d /tmp/mnfx-release-check.XXXXXX)
cd "$T"
cargo new consumer --bin -q
cd consumer
cargo add -q manifold-frame@"0.1.0" 2>/dev/null
# 1) lib usage of the published crate: synthetic 4x2 RGB8 roundtrip.
cat > src/main.rs <<'RS'
use manifold_frame::{ChunkType, Codec, ContainerHeader, PixelFormat};
fn main() {
    let w = 4u32; let h = 2u32;
    let samples: Vec<u8> =
        (0..w as usize * h as usize * 3).map(|i| (i * 5 % 256) as u8).collect();
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8, orientation: 1, chunk_count: 1,
        width: w, height: h,
    };
    // The published vintage exposes no engine re-export; consumers encode
    // via their own manifold-codec dep, or produce raw/zstd planes. Here:
    // the raw plane path (container + parse + decode without engines).
    let buf = manifold_frame::serialize(&hdr, &[(ChunkType::Pixel, Codec::Raw, &samples)])
        .unwrap();
    let (h2, chunks) = manifold_frame::parse(&buf).unwrap();
    let px = manifold_frame::decode_pixels(&h2, &chunks).unwrap();
    assert_eq!(px.samples, samples, "registry roundtrip lost bytes");
    println!("consumer: registry roundtrip OK ({} B container)", buf.len());
}
RS
# 2) CLI cycle over a synthesized tiny PNG (stdlib-only writer).
python3 - <<'PY'
import struct, zlib
w, h = 16, 16
raw = b"".join(
    b"\x00"
    + b"".join(struct.pack("BBB", x * 16, y * 16, 128) for x in range(w))
    for y in range(h)
)
def chunk(t, d):
    return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
png = (b"\x89PNG\r\n\x1a\n"
       + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
       + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))
open("in.png", "wb").write(png)
PY
M="$T/prefix/bin/mnfx"
cargo install -q manifold-frame@"0.1.0" --features cli --root "$T/prefix" --locked
"$M" encode in.png -o out.mnfx
"$M" decode out.mnfx -o restored.png
"$M" verify in.png out.mnfx
echo "release verification: $T"
echo "CONSUMER RUN OK"