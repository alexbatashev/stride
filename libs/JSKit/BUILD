load("@rules_cc//cc:defs.bzl", "cc_library")
load("@build_bazel_rules_swift//swift:swift_interop_hint.bzl", "swift_interop_hint")
load("@build_bazel_rules_swift//swift:swift_library.bzl", "swift_library")
load("@build_bazel_rules_swift//swift:swift_test.bzl", "swift_test")

swift_interop_hint(
    name = "CQuickJS_interop",
    module_name = "CQuickJS",
)

cc_library(
    name = "CQuickJS",
    srcs = ["Sources/CQuickJS/quickjs_shim.c"],
    hdrs = ["Sources/CQuickJS/include/quickjs_shim.h"],
    copts = [
        "-DQUICKJS_NG_BUILD",
        "-D_GNU_SOURCE",
    ],
    includes = ["Sources/CQuickJS/include"],
    aspect_hints = [":CQuickJS_interop"],
    deps = ["//vendor/quickjs:quickjs"],
)

swift_library(
    name = "JSKit",
    srcs = glob(["Sources/JSKit/**/*.swift"]),
    module_name = "JSKit",
    deps = [":CQuickJS"],
    visibility = ["//visibility:public"],
)

swift_test(
    name = "JSKitTests",
    srcs = glob(["Tests/JSKitTests/**/*.swift"]),
    deps = [":JSKit"],
)
