import CaptionRender
import Foundation
import Testing

@testable import ProAppsCore

/// The movie writer around the shared renderer (whose own tests live in native/caption-render).
struct MotionCaptionRendererTests {
  static let style = MotionCaptionStyle(
    fontName: "HiraginoSans-W6", fontSize: 60, faceGrow: 0.3, keyline: 1.4, whiteBorder: 6,
    rim: 8, shadowOpacity: 0.7, shadowOffsetY: 6, shadowSigma: 2, gradientTop: "#97D8F7",
    gradientBottom: "#2FB0EE", topLocation: 0.27, bottomLocation: 0.39, outputGamma: 1.961)
  static let title = MotionCaptionTitle(
    text: "口I\n日", startFrame: 2, endFrame: 10, x: 0, y: 0, graphemeFrames: [2, 4, 4, 6])

  @Test func writesAProResAlphaMovieOnceAndReportsItsStates() async throws {
    let directory = try Files.temporaryDirectory().appendingPathComponent(
      "motion-caption-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: directory) }
    let request = MotionCaptionRequest(
      output: directory.appendingPathComponent("captions.mov").path, width: 320, height: 320,
      rate: 30, frames: 12, style: Self.style, titles: [Self.title])
    let report = try await MotionCaptionMovie.render(request)
    #expect((report.frames, report.states) == (12, 3))
    #expect(FileManager.default.fileExists(atPath: request.output))
    await #expect(throws: ProAppsError.self) { try await MotionCaptionMovie.render(request) }
  }

  @Test func refusesTitlesBeyondTheMovie() async {
    let request = MotionCaptionRequest(
      output: "/synthetic/never.mov", width: 320, height: 320, rate: 30, frames: 5,
      style: Self.style, titles: [Self.title])
    await #expect(throws: ProAppsError.self) { try await MotionCaptionMovie.render(request) }
  }

  @Test func readsTheRequestNamedOnTheCommandLine() throws {
    let directory = try Files.temporaryDirectory().appendingPathComponent(
      "motion-caption-request-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: directory) }
    let request = MotionCaptionRequest(
      output: directory.appendingPathComponent("captions.mov").path, width: 320, height: 320,
      rate: 30, frames: 12, style: Self.style, titles: [Self.title])
    let file = directory.appendingPathComponent("request.json")
    try JSONEncoder().encode(request).write(to: file)
    #expect(
      try MotionCaptionMovie.command(arguments: ["caption-alpha-render", file.path])
        == .render(request))
    #expect(throws: ProAppsError.self) {
      try MotionCaptionMovie.command(arguments: ["caption-alpha-render"])
    }
    #expect(throws: ProAppsError.self) {
      try MotionCaptionMovie.command(arguments: ["caption-alpha-render", "--inspect"])
    }
    #expect(throws: ProAppsError.self) {
      try MotionCaptionMovie.command(
        arguments: ["caption-alpha-render", directory.appendingPathComponent("missing").path])
    }
  }
}
