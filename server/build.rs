use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=frontend/src");
    println!("cargo:rerun-if-changed=frontend/package.json");
    println!("cargo:rerun-if-changed=frontend/build.mjs");
    println!("cargo:rerun-if-changed=frontend/tsconfig.json");

    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let frontend = std::path::Path::new(&manifest).join("frontend");

    let status = Command::new("pnpm")
        .args(["install", "--frozen-lockfile"])
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
}
