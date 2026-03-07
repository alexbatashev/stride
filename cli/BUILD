load("@rules_rust//rust:defs.bzl", "rust_binary")
load("@rust_crates//:defs.bzl", "crate_deps")

rust_binary(
    name = "friday",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "friday_cli",
    edition = "2024",
    deps = crate_deps(
        [
            "futures",
            "tokio",
            "uuid",
        ],
        package_name = "",
    ) + [
        "//libs/core:core",
    ],
    visibility = ["//visibility:public"],
)
