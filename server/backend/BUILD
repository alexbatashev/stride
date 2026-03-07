load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_library", "rust_test")
load("@crates//:defs.bzl", "crate_deps")

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
        "tonic-web",
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
    srcs = glob(["src/**/*.rs"], exclude = ["src/main.rs"]),
    crate_name = "friday_backend",
    edition = "2024",
    deps = _DEPS,
)

rust_binary(
    name = "friday-serve",
    srcs = ["src/main.rs"],
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
    srcs = ["tests/auth_integration_test.rs"],
    crate_root = "tests/auth_integration_test.rs",
    edition = "2024",
    deps = [
        ":backend",
        "//libs/core:core",
    ] + crate_deps(
        [
            "uuid",
            "tokio",
            "tonic",
        ],
        package_name = "",
    ),
)
