import CoreGraphics
import Foundation
import Testing

@testable import CaptionRender

/// The shared expected results of `docs/caption-plan.md` "Caption position".
struct PlacementFixture: Decodable {
  struct Title: Decodable {
    let x: Double
    let y: Double
    let basis: CaptionPlacement.Basis
  }

  struct Case: Decodable {
    let name: String
    let layout: CaptionLayout
    let titles: [Title]
  }

  let tolerance: Double
  let cases: [Case]

  static let directory = CaptionPlanTests.fixture.deletingLastPathComponent()

  static func load() throws -> PlacementFixture {
    try JSONDecoder().decode(
      PlacementFixture.self,
      from: Data(contentsOf: directory.appendingPathComponent("caption-layout-expected-v1.json")))
  }

  static func plan() throws -> CaptionPlan {
    try CaptionPlan.decode(Data(contentsOf: CaptionPlanTests.fixture))
  }

  static func faces() throws -> CaptionFaces {
    try CaptionFaces.decode(
      Data(contentsOf: directory.appendingPathComponent("caption-faces-v1.json")))
  }
}

struct CaptionPlacementTests {
  @Test(arguments: [
    "absent", "plan-offset", "above-face", "above-face-no-drift", "above-face-one-sided",
    "above-face-high", "top", "bottom", "bottom-offset-clamped", "center",
  ])
  func matchesTheSharedExpectedPlacements(_ name: String) throws {
    let fixture = try PlacementFixture.load()
    let expected = try #require(fixture.cases.first { $0.name == name })
    let placed = try CaptionPlacement.placements(
      plan: PlacementFixture.plan(), layout: expected.layout, faces: PlacementFixture.faces())
    #expect(placed.map(\.basis) == expected.titles.map(\.basis))
    let worst = zip(placed, expected.titles).map { max(abs($0.x - $1.x), abs($0.y - $1.y)) }
      .max()
    #expect(try #require(worst) <= fixture.tolerance)
  }

  @Test func refusesKeysOutsideTheContractNamingThem() {
    #expect(
      throws: CaptionRenderError.invalid(
        #"A caption layout has keys ["mode", "offsetZ", "version"], not ["faceGap", "margin", "maxDrift", "minScore", "mode", "offsetX", "offsetY", "speakers", "version"]"#
      )
    ) {
      try CaptionLayout.decode(Data(#"{"version": 1, "mode": "plan", "offsetZ": 1}"#.utf8))
    }
  }

  @Test func readsTheShapeOfArraysAndScalars() throws {
    #expect(try JSONShape.of(Data("[1, {}]".utf8)) == .array([.value, .object([:])]))
    #expect(try JSONShape.of(Data("3".utf8)) == .value)
    #expect(try JSONShape.of(Data("3".utf8))["a"] == .value)
    #expect(try JSONShape.of(Data("{}".utf8)).items.isEmpty)
    // Object keys are names only.
    #expect(JSONShape.Key(intValue: 0) == nil)
    #expect(JSONShape.Key(stringValue: "a").intValue == nil)
  }

  @Test func coversEveryExpectedCase() throws {
    #expect(try PlacementFixture.load().cases.count == 10)
  }

  @Test func resolvesAPlanWithMovedTitles() throws {
    let plan = try PlacementFixture.plan()
    let resolved = try CaptionPlacement.resolve(
      plan: plan, layout: CaptionLayout(mode: .top, margin: 40), faces: nil)
    #expect(resolved.titles.map(\.y) == [379.771270718232, 379.771270718232, 379.771270718232])
    #expect(resolved.titles.map(\.text) == plan.titles.map(\.text))
    #expect(resolved.nameplate == plan.nameplate)
  }

  @Test func readsTheSharedLayoutFile() throws {
    let layout = try CaptionLayout.decode(
      Data(contentsOf: PlacementFixture.directory.appendingPathComponent("caption-layout-v1.json")))
    #expect(layout == CaptionLayout(mode: .aboveFace, offsetY: -40))
    #expect(
      layout.overriding(mode: .bottom, offsetX: 5)
        == CaptionLayout(mode: .bottom, offsetX: 5, offsetY: -40))
    #expect(layout.overriding() == layout)
  }

  @Test(arguments: [
    #"{"version": 3, "mode": "plan"}"#, #"{"version": 1, "mode": "left"}"#,
    #"{"version": 1, "mode": "plan", "speakers": {}}"#,
    #"{"version": 2, "mode": "plan", "speakers": {"a": {"color": 1}}}"#,
    #"{"version": 2, "mode": "plan", "speakers": {"a": {"margin": -1}}}"#,
    #"{"version": 1, "mode": "plan", "margin": -1}"#,
    #"{"version": 1, "mode": "plan", "minScore": 1.5}"#, #"{"mode": "plan"}"#,
    #"{"version": 1, "mode": "plan", "offsetZ": 1}"#, "[]",
  ])
  func refusesInvalidLayouts(_ text: String) {
    #expect(throws: CaptionRenderError.self) { try CaptionLayout.decode(Data(text.utf8)) }
  }

  @Test(arguments: [
    #"{"version": 1, "clock": "source", "rate": 30, "width": 1, "height": 1, "samples": []}"#,
    #"{"version": 1, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "samples": [{"frame": 2, "faceCount": 0, "box": null}, {"frame": 2, "faceCount": 0, "box": null}]}"#,
    #"{"version": 1, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "samples": [{"frame": 2, "faceCount": 1, "box": null}]}"#,
    #"{"version": 1}"#,
    #"{"version": 1, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "source": {"model": "m", "modelSha256": "a", "file": "f", "sha256": "b"}, "samples": [], "extra": 1}"#,
    #"{"version": 1, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "source": {"model": "m", "modelSha256": "a", "file": "f"}, "samples": []}"#,
    #"{"version": 1, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "source": {"model": "m", "modelSha256": "a", "file": "f", "sha256": "b"}, "samples": [{"frame": 2, "faceCount": 0, "box": null, "note": 1}]}"#,
    #"{"version": 1, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "source": {"model": "m", "modelSha256": "a", "file": "f", "sha256": "b"}, "samples": [{"frame": 2, "faceCount": 1, "box": {"x": 1, "y": 1, "w": 1, "h": 1}}]}"#,
    #"{"version": 1, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "source": {"model": "m", "modelSha256": "a", "file": "f", "sha256": "b"}, "samples": [{"frame": 2, "faceCount": 1, "box": {"x": 1, "y": 1, "w": 0, "h": 1, "score": 0.9}}]}"#,
    "not json",
  ])
  func refusesInvalidFaceTracks(_ text: String) {
    #expect(throws: CaptionRenderError.self) { try CaptionFaces.decode(Data(text.utf8)) }
  }

  @Test func aboveFaceNeedsAFaceTrackOnThePlansCanvas() throws {
    let plan = try PlacementFixture.plan()
    #expect(
      throws: CaptionRenderError.invalid("Caption mode aboveFace needs the delivery's face track")
    ) {
      try CaptionPlacement.placements(
        plan: plan, layout: CaptionLayout(mode: .aboveFace), faces: nil)
    }
    let small = try CaptionFaces.decode(
      Data(
        #"{"version": 1, "clock": "delivery", "rate": 30, "width": 2, "height": 2, "source": {"model": "m", "modelSha256": "a", "file": "f", "sha256": "b"}, "samples": []}"#
          .utf8))
    #expect(throws: CaptionRenderError.invalid("The face track is not on the plan's canvas")) {
      try CaptionPlacement.placements(
        plan: plan, layout: CaptionLayout(mode: .aboveFace), faces: small)
    }
    // Faces are not read for other modes.
    #expect(try CaptionPlacement.placements(plan: plan, layout: .plan, faces: small).count == 3)
  }

  @Test func overlaysDrawTheResolvedPositions() throws {
    let plan = try PlacementFixture.plan()
    var planned = try CaptionOverlay(plan: plan)
    var moved = try CaptionOverlay(plan: plan, layout: CaptionLayout(mode: .plan, offsetY: -40))
    let before = try #require(try planned.layers(at: 30).first).rect
    let after = try #require(try moved.layers(at: 30).first).rect
    #expect(after.origin.y == before.origin.y - 40 && after.origin.x == before.origin.x)
  }
}

@Suite struct CaptionSafeZoneTests {
  /// The shared plan with a 1000 px width, font size 100, rim 13, and these margins.
  static func plan(left: Int?, right: Int?) throws -> CaptionPlan {
    var object = try #require(
      JSONSerialization.jsonObject(with: Data(contentsOf: CaptionPlanTests.fixture)) as? [String: Any])
    object["width"] = 1000
    var style = try #require(object["style"] as? [String: Any])
    style["fontSize"] = 100
    style["rim"] = 13
    object["style"] = style
    if let left { object["safeLeft"] = left }
    if let right { object["safeRight"] = right }
    return try CaptionPlan.decode(JSONSerialization.data(withJSONObject: object))
  }

  static func title(_ text: String) -> MotionCaptionTitle {
    MotionCaptionTitle(
      text: text, startFrame: 0, endFrame: 1, x: 0, y: 0,
      graphemeFrames: Array(repeating: 0, count: text.count))
  }

  @Test func aBlockIsItsWidestLineWithTheRimOnBothSides() throws {
    let plan = try Self.plan(left: 50, right: 150)
    #expect(
      CaptionPlacement.estimatedBlockWidth(Self.title("あい\nab"), style: plan.style)
        == 230)
    #expect(
      CaptionPlacement.estimatedBlockWidth(Self.title("ｱa"), style: plan.style)
        == 140)
  }

  @Test func aBlockStaysBetweenUnequalMargins() throws {
    let plan = try Self.plan(left: 50, right: 150)
    #expect(
      CaptionPlacement.insideSafeZone(100, title: Self.title("ああああああ"), plan: plan)
        == 35)
    #expect(
      CaptionPlacement.insideSafeZone(-300, title: Self.title("ああ"), plan: plan)
        == -300)
    #expect(
      CaptionPlacement.insideSafeZone(-500, title: Self.title("ああ"), plan: plan)
        == -335)
    #expect(
      CaptionPlacement.insideSafeZone(500, title: Self.title("ああ"), plan: plan)
        == 235)
  }

  @Test func aBlockWiderThanTheMarginsLeaveSitsInTheirMiddle() throws {
    let plan = try Self.plan(left: 50, right: 150)
    let wide = Self.title(String(repeating: "あ", count: 20))
    #expect(CaptionPlacement.insideSafeZone(100, title: wide, plan: plan) == -50)
  }

  @Test func aPlanWithoutMarginsLeavesTheTitleAlone() throws {
    let plan = try Self.plan(left: nil, right: nil)
    #expect(
      CaptionPlacement.insideSafeZone(490, title: Self.title("あ"), plan: plan)
        == 490)
  }

  @Test func marginsAloneOrLeavingNoWidthAreRefused() throws {
    #expect(throws: CaptionRenderError.self) { try Self.plan(left: 5, right: nil) }
    #expect(throws: CaptionRenderError.self) { try Self.plan(left: 1000, right: 0) }
    #expect(throws: CaptionRenderError.self) { try Self.plan(left: -1, right: 2) }
  }
}
