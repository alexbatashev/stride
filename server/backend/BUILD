load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_library", "rust_test")
load("@crates//:defs.bzl", "crate_deps")

_DEPS = crate_deps(
    [
        "argon2",
        "async-stream",
        "axum",
        "base64",
        "bytes",
        "chacha20poly1305",
        "futures",
        "jsonwebtoken",
        "ldap3",
        "serde",
        "sha2",
        "tar",
        "tokio",
        "tower",
        "prost",
        "tonic",
        "tonic-web",
        "tower-http",
        "uuid",
        "toml",
        "hyper",
    ],
    package_name = "",
) + [
    "//libs/core:core",
    "//libs/llm:llm",
    "//libs/minisql:minisql",
]

rust_library(
    name = "backend",
    srcs = glob(["src/**/*.rs"], exclude = ["src/main.rs"]),
    compile_data = ["//server/frontend:frontend_tar"],
    crate_name = "friday_backend",
    edition = "2024",
    rustc_env = {
        "FRONTEND_TAR": "$(execpath //server/frontend:frontend_tar)",
    },
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

rust_test(
    name = "llm_integration_test",
    srcs = ["tests/llm_integration_test.rs"],
    crate_root = "tests/llm_integration_test.rs",
    edition = "2024",
    deps = [
        ":backend",
        "//libs/core:core",
        "//libs/minisql:minisql",
    ] + crate_deps(
        [
            "tokio",
            "tonic",
            "uuid",
        ],
        package_name = "",
    ),
)
