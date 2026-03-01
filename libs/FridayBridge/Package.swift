// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "FridayBridge",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .library(name: "FridayBridge", type: .dynamic, targets: ["FridayBridge"])
    ],
    dependencies: [
        .package(path: "../CoreFriday"),
        .package(path: "../JSKit")
    ],
    targets: [
        .target(
            name: "FridayBridge",
            dependencies: [
                .product(name: "CoreFriday", package: "CoreFriday"),
                .product(name: "JSKit", package: "JSKit")
            ]
        )
    ]
)
