load("@build_bazel_rules_swift//swift:swift_library.bzl", "swift_library")
load("@build_bazel_rules_swift//swift:swift_test.bzl", "swift_test")

swift_library(
    name = "LLMKit",
    srcs = glob(["Sources/LLMKit/**/*.swift"]),
    module_name = "LLMKit",
    visibility = ["//visibility:public"],
)

swift_test(
    name = "LLMKitTests",
    srcs = glob(["Tests/LLMKitTests/**/*.swift"]),
    deps = [":LLMKit"],
)
