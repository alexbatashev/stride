// swift-tools-version: 6.2

import PackageDescription

let package = Package(
    name: "jskit-test",
    products: [
        .executable(name: "jskit-test", targets: ["jskit-test"]),
    ],
    dependencies: [
        .package(path: "../../libs/JSKit"),
    ],
    targets: [
        .executableTarget(
            name: "jskit-test",
            dependencies: [
                .product(name: "JSKit", package: "JSKit"),
            ]
        ),
    ]
)
