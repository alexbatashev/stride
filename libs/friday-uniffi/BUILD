load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_library", "rust_shared_library")
load("@rust_crates//:defs.bzl", "crate_deps")

rust_library(
    name = "friday_uniffi",
    srcs = ["src/lib.rs"],
    compile_data = ["Cargo.toml"],
    crate_name = "friday_uniffi",
    edition = "2024",
    deps = crate_deps(
        [
            "futures",
            "thiserror",
            "tokio",
            "uniffi",
            "uuid",
        ],
        package_name = "",
    ) + [
        "//libs/core:core",
    ],
    visibility = ["//visibility:public"],
)

rust_shared_library(
    name = "friday_uniffi_cdylib",
    srcs = ["src/lib.rs"],
    compile_data = ["Cargo.toml"],
    crate_name = "friday_uniffi",
    edition = "2024",
    deps = crate_deps(
        [
            "futures",
            "thiserror",
            "tokio",
            "uniffi",
            "uuid",
        ],
        package_name = "",
    ) + [
        "//libs/core:core",
    ],
    tags = ["manual"],
    visibility = ["//visibility:public"],
)

rust_binary(
    name = "bindgen",
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
    srcs = [":friday_uniffi_cdylib"],
    outs = [
        "swift/friday_uniffi.swift",
        "swift/friday_uniffiFFI.h",
        "swift/friday_uniffiFFI.modulemap",
    ],
    cmd = "$(execpath :bindgen) --language swift --library $(location :friday_uniffi_cdylib) --out-dir $(RULEDIR)/swift && test -f $(RULEDIR)/swift/friday_uniffi.swift && test -f $(RULEDIR)/swift/friday_uniffiFFI.h && test -f $(RULEDIR)/swift/friday_uniffiFFI.modulemap",
    tools = [":bindgen"],
    tags = ["manual"],
)

genrule(
    name = "kotlin_bindings",
    srcs = [":friday_uniffi_cdylib"],
    outs = ["kotlin/uniffi/friday_uniffi/friday_uniffi.kt"],
    cmd = "$(execpath :bindgen) --language kotlin --library $(location :friday_uniffi_cdylib) --out-dir $(RULEDIR)/kotlin && test -f $(RULEDIR)/kotlin/uniffi/friday_uniffi/friday_uniffi.kt",
    tools = [":bindgen"],
    tags = ["manual"],
)

filegroup(
    name = "bindings",
    srcs = [
        ":swift_bindings",
        ":kotlin_bindings",
    ],
    visibility = ["//visibility:public"],
)
