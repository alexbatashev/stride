use std::env;
use std::process::Command;

// Components compiled to Rust for SSR. app-files.tsx is client-only (it
// drives the REST API from the browser), so it stays out of this list.
const SSR_COMPONENTS: &[&str] = &[
    "src/components/app-approval-bar.tsx",
    "src/components/app-button.tsx",
    "src/components/app-message.tsx",
    "src/components/app-prompt-input.tsx",
    "src/components/app-quiz-bar.tsx",
    "src/components/app-sidebar.tsx",
    "src/components/app-spoiler.tsx",
    "src/components/app-text-input.tsx",
    "src/components/auth-form.tsx",
    "src/components/auto-markdown.tsx",
];

fn main() {
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/package.json");
    println!("cargo:rerun-if-changed=frontend/build.mjs");
    println!("cargo:rerun-if-changed=frontend/tsconfig.json");

    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();
    let frontend = std::path::Path::new(&manifest).join("frontend");

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

    let icons_dir = frontend.join("src/components/icons");
    let mut icons: Vec<String> = std::fs::read_dir(&icons_dir)
        .expect("failed to read icons dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "tsx"))
        .map(|e| format!("src/components/icons/{}", e.file_name().to_str().unwrap()))
        .collect();
    icons.sort();

    let status = Command::new("pnpm")
        .current_dir(&frontend)
        .args(["exec", "argon", "compile"])
        .args(SSR_COMPONENTS)
        .args(&icons)
        .args(["--rust", "--out-dir", &out_dir])
        .status()
        .expect("argon compile failed");
    assert!(status.success(), "argon --rust failed");
}
