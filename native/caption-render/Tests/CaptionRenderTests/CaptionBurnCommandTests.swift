import CryptoKit
import Foundation
import Testing

@testable import CaptionRender

struct CaptionBurnCommandTests {
  static let here = URL(fileURLWithPath: "/work")

  @Test func parsesPathsRangesAndOptions() throws {
    let command = try CaptionBurnCommand(
      arguments: [
        "in.mp4", "/plans/a.captions.json", "out/x.mp4", "--range", "1.5:3", "--range", "10:12",
        "--no-nameplate", "--nameplate-font", "font.ttf", "--no-gradient", "--top", "#FF0000",
        "--bottom", "#00FF00", "--codec", "hevc", "--over-burned",
      ], directory: Self.here)
    #expect(command.movie.path == "/work/in.mp4")
    #expect(command.plan.path == "/plans/a.captions.json")
    #expect(command.output.path == "/work/out/x.mp4")
    #expect(
      command.ranges == [
        CaptionExportRange(start: 1.5, end: 3), CaptionExportRange(start: 10, end: 12),
      ])
    #expect(!command.nameplate && !command.gradient && command.overBurned)
    #expect(command.nameplateFont?.path == "/work/font.ttf")
    #expect((command.top, command.bottom, command.codec) == ("#FF0000", "#00FF00", .hevc))
  }

  @Test func defaultsToTheWholeMovieAndThePlansColours() throws {
    let command = try CaptionBurnCommand(
      arguments: ["a.mp4", "a.json", "b.mp4"], directory: Self.here)
    #expect(command.ranges == nil && command.nameplate && command.gradient)
    #expect(command.codec == .h264 && !command.overBurned)
    let style = MotionCaptionRendererTests.style
    #expect(command.fill(for: style) == nil)
    let single = try CaptionBurnCommand(
      arguments: ["a.mp4", "a.json", "b.mp4", "--no-gradient"], directory: Self.here)
    #expect(
      single.fill(for: style) == CaptionFill(top: "#97D8F7", bottom: "#2FB0EE", gradient: false))
    let top = try CaptionBurnCommand(
      arguments: ["a.mp4", "a.json", "b.mp4", "--top", "#000000"], directory: Self.here)
    #expect(top.fill(for: style) == CaptionFill(top: "#000000", bottom: "#2FB0EE", gradient: true))
  }

  @Test(arguments: [
    ["a.mp4", "a.json"], ["a.mp4", "a.json", "b.mp4", "c.mp4"],
    ["a.mp4", "a.json", "b.mp4", "--range"], ["a.mp4", "a.json", "b.mp4", "--range", "1-2"],
    ["a.mp4", "a.json", "b.mp4", "--range", "x:2"], ["a.mp4", "a.json", "b.mp4", "--codec", "av1"],
    ["a.mp4", "a.json", "b.mp4", "--unknown"],
  ])
  func refusesBadArguments(_ arguments: [String]) {
    #expect(throws: CaptionRenderError.self) {
      try CaptionBurnCommand(arguments: arguments, directory: Self.here)
    }
  }

  static func digest(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
  }

  /// A plan beside a synthetic movie, with a nameplate whose icon lies beside the plan.
  static func files(in directory: URL) async throws -> (movie: URL, plan: URL, font: URL) {
    let movie = try await Synthetic.movie(in: directory, audio: false)
    let icon = try CaptionNameplateRendererTests.icon()
    try icon.write(to: directory.appendingPathComponent("movie.nameplate.png"))
    let font = CaptionNameplateRendererTests.variable
    let nameplate = """
      {"burned": false, "icon": "movie.nameplate.png", "iconSha256": "\(digest(icon))",
       "iconX": 10, "iconY": 10, "iconSize": 60, "ringWidth": 4, "ringColour": "#FFFFFF",
       "name": "Name", "nameFont": "SFNS.ttf", "nameFontSha256": "\(digest(try Data(contentsOf: font)))",
       "nameFontVariation": "Bold", "nameSize": 20, "nameColour": "#FFFFFF",
       "nameStrokeWidth": 3, "nameStrokeColour": "#000000", "nameX": 40.0, "nameY": 76,
       "nameAnchor": "mt"}
      """
    let plan = directory.appendingPathComponent("movie.captions.json")
    try Synthetic.plan(nameplate: nameplate).write(to: plan)
    return (movie, plan, font)
  }

  @Test func runsTheExportWithThePlansNameplate() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let (movie, plan, font) = try await Self.files(in: directory)
    let report = try await CaptionBurnCommand(
      arguments: [
        movie.path, plan.path, "out.mp4", "--range", "0:0.2", "--nameplate-font", font.path,
      ], directory: directory
    ).run()
    #expect(report.frames == 6)
    let first = try #require(
      try await Synthetic.frames(directory.appendingPathComponent("out.mp4")).first)
    #expect(Synthetic.marked(first) > 200)  // the nameplate, before any caption
    // Without --nameplate-font the plan's SFNS.ttf is found in /System/Library/Fonts.
    let found = try await CaptionBurnCommand(
      arguments: [movie.path, plan.path, "found.mp4", "--range", "0:0.2"], directory: directory
    ).run()
    #expect(found.frames == 6)
    let plain = try await CaptionBurnCommand(
      arguments: [movie.path, plan.path, "plain.mp4", "--range", "0:0.2", "--no-nameplate"],
      directory: directory
    ).run()
    #expect(plain.frames == 6)
  }

  @Test func findsThePlansFontByNameAndDigestOnly() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let (_, plan, font) = try await Self.files(in: directory)
    let plate = try #require(try CaptionPlan.decode(Data(contentsOf: plan)).nameplate)
    let system = URL(fileURLWithPath: "/System/Library/Fonts")
    #expect(
      throws: CaptionRenderError.invalid(
        "Nameplate font SFNS.ttf is in none of [\"\(directory.path)\"]; pass"
          + " --nameplate-font FILE or --no-nameplate")
    ) {
      try CaptionBurnCommand.font(plate, roots: [directory])
    }
    let impostor = directory.appendingPathComponent("SFNS.ttf")
    try Data("not the font".utf8).write(to: impostor)
    #expect(
      throws: CaptionRenderError.invalid(
        "Nameplate font \(impostor.path) is not the plan's (SHA-256 \(plate.nameFontSha256)); pass"
          + " --nameplate-font FILE or --no-nameplate")
    ) {
      try CaptionBurnCommand.font(plate, roots: [directory])
    }
    #expect(
      try CaptionBurnCommand.font(plate, roots: [directory, system]) == Data(contentsOf: font))
    #expect(CaptionBurnCommand.fontRoots.map(\.lastPathComponent) == ["Fonts", "Fonts", "Fonts"])
  }

  @Test func parsesPlacementOptions() throws {
    let command = try CaptionBurnCommand(
      arguments: [
        "a.mp4", "a.json", "b.mp4", "--layout", "l.json", "--mode", "aboveFace", "--offset-x",
        "-3.5", "--offset-y", "40",
      ], directory: Self.here)
    #expect(command.layout?.path == "/work/l.json" && command.mode == .aboveFace)
    #expect((command.offsetX, command.offsetY) == (-3.5, 40))
    #expect(throws: CaptionRenderError.self) {
      try CaptionBurnCommand(
        arguments: ["a.mp4", "a.json", "b.mp4", "--mode", "left"], directory: Self.here)
    }
    #expect(throws: CaptionRenderError.invalid("Not a finite number: inf")) {
      try CaptionBurnCommand(
        arguments: ["a.mp4", "a.json", "b.mp4", "--offset-y", "inf"], directory: Self.here)
    }
  }

  @Test func readsTheMoviesOwnLayoutAndFacesUnlessOverridden() throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let none = try CaptionBurnCommand(
      arguments: ["movie.mp4", "a.json", "b.mp4", "--offset-y", "7"], directory: directory)
    #expect(try none.resolvedLayout() == CaptionLayout(mode: .plan, offsetY: 7))
    #expect(try none.faces() == nil)
    try Data(#"{"version": 1, "mode": "top", "margin": 40}"#.utf8).write(
      to: directory.appendingPathComponent("movie.caption-layout.json"))
    try Data(contentsOf: PlacementFixture.directory.appendingPathComponent("caption-faces-v1.json"))
      .write(to: directory.appendingPathComponent("movie.faces.json"))
    let own = try CaptionBurnCommand(
      arguments: ["movie.mp4", "a.json", "b.mp4", "--mode", "bottom"], directory: directory)
    #expect(try own.resolvedLayout() == CaptionLayout(mode: .bottom, margin: 40))
    #expect(try own.faces()?.samples.count == 6)
    let other = directory.appendingPathComponent("other.json")
    try Data(#"{"version": 1, "mode": "center"}"#.utf8).write(to: other)
    let given = try CaptionBurnCommand(
      arguments: ["movie.mp4", "a.json", "b.mp4", "--layout", other.path], directory: directory)
    #expect(try given.resolvedLayout() == CaptionLayout(mode: .center))
  }

  @Test func parsesSpeakerChoices() throws {
    let command = try CaptionBurnCommand(
      arguments: [
        "a.mp4", "a.json", "b.mp4", "--speaker", "left", "--speaker", "unassigned",
        "--hide-speaker", "right",
      ], directory: Self.here)
    #expect(command.speakers == ["left", "unassigned"] && command.hiddenSpeakers == ["right"])
    let all = try CaptionBurnCommand(arguments: ["a.mp4", "a.json", "b.mp4"], directory: Self.here)
    #expect(all.speakers == nil && all.hiddenSpeakers.isEmpty)
  }

  /// A version-2 plan for the synthetic movie: two speakers, one title each.
  static let speakersPlan = Data(
    String(decoding: Synthetic.plan(), as: UTF8.self)
      .replacingOccurrences(of: #""version": 1"#, with: #""version": 2"#)
      .replacingOccurrences(
        of: #""titles": [{"text": "口", "startFrame": 5, "endFrame": 25, "x": 0, "y": -20,"#,
        with: #"""
          "speakers": [
            {"id": "left", "name": "L", "colorTop": "#E2C6FF", "colorBottom": "#C48CFF", "laneX": -80,
             "icon": "synthetic-multi.speaker-left.png",
             "iconSha256": "92a68a3060c3748b05c595d0109ac9b46078a5a4dceee9228558cab6304cef63"},
            {"id": "right", "name": "R", "colorTop": "#B6EAFF", "colorBottom": "#6CD4FF", "laneX": 80}],
          "titles": [{"text": "口", "startFrame": 5, "endFrame": 25, "x": 80, "y": -20,
            "graphemeFrames": [5], "speaker": "right"},
           {"text": "口", "startFrame": 5, "endFrame": 25, "x": -80, "y": -20, "speaker": "left",
          """#
      )
      .utf8)

  @Test func burnsOnlyTheChosenSpeakers() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory, audio: false)
    let plan = directory.appendingPathComponent("movie.captions.json")
    try Self.speakersPlan.write(to: plan)
    try SpeakerFixture.data("synthetic-multi.speaker-left.png").write(
      to: directory.appendingPathComponent("synthetic-multi.speaker-left.png"))
    #expect(try CaptionPlan.decode(Self.speakersPlan).speakers?.count == 2)
    func run(_ name: String, _ options: [String]) async throws -> [UInt8] {
      _ = try await CaptionBurnCommand(
        arguments: [movie.path, plan.path, name, "--range", "0.3:0.4"] + options,
        directory: directory
      ).run()
      return try #require(try await Synthetic.frames(directory.appendingPathComponent(name)).first)
    }
    let both = Synthetic.marked(try await run("both.mp4", ["--no-gradient"]))
    let left = Synthetic.marked(try await run("left.mp4", ["--hide-speaker", "right"]))
    let none = Synthetic.marked(try await run("none.mp4", ["--speaker", "unassigned"]))
    #expect(both > left && left > 0)
    #expect(none == 0)  // a hidden speaker's icon is hidden too
    let icon = Synthetic.marked(try await run("icon.mp4", ["--speaker", "left"]))
    let bare = Synthetic.marked(try await run("bare.mp4", ["--speaker", "left", "--no-nameplate"]))
    #expect(icon > bare)
    await #expect(throws: CaptionRenderError.self) {
      try await CaptionBurnCommand(
        arguments: [movie.path, plan.path, "x.mp4", "--speaker", "nobody"], directory: directory
      ).run()
    }
  }

  @Test func refusesUnreadablePlans() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    await #expect(throws: CaptionRenderError.self) {
      try await CaptionBurnCommand(
        arguments: ["movie.mp4", "missing.json", "out.mp4"], directory: directory
      ).run()
    }
  }
}
