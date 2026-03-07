load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_library", "rust_shared_library", "rust_static_library")
load("@rules_rust_prost//:defs.bzl", "rust_prost_library")
load("@rust_crates//:defs.bzl", "crate_deps")
load("@rules_cc//cc:defs.bzl", "cc_library")
load("@protobuf//bazel:proto_library.bzl", "proto_library")
load("@build_bazel_rules_swift//swift:swift_interop_hint.bzl", "swift_interop_hint")
load("@build_bazel_rules_swift//swift:swift_library.bzl", "swift_library")

_SRCS = glob(["src/**/*.rs"])

proto_library(
    name = "api_proto",
    srcs = ["proto/hello.proto"],
)

rust_prost_library(
    name = "hello_grpc_rs",
    proto = ":api_proto",
)

_DEPS = crate_deps(
    [
        "async-stream",
        "futures",
        "hyper",
        "serde",
        "serde_json",
        "thiserror",
        "tokio",
        "uniffi",
        "ureq",
        "uuid",
        "tonic",
    ],
    package_name = "",
) + select({
    "@platforms//os:osx": crate_deps(
        [
            "dispatch2",
        ],
        package_name = "",
    ),
    "//conditions:default": [],
}) + [
    "//vendor/quickjs:quickjs",
    "//libs/llm:llm",
    "//libs/minisql:minisql",
    ":hello_grpc_rs",
]

rust_library(
    name = "core",
    srcs = _SRCS,
    compile_data = ["Cargo.toml"],
    crate_name = "friday",
    edition = "2024",
    deps = _DEPS,
    visibility = ["//visibility:public"],
)

# Used by uniffi_bindgen_tool to generate Swift/Kotlin bindings
rust_shared_library(
    name = "friday_cdylib",
    srcs = _SRCS,
    compile_data = ["Cargo.toml"],
    crate_name = "friday",
    edition = "2024",
    deps = _DEPS,
    visibility = ["//visibility:public"],
)

# Statically linked for embedding in the Swift app bundle
rust_static_library(
    name = "friday_staticlib",
    srcs = _SRCS,
    compile_data = ["Cargo.toml"],
    crate_name = "friday",
    edition = "2024",
    deps = _DEPS,
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

# C module wrapping the uniffi FFI header, linked against the Rust static lib.
# swift/fridayFFI.h is a generated output of :swift_bindings — Bazel builds it automatically.
swift_interop_hint(
    name = "friday_ffi_interop",
    module_name = "fridayFFI",
)

cc_library(
    name = "friday_ffi_c",
    hdrs = ["swift/fridayFFI.h"],
    includes = ["swift"],
    aspect_hints = [":friday_ffi_interop"],
    deps = [":friday_staticlib"],
    visibility = ["//visibility:public"],
)

# Swift library with uniffi-generated bindings.
# swift/friday.swift is a generated output of :swift_bindings — Bazel builds it automatically.
swift_library(
    name = "FridayCoreSwift",
    srcs = [
        "swift/friday.swift",
        "swift_ext/FridayExtensions.swift",
    ],
    module_name = "CoreFriday",
    deps = [":friday_ffi_c"],
    visibility = ["//visibility:public"],
)
