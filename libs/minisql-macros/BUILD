load("@rules_rust//rust:defs.bzl", "rust_proc_macro")
load("@crates//:defs.bzl", "crate_deps")

rust_proc_macro(
    name = "minisql_macros",
    srcs = ["src/lib.rs"],
    crate_name = "minisql_macros",
    edition = "2024",
    deps = crate_deps(
        ["proc-macro2", "quote", "syn"],
        package_name = "",
    ),
    visibility = ["//visibility:public"],
)
