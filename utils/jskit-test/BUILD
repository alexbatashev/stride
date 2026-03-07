load("@rules_rust//rust:defs.bzl", "rust_binary")

rust_binary(
    name = "jskit-test",
    srcs = ["src/main.rs"],
    crate_name = "jskit_test",
    edition = "2024",
    deps = ["//libs/core:core"],
    visibility = ["//visibility:public"],
)
