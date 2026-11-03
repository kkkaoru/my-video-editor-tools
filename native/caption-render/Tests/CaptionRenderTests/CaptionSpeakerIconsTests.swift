import CoreGraphics
import Foundation
import Testing

@testable import CaptionRender

struct CaptionSpeakerIconsTests {
  struct Expected: Decodable {
    let icons: [CaptionSpeakerIcons.Placement]
  }

  static func icons() throws -> [String: Data] {
    [
      "left": try SpeakerFixture.data("synthetic-multi.speaker-left.png"),
      "right": try SpeakerFixture.data("synthetic-multi.speaker-right.png"),
    ]
  }

  static func pixel(_ bitmap: MotionCaptionBitmap, _ x: Int, _ y: Int) -> [UInt8] {
    let offset = (y * bitmap.width + x) * 4
    return Array(bitmap.rgba[offset..<(offset + 4)])
  }

  @Test func placesIconsAsTheSharedFixtureExpects() throws {
    let expected = try JSONDecoder().decode(
      Expected.self, from: SpeakerFixture.data("caption-layout-expected-v2.json"))
    #expect(CaptionSpeakerIcons.placements(try SpeakerFixture.plan()) == expected.icons)
    #expect(
      expected.icons == [
        .init(id: "left", x: 607, y: 972, size: 86, ring: 2),
        .init(id: "right", x: 1607, y: 972, size: 86, ring: 2),
      ])
  }

  @Test func drawsEachShownIconInAWhiteRing() throws {
    let plan = try SpeakerFixture.plan()
    let all = try #require(try CaptionSpeakerIcons.bitmap(plan, icons: Self.icons()))
    #expect((all.width, all.height) == (1920, 1080))
    #expect(Self.pixel(all, 650, 1015) == [220, 60, 60, 255])  // left icon centre
    #expect(Self.pixel(all, 1650, 1015) == [60, 90, 220, 255])  // right icon centre
    #expect(Self.pixel(all, 608, 1015) == [255, 255, 255, 255])  // left ring
    #expect(Self.pixel(all, 1140, 1015) == [0, 0, 0, 0])  // middle has no icon
    let left = try #require(
      try CaptionSpeakerIcons.bitmap(plan, icons: Self.icons(), showing: ["left", "middle"]))
    #expect(Self.pixel(left, 1650, 1015) == [0, 0, 0, 0])
    #expect(try CaptionSpeakerIcons.bitmap(plan, icons: [:], showing: ["middle"]) == nil)
    #expect(try CaptionSpeakerIcons.bitmap(PlacementFixture.plan(), icons: [:]) == nil)
  }

  @Test func refusesMissingOrChangedIcons() throws {
    let plan = try SpeakerFixture.plan()
    #expect(throws: CaptionRenderError.invalid("No icon file for speaker right")) {
      try CaptionSpeakerIcons.bitmap(plan, icons: ["left": try Self.icons()["left"] ?? Data()])
    }
    #expect(
      throws: CaptionRenderError.invalid(
        "synthetic-multi.speaker-left.png does not have its recorded SHA-256")
    ) {
      try CaptionSpeakerIcons.bitmap(plan, icons: ["left": Data(), "right": Data()])
    }
  }

  @Test func layersTheNameplateAndIconsIntoOneUnderlay() {
    let a = MotionCaptionBitmap(
      originX: 0, originY: 0, width: 2, height: 1, rgba: [9, 9, 9, 255, 0, 0, 0, 0])
    let b = MotionCaptionBitmap(
      originX: 0, originY: 0, width: 2, height: 1, rgba: [0, 0, 0, 0, 7, 7, 7, 255])
    #expect(CaptionUnderlay.bitmap(nameplate: nil, icons: nil, width: 2, height: 1) == nil)
    #expect(CaptionUnderlay.bitmap(nameplate: a, icons: nil, width: 2, height: 1)?.rgba == a.rgba)
    #expect(
      CaptionUnderlay.bitmap(nameplate: a, icons: b, width: 2, height: 1)?.rgba
        == [9, 9, 9, 255, 7, 7, 7, 255])
  }

  @Test func overlaysDrawTheUnderlayBelowTheCaptions() throws {
    let plan = try SpeakerFixture.plan()
    let icons = try CaptionSpeakerIcons.bitmap(plan, icons: Self.icons())
    var overlay = try CaptionOverlay(plan: plan, layout: .plan, underlay: icons)
    let space = try #require(MotionCaptionBitmap.videoColorSpace)
    let context = try #require(
      CGContext(
        data: nil, width: 1920, height: 1080, bitsPerComponent: 8, bytesPerRow: 1920 * 4,
        space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
    try overlay.draw(frame: 250, in: context)
    let image = try #require(context.makeImage())
    let bytes = [UInt8](try #require(image.dataProvider?.data as Data?))
    let centre = (1015 * 1920 + 650) * 4
    #expect(Array(bytes[centre..<(centre + 4)]) == [220, 60, 60, 255])
  }
}

extension MotionCaptionBitmap: Equatable {
  public static func == (lhs: MotionCaptionBitmap, rhs: MotionCaptionBitmap) -> Bool {
    (lhs.originX, lhs.originY, lhs.width, lhs.height) == (
      rhs.originX, rhs.originY, rhs.width, rhs.height
    )
      && lhs.rgba == rhs.rgba
  }
}
