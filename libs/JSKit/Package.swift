// swift-tools-version: 6.2
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "JSKit",
    products: [
        .library(
            name: "JSKit",
            targets: ["JSKit"]
        ),
    ],
    targets: [
        .target(
            name: "CQuickJS",
            path: "Sources/CQuickJS",
            publicHeadersPath: "include",
            cSettings: [
                .headerSearchPath("../../Vendor/quickjs-ng"),
                .define("QUICKJS_NG_BUILD"),
                .define("_GNU_SOURCE"),
                .define("WIN32_LEAN_AND_MEAN", .when(platforms: [.windows])),
                .define("_WIN32_WINNT", to: "0x0601", .when(platforms: [.windows])),
            ],
            linkerSettings: [
                .linkedLibrary("dl", .when(platforms: [.linux])),
                .linkedLibrary("pthread", .when(platforms: [.linux])),
                .linkedLibrary("m", .when(platforms: [.linux])),
            ]
        ),
        .target(
            name: "JSKit",
            dependencies: ["CQuickJS"]
        ),
        .testTarget(
            name: "JSKitTests",
            dependencies: ["JSKit"]
        ),
    ]
)
