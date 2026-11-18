//! MNFX CLI — encode a PNG into an MNFX container, decode MNFX back to PNG.
//!
//! The reviewer's central W5 acceptance shape: a real image (pixels +
//! metadata + color profile + orientation) converts into MNFX and back
//! with nothing unintended lost.
//!
//! Usage:
//!   mnfx encode <image.png> -o out.mnfx [--geom] [--mnf8[=balanced|maximum]]
//!   mnfx decode <in.mnfx> -o out.png
//!   mnfx verify <image.png> <in.mnfx>

use manifold::codec::Dims;
use manifold::img::{self, Image};
use manifold::mnf8::{encode_mnf8, Profile};
use manifold::mnf9;
use manifold_frame::{ChunkType, Codec, ContainerHeader, PixelFormat};
use std::path::PathBuf;

fn die(m: &str) -> ! {
    eprintln!("{m}");
    std::process::exit(2)
}

const USAGE: &str = concat!(
    "mnfx v",
    env!("CARGO_PKG_VERSION"),
    " usage:
  mnfx encode <image.png> -o out.mnfx [--geom] [--mnf8[=balanced|maximum]]
  mnfx decode <in.mnfx> -o out.png
  mnfx verify <image.png> <in.mnfx>
Notes: the CLI converts PIXELS only (source ICC/EXIF/XMP are not
carried; v1 contract). --mnf8 (optionally =balanced|=maximum) PINS the
embedded payload to the MNF8 engine at that profile; without it the
encoder ships whichever whole-frame engine stream is smaller."
);

/// W8-F1 (audit #2, pinned by tests/cli_audit.rs): `--help`/`-h`
/// print usage to STDOUT with exit 0; argument errors print usage to
/// STDERR with exit 2 (die()).
fn help_or_usage() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() == 1 && (a[0] == "--help" || a[0] == "-h") {
        println!("{USAGE}");
        std::process::exit(0);
    }
}

struct Args {
    cmd: String,
    src: PathBuf,
    out: PathBuf,
    verify_file: Option<PathBuf>,
    geom: bool,
    profile: Profile,
    /// W8-F1 (audit #1): `--mnf8` (bare or with a profile) PINS the
    /// payload to the MNF8 engine — the MNF9 smaller-stream arm stops
    /// competing, so the embedded magic always matches the selection.
    force_mnf8: bool,
}

fn parse_args() -> Args {
    help_or_usage();
    let a: Vec<String> = std::env::args().skip(1).collect();
    let bad = |m: String| -> ! { die(&format!("{USAGE}\n{m}")) };
    let cmd = a
        .first()
        .cloned()
        .unwrap_or_else(|| bad("missing command".into()));
    if cmd.starts_with('-') || !matches!(cmd.as_str(), "encode" | "decode" | "verify") {
        bad(format!("unknown command '{cmd}'"));
    }
    let mut geom = false;
    let mut profile = Profile::Balanced;
    let mut force_mnf8 = false;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut it = a.iter().skip(1);
    while let Some(tok) = it.next() {
        match tok.as_str() {
            "-o" => {
                out = Some(PathBuf::from(
                    it.next().unwrap_or_else(|| bad("-o needs a path".into())),
                ))
            }
            "--geom" => geom = true,
            // W8-F1: all three spellings pin the engine (audit #1):
            // bare == balanced AND the pin; the profile variants imply
            // the pin, so none is a silent no-op anymore.
            "--mnf8" => {
                profile = Profile::Balanced;
                force_mnf8 = true;
            }
            "--mnf8=balanced" => {
                profile = Profile::Balanced;
                force_mnf8 = true;
            }
            "--mnf8=maximum" => {
                profile = Profile::Maximum;
                force_mnf8 = true;
            }
            other if !other.starts_with('-') => paths.push(PathBuf::from(other)),
            other => bad(format!("unknown flag '{other}'")),
        }
    }
    let src = paths
        .first()
        .cloned()
        .unwrap_or_else(|| bad("missing image path".into()));
    // encode/decode take their destination via -o; verify takes the
    // container as a second positional.
    let verify_file = if cmd == "verify" {
        Some(
            paths
                .get(1)
                .cloned()
                .unwrap_or_else(|| die("verify needs <image> <file.mnfx>")),
        )
    } else {
        None
    };
    let out = if cmd == "verify" {
        PathBuf::new() // unused
    } else {
        out.unwrap_or_else(|| bad("missing -o".into()))
    };
    Args {
        cmd,
        src,
        out,
        verify_file,
        geom,
        profile,
        force_mnf8,
    }
}

/// Extract the container's pixel plane back into a PNG-writable Image.
fn image_of(p: &manifold_frame::decode::Pixels) -> Result<Image, String> {
    match p.format {
        PixelFormat::Rgb8 => Ok(Image {
            width: p.dims.width,
            height: p.dims.height,
            pixels: p.samples.clone(),
        }),
        other => Err(format!(
            "png export of {other:?} lands with W5's full io surface (RGB8 is the corpus shape)"
        )),
    }
}

fn encode_cmd(args: &Args) {
    // Load via the image-io boundary. IMPORTANT (v1 contract): the CLI
    // encodes PIXELS only — a source PNG's ICC/EXIF/XMP are NOT carried
    // into the container (single-chunk Rgb8 shape; metadata stays a
    // serializer-path feature, see integrations/README.md's known
    // limitations). JPEG loads are exactness-caveated upstream — the
    // encode still works, the SOURCE was lossy, not MNFX.
    let img =
        img::load(&args.src).unwrap_or_else(|e| die(&format!("load {}: {e}", args.src.display())));
    let dims = Dims {
        width: img.width,
        height: img.height,
    };
    let t0 = std::time::Instant::now();
    // The engine pick, mirroring the manifold CLI's --mnf8 flow: MNF8
    // (the requested profile) competes with the MNF9 whole-frame arm at
    // final file size; smaller ships, decode dispatches by magic.
    let (mnf8_file, st, _backend) = encode_mnf8(&img.pixels, dims, args.geom, args.profile);
    // W8-F1: under the --mnf8 pin the MNF9 arm does not compete — the
    // selection flag and the embedded payload magic cannot disagree.
    let (file, codec) = if args.force_mnf8 {
        (mnf8_file, Codec::MnF8)
    } else {
        match mnf9::encode_exact(&img.pixels, dims) {
            Some(mnf9_file) if mnf9_file.len() < mnf8_file.len() => (mnf9_file, Codec::MnF9),
            _ => (mnf8_file, Codec::MnF8),
        }
    };
    let hdr = ContainerHeader {
        pixel_format: PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 1,
        width: dims.width,
        height: dims.height,
    };
    // W8-C: the CLI is a producer — every write it makes is PROVEN:
    // serialize_checked runs the emitted bytes through the full parse
    // rule set (rule-13 matrix incl. embed-on-Rgb8) before the file
    // lands on disk, per the produce-facing contract.
    let buf = manifold_frame::serialize_checked(&hdr, &[(ChunkType::Pixel, codec, &file)])
        .unwrap_or_else(|e| die(&format!("serialize: {e}")));
    std::fs::write(&args.out, &buf)
        .unwrap_or_else(|e| die(&format!("write {}: {e}", args.out.display())));
    println!(
        "mnfx encode {} {}: {} B (+{} framing), {} px, flags={:#04x}, {}",
        args.src.display(),
        if codec == Codec::MnF9 { "mnf9" } else { "mnf8" },
        buf.len(),
        buf.len() - file.len(),
        // W8-F1 (audit #3): the reported pixel count is computed in u64
        // (a u32 multiply would wrap/panic near the grammar's
        // checked_mul ceiling); the format machinery itself uses
        // checked u64 bounds (src/limits.rs, verified by w8e_limits).
        u64::from(dims.width) * u64::from(dims.height),
        st.chosen_flags,
        t0.elapsed().as_millis(),
    );
}

fn decode_cmd(args: &Args) {
    let buf = std::fs::read(&args.src)
        .unwrap_or_else(|e| die(&format!("read {}: {e}", args.src.display())));
    let (hdr, chunks) = manifold_frame::parse(&buf).unwrap_or_else(|e| die(&format!("parse: {e}")));
    let (px, _meta) = manifold_frame::decode_image(&hdr, &chunks)
        .unwrap_or_else(|e| die(&format!("decode: {e}")));
    let im = image_of(&px).unwrap_or_else(|e| die(&e));
    img::save_png(&args.out, px.dims, &im.pixels)
        .unwrap_or_else(|e| die(&format!("save {}: {e}", args.out.display())));
    println!(
        "mnfx decode {} {}x{} → {}",
        args.src.display(),
        px.dims.width,
        px.dims.height,
        args.out.display()
    );
}

fn verify_cmd(args: &Args) {
    let vf = args
        .verify_file
        .clone()
        .unwrap_or_else(|| die("verify needs <image> <file.mnfx>"));
    let src =
        img::load(&args.src).unwrap_or_else(|e| die(&format!("load {}: {e}", args.src.display())));
    let buf = std::fs::read(&vf).unwrap_or_else(|e| die(&format!("read {}: {e}", vf.display())));
    let (hdr, chunks) = manifold_frame::parse(&buf).unwrap_or_else(|e| die(&format!("parse: {e}")));
    let (px, _meta) = manifold_frame::decode_image(&hdr, &chunks)
        .unwrap_or_else(|e| die(&format!("decode: {e}")));
    let dims_ok = px.dims.width == src.width && px.dims.height == src.height;
    if dims_ok && px.samples == src.pixels {
        println!(
            "PASS: exact RGB8 reconstruction ({}x{})",
            src.width, src.height
        );
    } else {
        eprintln!("FAIL: reconstruction differs");
        std::process::exit(1);
    }
}

fn main() {
    let args = parse_args();
    match args.cmd.as_str() {
        "encode" => encode_cmd(&args),
        "decode" => decode_cmd(&args),
        "verify" => verify_cmd(&args),
        _ => unreachable!(),
    }
}
