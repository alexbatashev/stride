load("@build_bazel_rules_apple//apple:apple.bzl", "local_provisioning_profile")
load("@build_bazel_rules_apple//apple:ios.bzl", "ios_application")
load("@build_bazel_rules_apple//apple:macos.bzl", "macos_application")
load("@build_bazel_rules_swift//swift:swift_library.bzl", "swift_library")
load("@build_bazel_rules_swift//swift:swift_test.bzl", "swift_test")

_DEPS = [
    "//libs/CoreFriday:CoreFriday",
    "@swiftpkg_swift_markdown//:Markdown",
]

# Used by tests: excludes FridayApp.swift (@main) to avoid duplicate _main
# with the test runner's entry point.
swift_library(
    name = "FridayLib",
    srcs = glob(
        ["Friday/**/*.swift"],
        exclude = ["Friday/FridayApp.swift"],
    ),
    module_name = "Friday",
    deps = _DEPS,
)

# App entry point — separate module so its @main doesn't conflict with tests.
# FridayApp.swift uses `import Friday` to access internal-made-public types.
swift_library(
    name = "FridayAppEntry",
    srcs = ["Friday/FridayApp.swift"],
    module_name = "FridayAppEntry",
    deps = [":FridayLib"],
)

# macOS bundle
macos_application(
    name = "FridaymacOS",
    bundle_id = "me.batashev.Friday",
    infoplists = ["Friday/Info.plist"],
    minimum_os_version = "26.2",
    resources = glob(
        ["Friday/Assets.xcassets/**"],
        exclude = ["Friday/Assets.xcassets/AppIcon.appiconset/**"],
    ),
    deps = [":FridayAppEntry"],
)

# Provisioning profile for iOS device builds — resolved from locally installed
# profiles at build time. Add team_id once your Apple Developer team is known.
local_provisioning_profile(
    name = "ios_provisioning_profile",
    profile_name = "iOS Team Provisioning Profile: me.batashev.Friday",
)

# iOS bundle
ios_application(
    name = "FridayiOS",
    bundle_id = "me.batashev.Friday",
    families = [
        "iphone",
        "ipad",
    ],
    infoplists = ["Friday/Info.plist"],
    minimum_os_version = "26.2",
    provisioning_profile = ":ios_provisioning_profile",
    resources = glob(
        ["Friday/Assets.xcassets/**"],
        exclude = ["Friday/Assets.xcassets/AppIcon.appiconset/**"],
    ),
    deps = [":FridayAppEntry"],
)

# Unit tests (swift_test, no app host needed)
# no-sandbox: ModelData.init() writes to ~/Library/Application Support/Friday/
swift_test(
    name = "FridayTests",
    srcs = ["FridayTests/FridayTests.swift"],
    deps = [":FridayLib"],
    tags = ["no-sandbox"],
)
