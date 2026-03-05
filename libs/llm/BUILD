load("@rules_rust//rust:defs.bzl", "rust_library")
load("@rust_crates//:defs.bzl", "crate_deps")

rust_library(
    name = "llm",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "llm",
    edition = "2024",
    deps = crate_deps(
        [
            "async-stream",
            "axum",
            "bytes",
            "futures",
            "http-body-util",
            "hyper",
            "hyper-rustls",
            "hyper-util",
            "mio",
            "rustls",
            "rustls-aws-lc-rs",
            "webpki-roots",
            "serde",
            "serde_json",
            "thiserror",
            "uuid",
        ],
        package_name = "",
    ),
    visibility = ["//visibility:public"],
)
