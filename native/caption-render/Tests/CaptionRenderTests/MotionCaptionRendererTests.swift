import CoreGraphics
import Foundation
import Testing

@testable import CaptionRender

struct MotionCaptionRendererTests {
  static let style = MotionCaptionStyle(
    fontName: "HiraginoSans-W6", fontSize: 60, faceGrow: 0.3, keyline: 1.4, whiteBorder: 6,
    rim: 8, shadowOpacity: 0.7, shadowOffsetY: 6, shadowSigma: 2, gradientTop: "#97D8F7",
    gradientBottom: "#2FB0EE", topLocation: 0.27, bottomLocation: 0.39, outputGamma: 1.961)
  static let title = MotionCaptionTitle(
    text: "口I\n日", startFrame: 2, endFrame: 10, x: 0, y: 0, graphemeFrames: [2, 4, 4, 6])

  static func renderer(
    _ title: MotionCaptionTitle = title, style: MotionCaptionStyle = style
  ) throws -> MotionCaptionRenderer {
    try MotionCaptionRenderer(title, style: style, canvasWidth: 320, canvasHeight: 320)
  }

  static func opaque(_ bitmap: MotionCaptionBitmap) -> Int {
    stride(from: 3, to: bitmap.rgba.count, by: 4).count { bitmap.rgba[$0] == 255 }
  }

  static func pixels(_ bitmap: MotionCaptionBitmap) -> [[UInt8]] {
    stride(from: 0, to: bitmap.rgba.count, by: 4).map { Array(bitmap.rgba[$0..<($0 + 4)]) }
  }

  @Test func typeOnShowsEachGraphemeFromItsOnsetFrame() {
    #expect((0..<12).map(Self.title.visible(at:)) == [0, 0, 1, 1, 3, 3, 4, 4, 4, 4, 0, 0])
  }

  @Test func refusesTitlesWhoseOnsetsDoNotMatchTheirGraphemes() {
    let short = MotionCaptionTitle(
      text: "口I", startFrame: 0, endFrame: 4, x: 0, y: 0, graphemeFrames: [0])
    let late = MotionCaptionTitle(
      text: "口", startFrame: 0, endFrame: 4, x: 0, y: 0, graphemeFrames: [4])
    #expect(throws: CaptionRenderError.invalid("Invalid Motion caption title")) {
      try Self.renderer(short)
    }
    #expect(throws: CaptionRenderError.self) { try Self.renderer(late) }
  }

  @Test func refusesOutlinesThatDoNotNest() {
    let style = MotionCaptionStyle(
      fontName: "HiraginoSans-W6", fontSize: 60, faceGrow: 0, keyline: 7, whiteBorder: 6,
      rim: 8, shadowOpacity: 0.7, shadowOffsetY: 6, shadowSigma: 2, gradientTop: "#97D8F7",
      gradientBottom: "#2FB0EE", topLocation: 0.27, bottomLocation: 0.39, outputGamma: 1.961)
    #expect(throws: CaptionRenderError.invalid("Invalid Motion caption style")) {
      try Self.renderer(style: style)
    }
  }

  @Test func refusesAFontThatIsNotInstalled() {
    let style = MotionCaptionStyle(
      fontName: "NoSuchFont-Synthetic", fontSize: 60, faceGrow: 0, keyline: 1, whiteBorder: 6,
      rim: 8, shadowOpacity: 0.7, shadowOffsetY: 6, shadowSigma: 2, gradientTop: "#97D8F7",
      gradientBottom: "#2FB0EE", topLocation: 0.27, bottomLocation: 0.39, outputGamma: 1.961)
    #expect(
      throws: CaptionRenderError.invalid("Caption font is not installed: NoSuchFont-Synthetic")
    ) {
      try Self.renderer(style: style)
    }
  }

  @Test func refusesATitleThatDrawsNoGlyphs() {
    let blank = MotionCaptionTitle(
      text: " ", startFrame: 0, endFrame: 4, x: 0, y: 0, graphemeFrames: [0])
    #expect(throws: CaptionRenderError.invalid("Caption title draws no glyphs")) {
      try Self.renderer(blank)
    }
  }

  @Test func leftAlignedLinesStartAtThePositionWhereCentredOnesStraddleIt() throws {
    let left = MotionCaptionTitle(
      text: "口I\n日", startFrame: 2, endFrame: 10, x: 0, y: 0, graphemeFrames: [2, 4, 4, 6],
      alignment: .left)
    let started = try Self.renderer(left).bitmap(visible: 4)
    let centred = try Self.renderer().bitmap(visible: 4)
    // The outline and shadow reach a few pixels left of the first glyph's start at x 160.
    #expect((150..<160).contains(started.originX))
    #expect(centred.originX < 150)
    #expect((started.originY, started.height) == (centred.originY, centred.height))
  }

  @Test func revealsGlyphsInTheirFinalLayout() throws {
    let renderer = try Self.renderer()
    let none = try renderer.bitmap(visible: 0)
    let first = try renderer.bitmap(visible: 1)
    let all = try renderer.bitmap(visible: 4)
    #expect(none.rgba.allSatisfy { $0 == 0 })
    #expect(Self.opaque(first) > 0)
    #expect(Self.opaque(all) > Self.opaque(first))
    // Every pixel the first glyph covers is covered once the whole title shows.
    let kept = stride(from: 3, to: first.rgba.count, by: 4).allSatisfy {
      first.rgba[$0] < 255 || all.rgba[$0] == 255
    }
    #expect(kept)
    #expect((all.originX, all.originY) == (first.originX, first.originY))
  }

  @Test func drawsTheWhiteBandBlackOutlinesAndTheEncodedFill() throws {
    let pixels = Self.pixels(try Self.renderer().bitmap(visible: 4))
    #expect(pixels.contains([255, 255, 255, 255]))
    #expect(pixels.contains([0, 0, 0, 255]))
    // #2FB0EE in linear light encoded with gamma 1.961, as Motion writes it.
    #expect(pixels.contains([41, 166, 235, 255]))
    #expect(pixels.contains([140, 210, 246, 255]))
  }

  @Test func growsCoverageByASquare() throws {
    let renderer = try Self.renderer()
    var dot = [Float](repeating: 0, count: renderer.width * renderer.height)
    let centre = (renderer.height / 2) * renderer.width + renderer.width / 2
    dot[centre] = 1
    let grown = renderer.grown(dot, by: 2.5)
    #expect(grown.count { $0 == 1 } == 25)
    #expect(grown.count { $0 == 0.5 } == 20)
    #expect(grown.count { $0 == 0.25 } == 4)
    #expect(renderer.grown(dot, by: 0) == dot)
    #expect(renderer.grown(dot, by: 2).count { $0 == 1 } == 25)
  }

  @Test func easesTheGradientBetweenItsStopsInEachLineBox() throws {
    let renderer = try Self.renderer()
    let top = renderer.gradient(at: renderer.boxTop + 0.1 * renderer.boxHeight)
    let bottom = renderer.gradient(at: renderer.boxTop + 0.9 * renderer.boxHeight)
    let nextLine = renderer.gradient(at: renderer.boxTop + 1.9 * renderer.boxHeight)
    let above = renderer.gradient(at: renderer.boxTop - 0.9 * renderer.boxHeight)
    #expect(bottom == nextLine)
    #expect(top == above)
    #expect(Int((renderer.encode(bottom.0) * 255).rounded()) == 41)
    #expect(Int((renderer.encode(top.0) * 255).rounded()) == 140)
  }

  @Test func holdsTheTopColourWhenTheStopsCoincide() throws {
    let style = MotionCaptionStyle(
      fontName: "HiraginoSans-W6", fontSize: 60, faceGrow: 0.3, keyline: 1.4, whiteBorder: 6,
      rim: 8, shadowOpacity: 0.7, shadowOffsetY: 6, shadowSigma: 2, gradientTop: "#97D8F7",
      gradientBottom: "#2FB0EE", topLocation: 0.5, bottomLocation: 0.5, outputGamma: 1.961)
    let renderer = try Self.renderer(style: style)
    #expect(
      Int((renderer.encode(renderer.gradient(at: renderer.boxTop + 1).0) * 255).rounded()) == 140)
  }

  @Test func imagesAreStraightAlphaTaggedBT709WithTheBitmapBytes() throws {
    let bitmap = try Self.renderer().bitmap(visible: 4)
    let image = try bitmap.image()
    #expect((image.width, image.height) == (bitmap.width, bitmap.height))
    #expect(image.alphaInfo == .last)
    #expect(image.colorSpace == MotionCaptionBitmap.videoColorSpace)
    let data = try #require(image.dataProvider?.data as Data?)
    #expect([UInt8](data) == bitmap.rgba)
    #expect(
      bitmap.rect
        == CGRect(x: bitmap.originX, y: bitmap.originY, width: bitmap.width, height: bitmap.height))
  }

  @Test func videoColorSpaceDecodesCodeValuesWithThePowerLawVideoUses() throws {
    let space = try #require(MotionCaptionBitmap.videoColorSpace)
    let linear = try #require(CGColorSpace(name: CGColorSpace.extendedLinearSRGB))
    let grey = try #require(CGColor(colorSpace: space, components: [0.5, 0.5, 0.5, 1]))
    let decoded = try #require(
      grey.converted(to: linear, intent: .relativeColorimetric, options: nil)?.components)
    // 0.5^1.961 = 0.2569; BT.1886 (gamma 2.4, CGColorSpace.itur_709) would give 0.1895.
    #expect(abs(decoded[1] - 0.2569) < 0.001)
  }

  @Test func describesErrorsAsProAppsCoreDoes() {
    #expect("\(CaptionRenderError.invalid("a"))" == "Invalid request: a")
    #expect("\(CaptionRenderError.unavailable("b"))" == "Unavailable: b")
  }

  @Test func refusesAnImageLargerThanItsBytes() {
    #expect(throws: CaptionRenderError.unavailable("Cannot make a caption image")) {
      try MotionCaptionBitmap.image([0, 0, 0, 0], width: 2, height: 2)
    }
  }
}
