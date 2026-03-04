// swift-tools-version: 5.9
// This file is the single source of truth for all external Swift package
// dependencies in the monorepo. It is used by rules_swift_package_manager
// to generate Bazel targets for each package.

import PackageDescription

let package = Package(
    name: "friday",
    dependencies: [
        .package(url: "https://github.com/vapor/fluent.git", exact: "4.13.0"),
        .package(url: "https://github.com/vapor/fluent-sqlite-driver.git", exact: "4.8.1"),
        .package(url: "https://github.com/swiftlang/swift-syntax.git", exact: "602.0.0"),
        .package(url: "https://github.com/jpsim/Yams", from: "5.4.0"),
        .package(url: "https://github.com/swiftlang/swift-markdown", from: "0.7.3"),
    ],
    targets: []
)
