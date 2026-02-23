// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "FridayAPI",
    platforms: [
        .macOS(.v12),
        .iOS(.v15)
    ],
    products: [
        .library(name: "FridayAPI", targets: ["FridayAPI"])
    ],
    targets: [
        .target(name: "FridayAPI")
    ]
)
