load("@sourcekit_bazel_bsp//rules:setup_sourcekit_bsp.bzl", "setup_sourcekit_bsp")
load(
    "@rules_xcodeproj//xcodeproj:defs.bzl",
    "top_level_target",
    "xcodeproj",
)

setup_sourcekit_bsp(
    name = "setup_sourcekit_bsp",
    bazel_wrapper = "bazelisk",
    files_to_watch = [
        "libs/**/*.swift",
        "utils/**/*.swift",
        "apple/**/*.swift",
    ],
    index_flags = [
        "config=index_build",
    ],
    index_build_batch_size = 10,
    targets = [
        "//libs/...",
        "//apple/Friday/...",
    ],
    tags = ["manual"],
)

xcodeproj(
    name = "xcodeproj",
    project_name = "Friday",
    tags = ["manual", "apple"],
    target_compatible_with = ["@platforms//os:osx"],
    top_level_targets = [
        top_level_target("//apple/Friday:FridayiOS", target_environments = ["device", "simulator"]),
        top_level_target("//apple/Friday:FridaymacOS", target_environments = ["device"]),
    ],
)
