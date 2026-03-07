load("@rules_rust//rust:defs.bzl", "rust_library", "rust_test")
load("@crates//:defs.bzl", "crate_deps")

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

_minisql_test_srcs = glob(["tests/*.rs"])

_minisql_test_targets = [
    "minisql_int_test_" + src.replace("tests/", "").replace(".rs", "")
    for src in _minisql_test_srcs
]

[
    rust_test(
        name = "minisql_int_test_" + src.replace("tests/", "").replace(".rs", ""),
        srcs = [src],
        crate_root = src,
        deps = [":minisql"] + crate_deps([
            "tokio",
            "uuid",
        ], package_name = ""),
    )
    for src in _minisql_test_srcs
]

test_suite(
    name = "minisql_int_test",
    tests = [":" + target for target in _minisql_test_targets],
)
