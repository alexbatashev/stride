// swift-tools-version: 6.2
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "friday",
    platforms: [
        .macOS(.v14)
    ],
    dependencies: [
        .package(path: "../libs/CoreFriday")
    ],
    targets: [
        // Targets are the basic building blocks of a package, defining a module or a test suite.
        // Targets can depend on other targets in this package and products from dependencies.
        .executableTarget(
            name: "friday",
            dependencies: [
                .product(name: "CoreFriday", package: "CoreFriday")
            ],
            swiftSettings: [
                .enableUpcomingFeature("ApproachableConcurrency")
            ],
        ),
        .testTarget(
            name: "fridayTests",
            dependencies: ["friday"],
            swiftSettings: [
                .enableUpcomingFeature("ApproachableConcurrency")
            ],
        ),
    ],
    swiftLanguageModes: [.v6]
)
