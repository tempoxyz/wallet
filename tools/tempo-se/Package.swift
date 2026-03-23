// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "tempo-se",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(name: "tempo-se", path: "Sources")
    ]
)
