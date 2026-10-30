//! The ADR-009 W1 corpus gate: for every corpus image, the MNFX container
//! (MNFX-forced embed) decodes byte-identically to the standalone
//! MNF8/MNF9 decode of the same engine file, and the framing overhead is
//! exactly +34 B per file. The 30-file corpus = testdata roots + photos +
//! heldout + heldout-photos (the mandatory control from manifold's gates).

use std::path::{Path, PathBuf};

fn corpus() -> Vec<PathBuf> {
    let root = Path::new("../manifold/testdata");
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in ["", "photos", "heldout", "heldout-photos"] {
        let d = root.join(dir);
        let mut got: Vec<PathBuf> = std::fs::read_dir(&d)
            .unwrap_or_else(|e| panic!("corpus dir {}: {e}", d.display()))
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "png"))
            .collect();
        got.sort();
        files.extend(got);
    }
    files
}

/// Wrap a standalone engine file as the sole PIXEL chunk (MNFX-forced).
fn wrap(file: &[u8], codec: manifold_frame::Codec, w: u32, h: u32) -> Vec<u8> {
    let hdr = manifold_frame::ContainerHeader {
        pixel_format: manifold_frame::PixelFormat::Rgb8,
        orientation: 1,
        chunk_count: 1,
        width: w,
        height: h,
    };
    manifold_frame::serialize(&hdr, &[(manifold_frame::ChunkType::Pixel, codec, file)]).unwrap()
}

#[test]
fn corpus_embedded_equals_standalone_and_framing_is_34() {
    let files = corpus();
    assert_eq!(
        files.len(),
        30,
        "the 30-file corpus control must be complete (got {})",
        files.len()
    );
    let mut rows = Vec::new();
    for path in &files {
        let img = manifold::img::load(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let dims = manifold::codec::Dims {
            width: img.width,
            height: img.height,
        };
        // The CLI pick shape: MNF8 (the --mnf8 arm competes with the MNF9
        // whole-frame arm at final file size; mirror that here).
        let (mnf8, _st, _b) =
            manifold::mnf8::encode_mnf8(&img.pixels, dims, false, manifold::mnf8::Profile::Maximum);
        let file = match manifold::mnf9::encode_exact(&img.pixels, dims) {
            Some(mnf9) if mnf9.len() < mnf8.len() => (mnf9, manifold_frame::Codec::MnF9),
            _ => (mnf8.clone(), manifold_frame::Codec::MnF8),
        };
        let (engine_file, codec) = file;
        let buf = wrap(&engine_file, codec, dims.width, dims.height);

        // Gate 1: framing = +34 B exactly, per file, measured.
        assert_eq!(
            buf.len() - engine_file.len(),
            34,
            "{}: framing {} B",
            path.display(),
            buf.len() - engine_file.len()
        );

        // Gate 2: the container parses and its pixel decode is EXACT
        // equality with the standalone engine decode of the same file.
        let (h, chunks) =
            manifold_frame::parse(&buf).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let got = manifold_frame::decode_pixels(&h, &chunks)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let (sdims, spx) = if codec == manifold_frame::Codec::MnF9 {
            manifold::mnf9::decode_mnf9(&engine_file).unwrap()
        } else {
            let (d, px, _b) = manifold::mnf8::decode_mnf8(&engine_file).unwrap();
            (d, px)
        };
        assert_eq!(got.dims, sdims, "{}: dims differ", path.display());
        assert_eq!(
            got.rgb,
            spx,
            "{}: embedded decode != standalone ({} vs {} B)",
            path.display(),
            got.rgb.len(),
            spx.len()
        );
        // Gate 3: and the standalone decode is EXACT vs the source pixels.
        assert_eq!(got.rgb, img.pixels, "{}: pixels lost", path.display());

        rows.push(format!(
            "{:44} {} {} {} {:>8} B +34",
            path.file_name().unwrap().to_string_lossy(),
            dims.width,
            dims.height,
            if codec == manifold_frame::Codec::MnF9 {
                "MNF9"
            } else {
                "MNF8"
            },
            engine_file.len()
        ));
    }
    // The measured table is the evidence; print for the test log.
    println!(
        "MNFX-forced corpus (30 files, maximum profile):\n{}",
        rows.join("\n")
    );
}
