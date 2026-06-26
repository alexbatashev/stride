use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    // Offline / Nix builds: the frontend (including the Argon-generated SSR
    // modules) is produced by a separate derivation that has the JS toolchain
    // and network access. Copy those prebuilt modules into OUT_DIR and skip
    // pnpm entirely so the Rust build needs no network or Node.
    println!("cargo:rerun-if-env-changed=STRIDE_PREBUILT_SSR_DIR");
    if let Ok(prebuilt) = env::var("STRIDE_PREBUILT_SSR_DIR") {
        copy_prebuilt_ssr(Path::new(&prebuilt), Path::new(&out_dir));
        return;
    }

    build_frontend(&out_dir);
}

// Copies every generated `*.rs` Argon module from a prebuilt directory into
// OUT_DIR, where `components.rs` includes them.
fn copy_prebuilt_ssr(src: &Path, out_dir: &Path) {
    let entries = fs::read_dir(src)
        .unwrap_or_else(|e| panic!("failed to read STRIDE_PREBUILT_SSR_DIR {src:?}: {e}"));
    let mut copied = 0;
    for entry in entries {
        let path = entry.expect("failed to read prebuilt SSR entry").path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let dest = out_dir.join(path.file_name().unwrap());
            fs::copy(&path, &dest).expect("failed to copy prebuilt SSR module");
            copied += 1;
        }
    }
    assert!(
        copied > 0,
        "STRIDE_PREBUILT_SSR_DIR {src:?} held no .rs modules"
    );
}

fn build_frontend(out_dir: &str) {
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/package.json");
    println!("cargo:rerun-if-changed=frontend/build.mjs");
    println!("cargo:rerun-if-changed=frontend/tsconfig.json");
    println!("cargo:rerun-if-changed=frontend/ssr-components.txt");

    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let frontend = Path::new(&manifest).join("frontend");

    let status = Command::new("pnpm")
        .args(["install", "--frozen-lockfile"])
        .env("CI", "true")
        .current_dir(&frontend)
        .status()
        .expect("pnpm install failed");
    assert!(status.success(), "pnpm install failed");

    let status = Command::new("pnpm")
        .args(["run", "build"])
        .current_dir(&frontend)
        .status()
        .expect("pnpm build failed");
    assert!(status.success(), "pnpm build failed");

    compile_ssr_modules(&frontend, out_dir);
}

// Runs `argon compile --rust` over the curated SSR component list plus every
// icon, writing the generated modules into OUT_DIR.
fn compile_ssr_modules(frontend: &Path, out_dir: &str) {
    let ssr = read_ssr_components(frontend);
    let icons = read_icon_components(frontend);

    let status = Command::new("pnpm")
        .current_dir(frontend)
        .args(["exec", "argon", "compile"])
        .args(&ssr)
        .args(&icons)
        .args(["--rust", "--out-dir", out_dir])
        .status()
        .expect("argon compile failed");
    assert!(status.success(), "argon --rust failed");
}

fn read_ssr_components(frontend: &Path) -> Vec<String> {
    let manifest = frontend.join("ssr-components.txt");
    let text = fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("read {manifest:?}: {e}"));
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

fn read_icon_components(frontend: &Path) -> Vec<String> {
    let icons_dir = frontend.join("src/components/icons");
    let mut icons: Vec<String> = fs::read_dir(&icons_dir)
        .expect("failed to read icons dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "tsx"))
        .map(|e| format!("src/components/icons/{}", e.file_name().to_str().unwrap()))
        .collect();
    icons.sort();
    icons
}
