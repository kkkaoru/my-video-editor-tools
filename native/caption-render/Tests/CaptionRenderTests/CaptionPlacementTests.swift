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
