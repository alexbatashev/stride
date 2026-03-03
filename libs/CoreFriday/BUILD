load("@build_bazel_rules_swift//swift:swift_compiler_plugin.bzl", "swift_compiler_plugin")
load("@build_bazel_rules_swift//swift:swift_library.bzl", "swift_library")
load("@build_bazel_rules_swift//swift:swift_test.bzl", "swift_test")

swift_compiler_plugin(
    name = "CoreFridayMacroPlugin",
    srcs = glob(["Sources/CoreFridayMacroPlugin/**/*.swift"]),
    module_name = "CoreFridayMacroPlugin",
    deps = [
        "@swiftpkg_swift_syntax//:SwiftCompilerPlugin",
        "@swiftpkg_swift_syntax//:SwiftSyntax",
        "@swiftpkg_swift_syntax//:SwiftSyntaxBuilder",
        "@swiftpkg_swift_syntax//:SwiftSyntaxMacros",
    ],
)

swift_library(
    name = "CoreFridayMacros",
    srcs = glob(["Sources/CoreFridayMacros/**/*.swift"]),
    module_name = "CoreFridayMacros",
    deps = [":CoreFridayMacroPlugin"],
)

swift_library(
    name = "CoreFriday",
    srcs = glob(["Sources/CoreFriday/**/*.swift"]),
    module_name = "CoreFriday",
    plugins = [":CoreFridayMacroPlugin"],
    deps = [
        "//libs/LLMKit:LLMKit",
        "//libs/JSKit:JSKit",
        ":CoreFridayMacros",
        "@swiftpkg_fluent//:Fluent",
        "@swiftpkg_fluent_sqlite_driver//:FluentSQLiteDriver",
    ],
    visibility = ["//visibility:public"],
)

swift_test(
    name = "CoreFridayTests",
    srcs = glob(["Tests/CoreFridayTests/**/*.swift"]),
    deps = [":CoreFriday"],
)
