load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_library", "rust_shared_library")
load("@rust_crates//:defs.bzl", "crate_deps")

rust_library(
    name = "core",
    srcs = glob(["src/**/*.rs"]),
    compile_data = ["Cargo.toml"],
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
            "uniffi",
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

rust_shared_library(
    name = "friday_cdylib",
    srcs = glob(["src/**/*.rs"]),
    compile_data = ["Cargo.toml"],
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
            "uniffi",
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

rust_binary(
    name = "uniffi_bindgen_tool",
    srcs = ["tools/bindgen.rs"],
    crate_name = "friday_uniffi_bindgen",
    edition = "2024",
    deps = crate_deps(
        [
            "camino",
            "uniffi_bindgen",
        ],
        package_name = "",
    ),
    tags = ["manual"],
)

genrule(
    name = "swift_bindings",
    srcs = [":friday_cdylib"],
    outs = [
        "swift/friday.swift",
        "swift/fridayFFI.h",
        "swift/fridayFFI.modulemap",
    ],
    cmd = "$(execpath :uniffi_bindgen_tool) --language swift --library $(location :friday_cdylib) --out-dir $(RULEDIR)/swift && test -f $(RULEDIR)/swift/friday.swift && test -f $(RULEDIR)/swift/fridayFFI.h && test -f $(RULEDIR)/swift/fridayFFI.modulemap",
    tools = [":uniffi_bindgen_tool"],
    tags = ["manual"],
)

genrule(
    name = "kotlin_bindings",
    srcs = [":friday_cdylib"],
    outs = ["kotlin/uniffi/friday/friday.kt"],
    cmd = "$(execpath :uniffi_bindgen_tool) --language kotlin --library $(location :friday_cdylib) --out-dir $(RULEDIR)/kotlin && test -f $(RULEDIR)/kotlin/uniffi/friday/friday.kt",
    tools = [":uniffi_bindgen_tool"],
    tags = ["manual"],
)
