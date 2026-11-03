import CoreGraphics
import CryptoKit
import Foundation
import ImageIO
import Testing

@testable import CaptionRender

struct CaptionNameplateRendererTests {
  /// A system variable font with a weight axis, and a static one without.
  static let variable = URL(fileURLWithPath: "/System/Library/Fonts/SFNS.ttf")
  static let fixed = URL(fileURLWithPath: "/System/Library/Fonts/Supplemental/Arial.ttf")

  static func digest(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
  }

  /// A solid red PNG icon, or a mid-grey greyscale one.
  static func icon(size: Int = 40, grey: Bool = false) throws -> Data {
    let space = try #require(
      CGColorSpace(name: grey ? CGColorSpace.linearGray : CGColorSpace.sRGB))
    let context = try #require(
      CGContext(
        data: nil, width: size, height: size, bitsPerComponent: 8,
        bytesPerRow: size * (grey ? 1 : 4), space: space,
        bitmapInfo: grey
          ? CGImageAlphaInfo.none.rawValue : CGImageAlphaInfo.premultipliedLast.rawValue))
    if grey {
      context.setFillColor(gray: 0.5, alpha: 1)
    } else {
      context.setFillColor(red: 1, green: 0, blue: 0, alpha: 1)
    }
    context.fill(CGRect(x: 0, y: 0, width: size, height: size))
    let image = try #require(context.makeImage())
    let data = NSMutableData()
    let destination = try #require(
      CGImageDestinationCreateWithData(data, "public.png" as CFString, 1, nil))
    CGImageDestinationAddImage(destination, image, nil)
    #expect(CGImageDestinationFinalize(destination))
    return data as Data
  }

  static func plate(
    icon: Data, font: Data, nameX: Double = 60, variation: String = "Bold"
  ) -> CaptionNameplate {
    CaptionNameplate(
      burned: false, icon: "a.png", iconSha256: digest(icon), iconX: 10, iconY: 20, iconSize: 100,
      ringWidth: 4, ringColour: "#FFFFFF", name: "Name", nameFont: "SFNS.ttf",
      nameFontSha256: digest(font), nameFontVariation: variation, nameSize: 36,
      nameColour: "#FFFFFF", nameStrokeWidth: 5, nameStrokeColour: "#000000", nameX: nameX,
      nameY: 126, nameAnchor: "mt")
  }

  /// RGBA bytes at (x, y), top-left origin.
  static func pixel(_ image: CGImage, _ x: Int, _ y: Int) throws -> [UInt8] {
    let data = [UInt8](try #require(image.dataProvider?.data as Data?))
    let offset = y * image.bytesPerRow + x * 4
    return Array(data[offset..<(offset + 4)])
  }

  @Test func drawsTheIconInsideARingAndTheStrokedNameBelow() throws {
    let icon = try Self.icon()
    let font = try Data(contentsOf: Self.variable)
    let image = try CaptionNameplateRenderer.image(
      Self.plate(icon: icon, font: font), icon: icon, font: font, canvasWidth: 200,
      canvasHeight: 200)
    #expect((image.width, image.height) == (200, 200))
    #expect(image.colorSpace == MotionCaptionBitmap.videoColorSpace)
    #expect(try Self.pixel(image, 60, 70) == [255, 0, 0, 255])  // icon centre
    #expect(try Self.pixel(image, 12, 70) == [255, 255, 255, 255])  // inside the 4 px ring
    #expect(try Self.pixel(image, 5, 70) == [0, 0, 0, 0])  // outside the disc
    #expect(try Self.pixel(image, 11, 21) == [0, 0, 0, 0])  // disc corner
    // The name below the disc holds white fill and black stroke.
    var colours: Set<[UInt8]> = []
    var ink = (left: 200, top: 200, right: 0, bottom: 0)
    for y in 121..<200 {
      for x in 0..<200 {
        let pixel = try Self.pixel(image, x, y)
        colours.insert(pixel)
        guard pixel[3] > 0 else { continue }
        ink = (min(ink.left, x), min(ink.top, y), max(ink.right, x + 1), max(ink.bottom, y + 1))
      }
    }
    #expect(colours.contains([255, 255, 255, 255]))
    #expect(colours.contains([0, 0, 0, 255]))
    // caption_layout's Pillow drawing of this plate and font: ink box (8, 121, 113, 156).
    #expect(abs(ink.left - 8) <= 1 && abs(ink.right - 113) <= 1)
    #expect(abs(ink.top - 121) <= 1 && abs(ink.bottom - 156) <= 1)
  }

  @Test func bitmapsAreStraightAlpha() throws {
    let icon = try Self.icon()
    let font = try Data(contentsOf: Self.variable)
    let bitmap = try CaptionNameplateRenderer.bitmap(
      Self.plate(icon: icon, font: font), icon: icon, font: font, canvasWidth: 200,
      canvasHeight: 200)
    #expect((bitmap.originX, bitmap.originY, bitmap.width, bitmap.height) == (0, 0, 200, 200))
    // The ring's anti-aliased outer edge keeps its white once unpremultiplied.
    let edge = (70 * 200 + 10) * 4
    #expect((1..<255).contains(bitmap.rgba[edge + 3]))
    #expect(bitmap.rgba[edge..<(edge + 3)].allSatisfy { $0 >= 250 })
  }

  @Test func colourConvertsAGreyscaleIcon() throws {
    let icon = try Self.icon(grey: true)
    let font = try Data(contentsOf: Self.variable)
    let image = try CaptionNameplateRenderer.image(
      Self.plate(icon: icon, font: font), icon: icon, font: font, canvasWidth: 200,
      canvasHeight: 200)
    let centre = try Self.pixel(image, 60, 70)
    #expect(centre[0] == centre[1] && centre[1] == centre[2] && centre[3] == 255)
    #expect((100..<250).contains(centre[0]))
  }

  @Test func refusesFilesWithoutTheirRecordedDigest() throws {
    let icon = try Self.icon()
    let font = try Data(contentsOf: Self.variable)
    let plate = Self.plate(icon: icon, font: font)
    #expect(throws: CaptionRenderError.invalid("a.png does not have its recorded SHA-256")) {
      try CaptionNameplateRenderer.image(
        plate, icon: Data(), font: font, canvasWidth: 200, canvasHeight: 200)
    }
    #expect(throws: CaptionRenderError.invalid("SFNS.ttf does not have its recorded SHA-256")) {
      try CaptionNameplateRenderer.image(
        plate, icon: icon, font: icon, canvasWidth: 200, canvasHeight: 200)
    }
  }

  @Test func refusesANameOutsideTheCanvas() throws {
    let icon = try Self.icon()
    let font = try Data(contentsOf: Self.variable)
    #expect(
      throws: CaptionRenderError.invalid("The caption nameplate's name does not fit its canvas")
    ) {
      try CaptionNameplateRenderer.image(
        Self.plate(icon: icon, font: font, nameX: 5), icon: icon, font: font, canvasWidth: 200,
        canvasHeight: 200)
    }
    #expect(throws: CaptionRenderError.self) {
      try CaptionNameplateRenderer.image(
        Self.plate(icon: icon, font: font), icon: icon, font: font, canvasWidth: 200,
        canvasHeight: 140)
    }
  }

  @Test func refusesFontsWithoutTheBoldWeight() throws {
    let icon = try Self.icon()
    let fixed = try Data(contentsOf: Self.fixed)
    let variable = try Data(contentsOf: Self.variable)
    #expect(throws: CaptionRenderError.invalid("SFNS.ttf has no weight axis")) {
      try CaptionNameplateRenderer.image(
        Self.plate(icon: icon, font: fixed), icon: icon, font: fixed, canvasWidth: 200,
        canvasHeight: 200)
    }
    #expect(throws: CaptionRenderError.invalid("Unsupported nameplate font variation")) {
      try CaptionNameplateRenderer.image(
        Self.plate(icon: icon, font: variable, variation: "Thin"), icon: icon, font: variable,
        canvasWidth: 200, canvasHeight: 200)
    }
  }

  @Test func refusesUnreadableIconsAndFonts() throws {
    let icon = try Self.icon()
    let junk = Data("not an image".utf8)
    let font = try Data(contentsOf: Self.variable)
    #expect(throws: CaptionRenderError.invalid("a.png is not a readable image")) {
      try CaptionNameplateRenderer.image(
        Self.plate(icon: junk, font: font), icon: junk, font: font, canvasWidth: 200,
        canvasHeight: 200)
    }
    #expect(throws: CaptionRenderError.invalid("SFNS.ttf is not a readable font")) {
      try CaptionNameplateRenderer.image(
        Self.plate(icon: icon, font: junk), icon: icon, font: junk, canvasWidth: 200,
        canvasHeight: 200)
    }
  }

  @Test func refusesAnInvalidPlate() throws {
    let icon = try Self.icon()
    let font = try Data(contentsOf: Self.variable)
    let base = Self.plate(icon: icon, font: font)
    let bad = CaptionNameplate(
      burned: false, icon: "a.png", iconSha256: base.iconSha256, iconX: 10, iconY: 20,
      iconSize: 100, ringWidth: 4, ringColour: "white", name: "Name", nameFont: "SFNS.ttf",
      nameFontSha256: base.nameFontSha256, nameFontVariation: "Bold", nameSize: 36,
      nameColour: "#FFFFFF", nameStrokeWidth: 5, nameStrokeColour: "#000000", nameX: 60,
      nameY: 126, nameAnchor: "mt")
    #expect(throws: CaptionRenderError.invalid("Invalid caption nameplate")) {
      try CaptionNameplateRenderer.image(
        bad, icon: icon, font: font, canvasWidth: 200, canvasHeight: 200)
    }
  }
}
