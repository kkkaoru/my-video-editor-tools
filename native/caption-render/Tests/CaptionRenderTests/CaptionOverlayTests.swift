import CoreGraphics
import Foundation
import Testing

@testable import CaptionRender

struct CaptionOverlayTests {
  static let style = MotionCaptionRendererTests.style
  static let first = MotionCaptionTitle(
    text: "口I", startFrame: 0, endFrame: 4, x: 0, y: 0, graphemeFrames: [0, 2])
  static let second = MotionCaptionTitle(
    text: "日", startFrame: 2, endFrame: 6, x: 0, y: -60, graphemeFrames: [2])

  static func overlay(fill: CaptionFill? = nil) throws -> CaptionOverlay {
    try CaptionOverlay(
      titles: [first, second], style: style, fill: fill, canvasWidth: 320, canvasHeight: 320)
  }

  @Test func singleFillUsesTheTopColour() throws {
    let single = try Self.style.applying(
      CaptionFill(top: "#FF0000", bottom: "#0000FF", gradient: false))
    let both = try Self.style.applying(
      CaptionFill(top: "#FF0000", bottom: "#0000FF", gradient: true))
    #expect((single.gradientTop, single.gradientBottom) == ("#FF0000", "#FF0000"))
    #expect((both.gradientTop, both.gradientBottom) == ("#FF0000", "#0000FF"))
    #expect(single.fontSize == Self.style.fontSize)
    #expect(throws: CaptionRenderError.invalid("Invalid Motion caption style")) {
      try Self.style.applying(CaptionFill(top: "red", bottom: "#0000FF", gradient: false))
    }
  }

  @Test func singleFillDrawsOneColourOnEveryFaceRow() throws {
    let fill = CaptionFill(top: "#FF0000", bottom: "#0000FF", gradient: false)
    var overlay = try Self.overlay(fill: fill)
    let pixels = MotionCaptionRendererTests.pixels(try #require(overlay.layers(at: 3).first).bitmap)
    #expect(pixels.contains([255, 0, 0, 255]))
    #expect(!pixels.contains([0, 0, 255, 255]))
  }

  @Test func keepsOnlyTheCurrentStateOfEachVisibleTitle() throws {
    var overlay = try Self.overlay()
    #expect(try overlay.layers(at: 0).count == 1)
    #expect(try overlay.layers(at: 1).count == 1)
    #expect(overlay.drawn == 1)
    let both = try overlay.layers(at: 2)
    #expect(both.count == 2)
    #expect(overlay.drawn == 3)
    #expect(both[0].rect == both[0].bitmap.rect)
    #expect(try overlay.layers(at: 3).count == 2)
    #expect(overlay.drawn == 3)
    #expect(try overlay.layers(at: 4).count == 1)
    #expect(overlay.states.keys.sorted() == [1])
    #expect(try overlay.layers(at: 9).isEmpty)
    #expect(overlay.states.isEmpty)
    // Seeking back lays the title out again.
    #expect(try overlay.layers(at: 0).count == 1)
    #expect(overlay.drawn == 4)
  }

  @Test func drawsAFrameAsTheMovieComposesIt() throws {
    var overlay = try Self.overlay()
    let space = try #require(MotionCaptionBitmap.videoColorSpace)
    let context = try #require(
      CGContext(
        data: nil, width: 320, height: 320, bitsPerComponent: 8, bytesPerRow: 320 * 4,
        space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
    try overlay.draw(frame: 3, in: context)
    let image = try #require(context.makeImage())
    let bytes = [UInt8](try #require(image.dataProvider?.data as Data?))
    var expectedOverlay = try Self.overlay()
    let straight = CaptionFrame.composite(
      try expectedOverlay.layers(at: 3).map(\.bitmap), width: 320, height: 320)
    // Opaque pixels survive premultiplication unchanged.
    let opaque = stride(from: 0, to: straight.count, by: 4).filter { straight[$0 + 3] == 255 }
    #expect(!opaque.isEmpty)
    #expect(opaque.allSatisfy { Array(bytes[$0..<($0 + 4)]) == Array(straight[$0..<($0 + 4)]) })
  }

  @Test func drawsEachSpeakersTitlesInItsOwnColours() throws {
    let red = CaptionFill(top: "#FF0000", bottom: "#FF0000", gradient: false)
    let green = CaptionFill(top: "#00FF00", bottom: "#00FF00", gradient: false)
    let titles = [
      MotionCaptionTitle(
        text: "口", startFrame: 0, endFrame: 4, x: -80, y: 0, graphemeFrames: [0], speaker: "a"),
      MotionCaptionTitle(
        text: "口", startFrame: 0, endFrame: 4, x: 80, y: 0, graphemeFrames: [0], speaker: "b"),
      MotionCaptionTitle(text: "口", startFrame: 0, endFrame: 4, x: 0, y: -90, graphemeFrames: [0]),
    ]
    var overlay = try CaptionOverlay(
      titles: titles, style: Self.style, fill: green, speakerFills: ["a": red],
      canvasWidth: 320, canvasHeight: 320)
    let pixels = try overlay.layers(at: 0).map { MotionCaptionRendererTests.pixels($0.bitmap) }
    #expect(pixels[0].contains([255, 0, 0, 255]) && !pixels[0].contains([0, 255, 0, 255]))
    #expect(pixels[1].contains([0, 255, 0, 255]) && !pixels[1].contains([255, 0, 0, 255]))
    #expect(pixels[2].contains([0, 255, 0, 255]))
    var plain = try CaptionOverlay(
      titles: titles, style: Self.style, speakerFills: ["a": red], canvasWidth: 320,
      canvasHeight: 320)
    let unfilled = try plain.layers(at: 0).map { MotionCaptionRendererTests.pixels($0.bitmap) }
    // Without `fill`, other speakers keep the style's own gradient.
    #expect(!unfilled[1].contains([0, 255, 0, 255]) && !unfilled[1].contains([255, 0, 0, 255]))
    #expect(unfilled[1] != pixels[1] && unfilled[0] == pixels[0])
    #expect(throws: CaptionRenderError.self) {
      try CaptionOverlay(
        titles: titles, style: Self.style,
        speakerFills: ["a": CaptionFill(top: "red", bottom: "#000000", gradient: true)],
        canvasWidth: 320, canvasHeight: 320)
    }
  }

  @Test func refusesInvalidInput() {
    let bad = MotionCaptionTitle(
      text: "", startFrame: 0, endFrame: 1, x: 0, y: 0, graphemeFrames: [])
    #expect(throws: CaptionRenderError.self) {
      try CaptionOverlay(titles: [bad], style: Self.style, canvasWidth: 320, canvasHeight: 320)
    }
    #expect(throws: CaptionRenderError.self) {
      try Self.overlay(fill: CaptionFill(top: "#FF0000", bottom: "nope", gradient: true))
    }
  }
}

struct CaptionFrameTests {
  static func bitmap(x: Int, y: Int, _ rgba: [UInt8]) -> MotionCaptionBitmap {
    MotionCaptionBitmap(originX: x, originY: y, width: 1, height: 1, rgba: rgba)
  }

  @Test func composesOverlappingTitlesWithStraightAlpha() {
    let canvas = CaptionFrame.composite(
      [Self.bitmap(x: 0, y: 0, [30, 20, 10, 128]), Self.bitmap(x: 0, y: 0, [200, 100, 0, 128])],
      width: 1, height: 1)
    #expect(canvas == [143, 73, 3, 192])
  }

  @Test func copiesTheFirstLayerAndClipsToTheCanvas() throws {
    let canvas = CaptionFrame.composite(
      [
        Self.bitmap(x: 1, y: 0, [9, 8, 7, 6]), Self.bitmap(x: 2, y: 0, [1, 1, 1, 1]),
        Self.bitmap(x: 0, y: 1, [1, 1, 1, 1]), Self.bitmap(x: 0, y: 0, [5, 5, 5, 0]),
      ], width: 2, height: 1)
    #expect(canvas == [0, 0, 0, 0, 9, 8, 7, 6])
    let image = try CaptionFrame.image(
      [Self.bitmap(x: 0, y: 0, [9, 8, 7, 6])], width: 2, height: 1)
    #expect((image.width, image.height, image.alphaInfo) == (2, 1, .last))
  }
}
