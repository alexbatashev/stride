load("@rules_rust//rust:defs.bzl", "rust_library")
load("@rust_crates//:defs.bzl", "crate_deps")

rust_library(
    name = "core",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "friday",
    edition = "2024",
    deps = crate_deps(
        [
            "async-stream",
            "futures",
            "serde",
            "serde_json",
            "thiserror",
            "tokio",
            "uuid",
        ],
        package_name = "",
    ) + [
        "//libs/JSKit:CQuickJS",
        "//libs/llm:llm",
        "//libs/minisql:minisql",
    ],
    visibility = ["//visibility:public"],
)
