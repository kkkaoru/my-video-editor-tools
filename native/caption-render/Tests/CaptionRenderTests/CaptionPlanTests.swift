import Foundation
import Testing

@testable import CaptionRender

struct CaptionPlanTests {
  /// The shared contract fixture that `replay.caption_plan` tests also read.
  static let fixture = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appendingPathComponent("tests/fixtures/caption-plan-v1.json")

  static let style = """
    {"fontName": "HiraginoSans-W6", "fontSize": 60, "faceGrow": 0.3, "keyline": 1.4,
     "whiteBorder": 6, "rim": 8, "shadowOpacity": 0.7, "shadowOffsetY": 6, "shadowSigma": 2,
     "gradientTop": "#97D8F7", "gradientBottom": "#2FB0EE", "topLocation": 0.27,
     "bottomLocation": 0.39, "outputGamma": 1.961}
    """

  static let nameplate = """
    {"burned": true, "icon": "a.nameplate.png", "iconSha256": "\(String(repeating: "a", count: 64))",
     "iconX": 10, "iconY": 20, "iconSize": 100, "ringWidth": 4, "ringColour": "#FFFFFF",
     "name": "名前", "nameFont": "Font.ttf", "nameFontSha256": "\(String(repeating: "0", count: 64))",
     "nameFontVariation": "Bold", "nameSize": 36, "nameColour": "#FFFFFF",
     "nameStrokeWidth": 5, "nameStrokeColour": "#000000", "nameX": 60.0, "nameY": 126,
     "nameAnchor": "mt"}
    """

  static func plan(
    version: Int = 1, movie: String = "a.mp4", frames: Int = 30, titles: String? = nil,
    nameplate: String = "null"
  ) -> Data {
    let titles =
      titles ?? """
        [{"text": "口I", "startFrame": 0, "endFrame": 10, "x": 0, "y": 0,
          "graphemeFrames": [0, 4], "speaker": "a"},
         {"text": "日", "startFrame": 10, "endFrame": 20, "x": 0, "y": 0, "graphemeFrames": [10],
          "alignment": "left", "style": \(style)}]
        """
    return Data(
      """
      {"version": \(version), "clock": "delivery", "movie": "\(movie)", "burned": false,
       "width": 320, "height": 240, "rate": 30, "frames": \(frames), "style": \(style),
       "titles": \(titles), "nameplate": \(nameplate)}
      """.utf8)
  }

  @Test func decodesTheSharedFixture() throws {
    let plan = try CaptionPlan.decode(Data(contentsOf: Self.fixture))
    #expect((plan.version, plan.clock, plan.burned) == (1, "delivery", false))
    #expect((plan.width, plan.height, plan.rate, plan.frames) == (1920, 1080, 30, 300))
    #expect(plan.style.fontName == "NotoSansJP-Thin_Bold")
    #expect(plan.titles.first?.alignment == .left)
    let nameplate = try #require(plan.nameplate)
    #expect((nameplate.iconSize, nameplate.ringWidth, nameplate.nameAnchor) == (150, 4, "mt"))
    var overlay = try CaptionOverlay(plan: plan)
    #expect(try overlay.layers(at: 18).count == 1)
  }

  @Test func decodesOptionalTitleFieldsAndTheNameplate() throws {
    let plan = try CaptionPlan.decode(Self.plan(nameplate: Self.nameplate))
    #expect(plan.titles.map(\.speaker) == ["a", nil])
    #expect(plan.titles[1].style?.fontSize == 60)
    #expect(plan.nameplate?.name == "名前")
    #expect(try CaptionPlan.decode(Self.plan()).nameplate == nil)
  }

  @Test(arguments: [
    plan(version: 2), plan(movie: "dir/a.mp4"), plan(frames: 15),
    plan(
      titles: """
        [{"text": "口", "startFrame": 5, "endFrame": 10, "x": 0, "y": 0, "graphemeFrames": [5]},
         {"text": "日", "startFrame": 8, "endFrame": 12, "x": 0, "y": 0, "graphemeFrames": [8]}]
        """),
    plan(
      titles: """
        [{"text": "口", "startFrame": 5, "endFrame": 10, "x": 0, "y": 0, "graphemeFrames": [4]}]
        """),
    plan(nameplate: nameplate.replacingOccurrences(of: "\"mt\"", with: "\"lt\"")),
    plan(nameplate: nameplate.replacingOccurrences(of: "aaaa", with: "AAAA")),
    plan(nameplate: nameplate.replacingOccurrences(of: #""burned": true, "#, with: "")),
    plan(
      titles: """
        [{"text": "口", "startFrame": 5, "endFrame": 10, "x": 0, "y": 0, "graphemeFrames": [5],
          "colour": "red"}]
        """),
    Data(
      String(decoding: plan(), as: UTF8.self).replacingOccurrences(
        of: #""version": 1,"#, with: #""version": 1, "extra": 0,"#
      ).utf8),
    Data(
      String(decoding: plan(), as: UTF8.self).replacingOccurrences(
        of: #""outputGamma": 1.961}"#, with: #""outputGamma": 1.961, "x": 1}"#
      ).utf8),
  ])
  func refusesPlansThatBreakTheContract(_ data: Data) {
    #expect(throws: CaptionRenderError.self) { try CaptionPlan.decode(data) }
  }

  @Test func replacesTitlePositionsForAPlacement() throws {
    let plan = try CaptionPlan.decode(Self.plan())
    let moved = try plan.replacing(titles: plan.titles.map { $0.moved(x: 12, y: -34) })
    #expect(moved.titles.map(\.x) == [12, 12] && moved.titles.map(\.y) == [-34, -34])
    #expect(moved.titles[0].speaker == "a" && moved.titles[1].alignment == .left)
    #expect(moved.titles.map(\.graphemeFrames) == plan.titles.map(\.graphemeFrames))
    #expect(moved.nameplate == plan.nameplate && moved.style == plan.style)
    #expect(throws: CaptionRenderError.self) {
      try plan.replacing(titles: plan.titles.reversed())
    }
  }

  @Test func refusesUnreadableJSON() {
    #expect(throws: CaptionRenderError.self) { try CaptionPlan.decode(Data("{}".utf8)) }
  }

  @Test func recognisesLowercaseDigests() {
    #expect(CaptionNameplate.isDigest(String(repeating: "0f", count: 32)))
    #expect(!CaptionNameplate.isDigest(String(repeating: "0g", count: 32)))
    #expect(!CaptionNameplate.isDigest("00"))
  }
}
