// swift-tools-version: 6.2
import PackageDescription

// The style-20 caption renderer shared by the burned-caption path and the Viewer; only system
// frameworks, so the Viewer can draw the same captions without ProAppsCore's dependencies.
let package = Package(
  name: "CaptionRender",
  platforms: [.macOS(.v15)],
  products: [
    .library(name: "CaptionRender", targets: ["CaptionRender"]),
    .executable(name: "caption-burn", targets: ["CaptionBurn"]),
  ],
  targets: [
    .target(name: "CaptionRender"),
    .executableTarget(name: "CaptionBurn", dependencies: ["CaptionRender"]),
    .testTarget(name: "CaptionRenderTests", dependencies: ["CaptionRender"]),
  ],
  swiftLanguageModes: [.v6]
)
