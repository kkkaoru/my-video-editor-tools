import Foundation
import Testing

@testable import CaptionRender

/// The shared version-2 (multiple speakers) fixtures of `docs/caption-plan.md`.
struct SpeakerFixture: Decodable {
  struct Case: Decodable {
    let name: String
    let layout: CaptionLayout
    let faces: String
    let titles: [PlacementFixture.Title]
  }

  let tolerance: Double
  let cases: [Case]

  static func data(_ name: String) throws -> Data {
    try Data(contentsOf: PlacementFixture.directory.appendingPathComponent(name))
  }

  static func plan() throws -> CaptionPlan {
    try CaptionPlan.decode(data("caption-plan-v2.json"))
  }
}

struct CaptionSpeakersTests {
  @Test(arguments: [
    "absent", "fixture-layout", "bottom-all", "above-face-all", "above-face-v1-faces",
    "per-speaker-offsets",
  ])
  func matchesTheSharedExpectedPlacements(_ name: String) throws {
    let fixture = try JSONDecoder().decode(
      SpeakerFixture.self, from: SpeakerFixture.data("caption-layout-expected-v2.json"))
    let expected = try #require(fixture.cases.first { $0.name == name })
    let placed = try CaptionPlacement.placements(
      plan: SpeakerFixture.plan(), layout: expected.layout,
      faces: CaptionFaces.decode(SpeakerFixture.data(expected.faces)))
    #expect(placed.map(\.basis) == expected.titles.map(\.basis))
    let worst = zip(placed, expected.titles).map { max(abs($0.x - $1.x), abs($0.y - $1.y)) }
      .max()
    #expect(try #require(worst) <= fixture.tolerance)
  }

  @Test func coversEveryExpectedCase() throws {
    let fixture = try JSONDecoder().decode(
      SpeakerFixture.self, from: SpeakerFixture.data("caption-layout-expected-v2.json"))
    #expect(fixture.cases.count == 6)
  }

  @Test func readsTheSharedVersion2Files() throws {
    let plan = try SpeakerFixture.plan()
    #expect(plan.version == 2 && plan.speakers?.map(\.id) == ["left", "middle", "right"])
    #expect(plan.speakers?.map(\.colorTop) == ["#E2C6FF", "#BEEEC6", "#B6EAFF"])
    #expect(plan.titles.map(\.speaker) == ["left", "middle", "right", nil, nil, nil, "left"])
    let layout = try CaptionLayout.decode(SpeakerFixture.data("caption-layout-v2.json"))
    #expect(layout.version == 2 && layout.speakers?["right"]?.offsetY == -30)
    let faces = try CaptionFaces.decode(SpeakerFixture.data("caption-faces-v2.json"))
    #expect(faces.version == 2 && faces.samples.map { $0.boxes?.count } == [0, 3, 3, 1, 1, 1, 0])
  }

  @Test func showsOrHidesSpeakersWithoutMovingTheRest() throws {
    let plan = try SpeakerFixture.plan()
    let placed = try CaptionPlacement.resolve(plan: plan, layout: .plan, faces: nil)
    let left = try placed.showing(only: ["left"])
    #expect(left.titles.map(\.text) == ["こんにちは", "またね"])
    let quiet = try placed.showing(hiding: ["unassigned", "right"])
    #expect(quiet.titles.map(\.speaker) == ["left", "middle", "left"])
    let unassigned = try placed.showing(only: ["unassigned"])
    #expect(
      unassigned.titles.map(\.y) == [placed.titles[3].y, placed.titles[4].y, placed.titles[5].y])
    #expect(try placed.showing().titles == placed.titles)
    #expect(
      throws: CaptionRenderError.invalid(
        "Unknown speaker: the plan has left, middle, right, unassigned")
    ) {
      try placed.showing(hiding: ["nobody"])
    }
    #expect(throws: CaptionRenderError.self) { try placed.showing(only: ["nobody"]) }
  }

  @Test func givesEachSpeakerItsOwnColours() throws {
    let plan = try SpeakerFixture.plan()
    #expect(
      plan.speakerFills()["left"] == CaptionFill(top: "#E2C6FF", bottom: "#C48CFF", gradient: true))
    #expect(plan.speakerFills(gradient: false)["right"]?.gradient == false)
    #expect(
      try CaptionPlan.decode(Data(contentsOf: CaptionPlanTests.fixture)).speakerFills().isEmpty)
    var overlay = try CaptionOverlay(plan: plan, layout: .plan)
    #expect(overlay.speakerFills.keys.sorted() == ["left", "middle", "right"])
    let mine = CaptionFill(top: "#000000", bottom: "#000000", gradient: false)
    let chosen = try CaptionOverlay(plan: plan, layout: .plan, speakerFills: ["left": mine])
    #expect(chosen.speakerFills["left"] == mine && chosen.speakerFills["middle"] != nil)
    #expect(try overlay.layers(at: 100).count == 3)
  }

  /// The fixture plan with `change` applied to its text.
  static func plan(_ change: (String) -> String) throws -> Data {
    Data(
      change(String(decoding: try SpeakerFixture.data("caption-plan-v2.json"), as: UTF8.self)).utf8)
  }

  @Test(
    arguments: [
      { $0.replacingOccurrences(of: #""version": 2"#, with: #""version": 1"#) },
      { $0.replacingOccurrences(of: #""id": "middle""#, with: #""id": "left""#) },
      { $0.replacingOccurrences(of: #""id": "middle""#, with: #""id": "unassigned""#) },
      { $0.replacingOccurrences(of: ##""colorTop": "#BEEEC6""##, with: #""colorTop": "green""#) },
      { $0.replacingOccurrences(of: #""laneX": 180.0"#, with: #""laneX": 180.0, "lane": 1"#) },
      {
        $0.replacingOccurrences(of: #""laneX": 180.0"#, with: #""laneX": 180.0, "icon": "m.png""#)
      },
      { $0.replacingOccurrences(of: #""speaker": "middle""#, with: #""speaker": "nobody""#) },
      // "こんにちは" running into its speaker's next title.
      { $0.replacingOccurrences(of: #""endFrame": 60"#, with: #""endFrame": 170"#) },
    ] as [@Sendable (String) -> String])
  func refusesVersion2PlansThatBreakTheContract(_ change: @Sendable (String) -> String) throws {
    let data = try Self.plan(change)
    #expect(data != (try SpeakerFixture.data("caption-plan-v2.json")))
    #expect(throws: CaptionRenderError.self) { try CaptionPlan.decode(data) }
  }

  @Test func acceptsASpeakerIcon() throws {
    let data = try Self.plan {
      $0.replacingOccurrences(
        of: #""laneX": 180.0"#,
        with:
          #""laneX": 180.0, "icon": "m.png", "iconSha256": "\#(String(repeating: "0", count: 64))""#
      )
    }
    #expect(try CaptionPlan.decode(data).speakers?[1].icon == "m.png")
  }

  @Test func refusesSpeakersInAVersion1Plan() throws {
    let text = String(decoding: try Data(contentsOf: CaptionPlanTests.fixture), as: UTF8.self)
    let data = Data(
      text.replacingOccurrences(of: #""nameplate": {"#, with: #""speakers": [], "nameplate": {"#)
        .utf8)
    #expect(throws: CaptionRenderError.invalid("Only a version-2 caption plan has speakers")) {
      try CaptionPlan.decode(data)
    }
  }

  @Test func refusesLayoutsNamingUnknownSpeakers() throws {
    let layout = CaptionLayout(mode: .plan, speakers: ["nobody": .init(offsetX: 1)])
    #expect(throws: CaptionRenderError.invalid("The caption layout names a speaker the plan lacks"))
    {
      try CaptionPlacement.placements(plan: SpeakerFixture.plan(), layout: layout, faces: nil)
    }
    #expect(layout.overriding(offsetY: 2).speakers?["nobody"]?.offsetX == 1)
  }

  @Test(arguments: [
    #"{"version": 2, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "source": {"model": "m", "modelSha256": "a", "file": "f", "sha256": "b"}, "samples": [{"frame": 1, "boxes": [], "faceCount": 0}]}"#,
    #"{"version": 1, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "source": {"model": "m", "modelSha256": "a", "file": "f", "sha256": "b"}, "samples": [{"frame": 1, "boxes": []}]}"#,
    #"{"version": 2, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "source": {"model": "m", "modelSha256": "a", "file": "f", "sha256": "b"}, "samples": [{"frame": 1, "faceCount": 0, "box": null}]}"#,
    #"{"version": 2, "clock": "delivery", "rate": 30, "width": 1, "height": 1, "source": {"model": "m", "modelSha256": "a", "file": "f", "sha256": "b"}, "samples": [{"frame": 1, "boxes": [{"x": 0, "y": 0, "w": 1, "h": 1}]}]}"#,
  ])
  func refusesVersion2FaceTracksThatBreakTheContract(_ text: String) {
    #expect(throws: CaptionRenderError.self) { try CaptionFaces.decode(Data(text.utf8)) }
  }

  @Test func bandsSplitTheCanvasAtLaneMidpoints() throws {
    let plan = try SpeakerFixture.plan()
    #expect(CaptionPlacement.band(of: "left", plan: plan) == -Double.infinity..<895)
    #expect(CaptionPlacement.band(of: "middle", plan: plan) == 895..<1395)
    #expect(CaptionPlacement.band(of: "right", plan: plan) == 1395..<Double.infinity)
    #expect(CaptionPlacement.band(of: "nobody", plan: plan).isEmpty)
  }
}
