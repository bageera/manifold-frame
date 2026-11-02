fn main() {
    // Build-script cwd = this crate's manifest dir; smoke.c lives here.
    // The workspace root is the ancestor holding both capi/ and src/
    // (anchored find — depth arithmetic breaks if the crate moves).
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = std::path::Path::new(&manifest)
        .ancestors()
        .find(|p| p.join("capi").is_dir() && p.join("src").is_dir())
        .unwrap()
        .to_path_buf();
    let target = root.join("target").join("release");
    let staticlib = target.join("libmanifold_frame_capi.a");
    if !staticlib.exists() {
        panic!(
            "missing {} — run `cargo build -p manifold-frame-capi --release` first",
            staticlib.display()
        );
    }
    println!("cargo:rustc-link-search=native={}", target.display());
    println!("cargo:rustc-link-lib=static=manifold_frame_capi");
    println!("cargo:rerun-if-changed=smoke.c");
    println!(
        "cargo:rerun-if-changed={}",
        root.join("capi").join("include").join("mnfx.h").display()
    );
    cc::Build::new()
        .file("smoke.c")
        .include(root.join("capi").join("include"))
        .warnings_into_errors(true)
        .compile("mnfx_smoke");
}