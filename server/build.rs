use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    install_frontend_dependencies();
    argon_build::run("frontend/argon.toml");
}

fn install_frontend_dependencies() {
    if env::var_os("ARGON_PREBUILT_DIR").is_some() || env::var_os("ARGON_BIN").is_some() {
        return;
    }

    let manifest_dir = env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set");
    let frontend = Path::new(&manifest_dir).join("frontend");
    let status = Command::new("pnpm")
        .args(["install", "--frozen-lockfile"])
        .env("CI", "true")
        .current_dir(frontend)
        .status()
        .expect("failed to run pnpm install");

    assert!(status.success(), "pnpm install failed with {status}");
}
