load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_library", "rust_test")
load("@rust_crates//:defs.bzl", "crate_deps")

_DEPS = crate_deps(
    [
        "argon2",
        "axum",
        "futures",
        "jsonwebtoken",
        "serde",
        "tokio",
        "prost",
        "tonic",
        "tower-http",
        "uuid",
    ],
    package_name = "",
) + [
    "//libs/core:core",
    "//libs/minisql:minisql",
]

rust_library(
    name = "backend",
    srcs = ["backend/lib.rs"],
    crate_name = "friday_backend",
    edition = "2024",
    aliases = {
        "@rrprd__prost-0.13.1//:prost": "prost",
        "@rrprd__tokio-1.39.3//:tokio": "tonic_tokio",
    },
    deps = _DEPS,
)

rust_binary(
    name = "friday-serve",
    srcs = ["backend/main.rs"],
    crate_name = "friday_serve",
    edition = "2024",
    deps = [
        ":backend",
    ] + crate_deps(["tokio"], package_name = ""),
    data = [
        "//libs/core:proto/hello.proto",
        "//server/frontend:frontend",
    ],
)

rust_test(
    name = "auth_integration_test",
    srcs = ["backend/auth_integration_test.rs"],
    crate_root = "backend/auth_integration_test.rs",
    edition = "2024",
    deps = [
        ":backend",
        "//libs/core:core",
        "@rrprd__tokio-1.39.3//:tokio",
        "@rrprd__tonic-0.12.1//:tonic",
    ] + crate_deps(
        [
            "uuid",
        ],
        package_name = "",
    ),
)
