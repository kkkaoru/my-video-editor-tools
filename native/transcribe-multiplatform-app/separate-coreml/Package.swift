// swift-tools-version: 6.0
import PackageDescription

let package = Package(
  name: "separate-coreml",
  platforms: [.macOS(.v13)],
  targets: [
    .target(name: "SeparationCore"),
    .executableTarget(name: "separate-coreml", dependencies: ["SeparationCore"]),
    .testTarget(name: "SeparationCoreTests", dependencies: ["SeparationCore"], exclude: ["make_fixtures.py"]),
  ]
)
