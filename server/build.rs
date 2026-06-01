use std::env;
use std::process::Command;

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

    // Compile Argon components to Rust for SSR
    println!("cargo:rerun-if-changed=frontend/src/components/app-button.ts");
    let status = Command::new("pnpm")
        .current_dir(&frontend)
        .args([
            "exec",
            "argon",
            "compile",
            "src/components/app-button.ts",
            "--rust",
            "--out-dir",
            &out_dir,
        ])
        .status()
        .expect("argon compile failed");
    assert!(status.success(), "argon --rust failed for app-button.ts");

    let icons_dir = frontend.join("src/components/icons");
    let mut entries: Vec<_> = std::fs::read_dir(&icons_dir)
        .expect("failed to read icons dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "ts"))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let file_name = entry.file_name().to_str().unwrap().to_string();
        println!("cargo:rerun-if-changed=frontend/src/components/icons/{file_name}");
        let status = Command::new("pnpm")
            .current_dir(&frontend)
            .args([
                "exec",
                "argon",
                "compile",
                &format!("src/components/icons/{file_name}"),
                "--rust",
                "--out-dir",
                &out_dir,
            ])
            .status()
            .expect("argon compile failed");
        assert!(status.success(), "argon --rust failed for {file_name}");
    }
}
