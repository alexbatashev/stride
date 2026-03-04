load("@rules_rust//rust:defs.bzl", "rust_library")
load("@rust_crates//:defs.bzl", "crate_deps")

rust_library(
    name = "minisql",
    srcs = glob(["src/**/*.rs"]),
    crate_name = "minisql",
    edition = "2024",
    deps = crate_deps(
        [
            "futures",
            "serde",
            "sqlite",
            "sqlite-vec",
            "sqlparser",
            "uuid",
        ],
        package_name = "",
    ),
    proc_macro_deps = ["//libs/minisql-macros:minisql_macros"],
    visibility = ["//visibility:public"],
)
