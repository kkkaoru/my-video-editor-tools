import CaptionRender
import CoreGraphics
import CryptoKit
import Foundation
import Testing

@testable import TranscriptCore

/// A delivery folder holding the shared synthetic plan (`tests/fixtures/caption-plan-v1.json`)
/// beside `synthetic-review.mp4`, its text edited per test.
struct CaptionPlanFixture {
    static let repository = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    static let font = URL(fileURLWithPath: "/System/Library/Fonts/SFNS.ttf")

    let folder: URL
    /// `synthetic-review` (one speaker, version 1) or `synthetic-multi` (version 2).
    let stem: String
    var movie: URL { folder.appending(path: "\(stem).mp4") }
    var plan: URL { folder.appending(path: "\(stem).captions.json") }

    /// `siblings` copies those fixtures beside the movie as `<stem><suffix>` (for example
    /// `caption-layout-v1.json` as `.caption-layout.json`). `speakers` uses the version-2 plan
    /// of three speakers, with its two icons.
    init(
        _ replacements: [(String, String)] = [], dropsNameplate: Bool = false,
        siblings: [String: String] = [:], speakers: Bool = false
    ) throws {
        folder = try transcriptTestWorkRoot().appending(path: "captions-\(UUID().uuidString)")
        stem = speakers ? "synthetic-multi" : "synthetic-review"
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let fixtures = Self.repository.appending(path: "tests/fixtures")
        let images =
            speakers
            ? ["synthetic-multi.speaker-left.png", "synthetic-multi.speaker-right.png"]
            : ["synthetic-review.nameplate.png"]
        for image in images {
            try FileManager.default.copyItem(
                at: fixtures.appending(path: image), to: folder.appending(path: image))
        }
        for (fixture, suffix) in siblings {
            try FileManager.default.copyItem(
                at: fixtures.appending(path: fixture),
                to: folder.appending(path: "\(stem)\(suffix)"))
        }
        var text = try String(
            contentsOf: fixtures.appending(
                path: speakers ? "caption-plan-v2.json" : "caption-plan-v1.json"),
            encoding: .utf8)
        if dropsNameplate, let plate = text.range(of: "\"nameplate\": {") {
            text = text[..<plate.lowerBound] + "\"nameplate\": null\n}\n"
        }
        for (old, new) in replacements {
            #expect(text.contains(old))
            text = text.replacingOccurrences(of: old, with: new)
        }
        try Data(text.utf8).write(to: plan)
    }

    /// The fixture's nameplate drawn with a real variable font (SF's).
    static func realFont() throws -> [(String, String)] {
        let digest = SHA256.hash(data: try Data(contentsOf: font))
            .map { String(format: "%02x", $0) }.joined()
        return [
            ("NotoSansJP-VariableFont_wght.ttf", font.lastPathComponent),
            (String(repeating: "1", count: 64), digest),
        ]
    }

    func remove() throws { try FileManager.default.removeItem(at: folder) }
}

struct DeliveryCaptionsTests {
    @Test func planSitsBesideTheMovieUnderItsStem() {
        let movie = URL(fileURLWithPath: "/v/a/2026_JST_replay-1-review.mp4")
        let video = VideoTranscript(
            title: "t", movieURL: movie, transcriptURL: movie, cues: [])
        #expect(video.captionPlanURL.path == "/v/a/2026_JST_replay-1-review.captions.json")
    }

    @Test func aMovieWithoutAPlanHasNoCaptions() throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let other = fixture.folder.appending(path: "other.mp4")
        #expect(try DeliveryCaptions.load(movie: other) == nil)
    }

    @Test func theFixturePlanLoadsAndAnUnverifiableNameplateIsReported() throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let captions = try #require(try DeliveryCaptions.load(movie: fixture.movie))
        #expect(captions.plan.titles.count == 3)
        #expect(!captions.plan.burned)
        // The fixture pins a placeholder font digest, so only the nameplate is refused.
        #expect(captions.nameplate == nil)
        #expect(captions.nameplateIssue?.hasPrefix("アイコンを描けません") == true)
    }

    @Test func theNameplateIsDrawnWithTheFontFoundInTheRoots() throws {
        let fixture = try CaptionPlanFixture(CaptionPlanFixture.realFont())
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let missing = fixture.folder.appending(path: "no-fonts")
        let captions = try #require(
            try DeliveryCaptions.load(
                movie: fixture.movie,
                fontRoots: [missing, CaptionPlanFixture.font.deletingLastPathComponent()]))
        let plate = try #require(captions.nameplate)
        #expect(plate.width == 1920)
        #expect(plate.height == 1080)
        #expect(captions.nameplateIssue == nil)
        let absent = try #require(
            try DeliveryCaptions.load(movie: fixture.movie, fontRoots: [missing]))
        #expect(absent.nameplate == nil)
        #expect(absent.nameplateIssue?.contains("SFNS.ttf がありません") == true)
    }

    @Test func aBurnedOrAbsentNameplateIsNotDrawn() throws {
        let burned = try CaptionPlanFixture([("\"burned\": false", "\"burned\": true")])
        defer { #expect(throws: Never.self) { try burned.remove() } }
        let captions = try #require(try DeliveryCaptions.load(movie: burned.movie))
        #expect(captions.plan.burned)
        #expect(captions.nameplate == nil)
        #expect(captions.nameplateIssue == nil)
        let none = try CaptionPlanFixture(dropsNameplate: true)
        defer { #expect(throws: Never.self) { try none.remove() } }
        let bare = try #require(try DeliveryCaptions.load(movie: none.movie))
        #expect(bare.plan.nameplate == nil)
        #expect(bare.nameplateIssue == nil)
    }

    @Test func theSavedPositionAndFaceTrackLoadBesideThePlan() throws {
        let fixture = try CaptionPlanFixture(siblings: [
            "caption-layout-v1.json": ".caption-layout.json",
            "caption-faces-v1.json": ".faces.json",
        ])
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let captions = try #require(try DeliveryCaptions.load(movie: fixture.movie))
        #expect(captions.layout == CaptionLayout(mode: .aboveFace, offsetY: -40))
        #expect(captions.faces?.samples.count == 6)
        #expect(captions.positionIssue == nil)
        let bare = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try bare.remove() } }
        let none = try #require(try DeliveryCaptions.load(movie: bare.movie))
        #expect(none.layout == .plan)
        #expect(none.faces == nil)
    }

    @Test func anUnusablePositionOrFaceTrackIsReportedAndLeftOut() throws {
        let fixture = try CaptionPlanFixture(siblings: [
            "caption-layout-v1.json": ".caption-layout.json",
            "caption-faces-v1.json": ".faces.json",
        ])
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        try Data(#"{"version": 1, "mode": "sideways"}"#.utf8).write(
            to: CaptionLayoutFile.url(for: fixture.movie))
        var captions = try #require(try DeliveryCaptions.load(movie: fixture.movie))
        #expect(captions.layout == .plan)
        #expect(captions.faces != nil)
        #expect(captions.positionIssue?.hasPrefix("字幕位置を読めません") == true)
        try FileManager.default.removeItem(at: CaptionLayoutFile.url(for: fixture.movie))
        let faces = DeliveryCaptions.sibling(of: fixture.movie, ".faces.json")
        let wide = try String(contentsOf: faces, encoding: .utf8).replacingOccurrences(
            of: "\"width\": 1920", with: "\"width\": 1080")
        try Data(wide.utf8).write(to: faces)
        captions = try #require(try DeliveryCaptions.load(movie: fixture.movie))
        #expect(captions.faces == nil)
        #expect(captions.positionIssue?.contains("別の画面サイズ") == true)
    }

    @Test func positionsAreWrittenAsPythonWritesThemWithEveryOptionTheyCarry() throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let target = CaptionLayoutFile.url(for: fixture.movie)
        try CaptionLayoutFile.write(
            CaptionLayout(mode: .bottom, offsetY: -40), movie: fixture.movie)
        #expect(
            try String(contentsOf: target, encoding: .utf8)
                == "{\n \"version\": 1,\n \"mode\": \"bottom\",\n \"offsetY\": -40.0\n}\n")
        let mode = try FileManager.default.attributesOfItem(atPath: target.path)[.posixPermissions]
        #expect(mode as? Int == 0o600)
        try CaptionLayoutFile.write(
            CaptionLayout(mode: .center, offsetX: 12.5, margin: 100, maxDrift: 80),
            movie: fixture.movie)
        #expect(
            try String(contentsOf: target, encoding: .utf8)
                == "{\n \"version\": 1,\n \"mode\": \"center\",\n \"offsetX\": 12.5,\n"
                + " \"margin\": 100.0,\n \"maxDrift\": 80.0\n}\n")
        #expect(
            try FileManager.default.contentsOfDirectory(atPath: fixture.folder.path)
                .filter { $0.hasPrefix(".") }.isEmpty)
        let speakers = CaptionLayout(
            mode: .plan,
            speakers: ["b": .init(), "あ": .init(mode: .bottom, offsetY: -8)])
        try CaptionLayoutFile.write(speakers, movie: fixture.movie)
        #expect(try CaptionLayout.decode(Data(contentsOf: target)) == speakers)
        #expect(
            try String(contentsOf: target, encoding: .utf8)
                == "{\n \"version\": 2,\n \"mode\": \"plan\",\n \"speakers\": {\n  \"b\": {},\n"
                + "  \"あ\": {\n   \"mode\": \"bottom\",\n   \"offsetY\": -8.0\n  }\n }\n}\n")
        #expect(throws: CaptionRenderError.self) {
            try CaptionLayoutFile.write(CaptionLayout(mode: .top, margin: -1), movie: fixture.movie)
        }
        #expect(throws: DeliveryCaptionsError.self) {
            try CaptionLayoutFile.write(
                .plan, movie: fixture.folder.appending(path: "gone/x.mp4"))
        }
    }

    @Test func aPlanForAnotherMovieIsRefused() throws {
        let fixture = try CaptionPlanFixture([("synthetic-review.mp4", "other.mp4")])
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        #expect(throws: DeliveryCaptionsError.otherMovie("other.mp4")) {
            try DeliveryCaptions.load(movie: fixture.movie)
        }
        #expect(
            DeliveryCaptionsError.otherMovie("other.mp4").errorDescription
                == "字幕プランが別の動画（other.mp4）のものです")
    }

    @Test func malformedLinkedOrOversizedPlansAreRefused() throws {
        let fixture = try CaptionPlanFixture([("\"version\": 1", "\"version\": 2")])
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        #expect(throws: DeliveryCaptionsError.self) {
            try DeliveryCaptions.load(movie: fixture.movie)
        }
        let linked = fixture.folder.appending(path: "linked.mp4")
        try FileManager.default.createSymbolicLink(
            at: DeliveryCaptions.planURL(for: linked), withDestinationURL: fixture.plan)
        #expect(throws: DeliveryCaptionsError.unreadable("linked.captions.json")) {
            try DeliveryCaptions.load(movie: linked)
        }
        let large = fixture.folder.appending(path: "large.mp4")
        try Data(count: DeliveryCaptions.maximumBytes + 1).write(
            to: DeliveryCaptions.planURL(for: large))
        #expect(throws: DeliveryCaptionsError.unreadable("large.captions.json")) {
            try DeliveryCaptions.load(movie: large)
        }
        #expect(
            DeliveryCaptionsError.unreadable("x").errorDescription == "字幕プランを読み込めません: x")
    }

    @Test(arguments: [
        (0.0, 0), (-1.0, 0), (Double.nan, 0), (1.0 / 30, 1), (0.999, 29), (285.5, 8565),
        (8565.0 / 30, 8565),
    ])
    func playerTimeMapsToTheShowingFrame(seconds: Double, frame: Int) {
        #expect(CaptionGeometry.frame(atSeconds: seconds, rate: 30) == frame)
    }

    @Test func aPortraitCanvasPillarboxedInALandscapeViewKeepsItsScaleAndTop() {
        // 1080×1920 shown 540×960 at x 690 of a 1920×960 view.
        let video = CGRect(x: 690, y: 0, width: 540, height: 960)
        let canvas = CGSize(width: 1080, height: 1920)
        let rect = CGRect(x: 100, y: 200, width: 400, height: 120)
        #expect(
            CaptionGeometry.frame(of: rect, canvas: canvas, in: video, flipped: false)
                == CGRect(x: 740, y: 800, width: 200, height: 60))
        #expect(
            CaptionGeometry.frame(of: rect, canvas: canvas, in: video, flipped: true)
                == CGRect(x: 740, y: 100, width: 200, height: 60))
    }

    @Test func aLandscapeCanvasLetterboxedKeepsItsBottomEdge() {
        let video = CGRect(x: 0, y: 30, width: 960, height: 540)
        let canvas = CGSize(width: 1920, height: 1080)
        let bottom = CGRect(x: 0, y: 1000, width: 1920, height: 80)
        #expect(
            CaptionGeometry.frame(of: bottom, canvas: canvas, in: video, flipped: false)
                == CGRect(x: 0, y: 30, width: 960, height: 40))
    }
}
