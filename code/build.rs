use std::path::{Path, PathBuf};
use std::process::Command;

const CAPNP_VERSION: &str = "1.0.2";

fn main() {
    let mut cmd = capnpc::CompilerCommand::new();
    cmd.src_prefix("schema").file("schema/agent.capnp");

    // Fall back to a self-built compiler when capnp is not on PATH.
    if let Some(capnp) = build_capnp_if_missing() {
        cmd.capnp_executable(capnp);
    }

    cmd.run().expect("compiling Cap'n'Proto schema");
}

fn build_capnp_if_missing() -> Option<PathBuf> {
    if Command::new("capnp").arg("--version").output().is_ok() {
        return None;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let prefix = out_dir.join("capnp-install");
    let bin = prefix.join("bin").join(capnp_bin_name());
    if bin.exists() {
        return Some(bin);
    }

    let src = download_source(&out_dir);
    cmake::Config::new(&src)
        .define("BUILD_TESTING", "OFF")
        .out_dir(&prefix)
        .build();
    Some(bin)
}

fn download_source(out_dir: &Path) -> PathBuf {
    let src = out_dir.join(format!("capnproto-{CAPNP_VERSION}"));
    if src.join("CMakeLists.txt").exists() {
        return src;
    }

    let tarball = out_dir.join("capnproto.tar.gz");
    let url = format!(
        "https://github.com/capnproto/capnproto/archive/refs/tags/v{CAPNP_VERSION}.tar.gz"
    );
    run(
        Command::new("curl")
            .args(["-sSfL", "-o"])
            .arg(&tarball)
            .arg(&url),
        "downloading Cap'n'Proto source",
    );
    run(
        Command::new("tar")
            .arg("xzf")
            .arg(&tarball)
            .arg("-C")
            .arg(out_dir),
        "extracting Cap'n'Proto source",
    );
    src
}

fn run(cmd: &mut Command, what: &str) {
    let status = cmd.status().unwrap_or_else(|e| panic!("{what} failed: {e}"));
    assert!(status.success(), "{what} failed");
}

fn capnp_bin_name() -> &'static str {
    if cfg!(windows) { "capnp.exe" } else { "capnp" }
}
