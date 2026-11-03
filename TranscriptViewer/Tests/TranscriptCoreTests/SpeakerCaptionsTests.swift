import AVFoundation
import AppKit
import CaptionRender
import SwiftUI
import Testing
import TranscriptCore
import ViewInspector

@testable import TranscriptViewer

/// Multiple speakers (plan version 2): chips, hiding, icons and per-speaker positions, on the
/// shared `caption-plan-v2.json` fixture (left and right have icons, middle has none).
@MainActor
struct SpeakerCaptionsTests {
    private static func opened(_ fixture: CaptionPlanFixture) async -> CaptionsModel {
        let model = CaptionsModel()
        model.open(fixture.movie)
        await model.loading?.value
        return model
    }

    @Test func theSpeakersIconsLoadBesideThePlanAndAMissingOneIsReported() throws {
        let fixture = try CaptionPlanFixture(speakers: true)
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let captions = try #require(try DeliveryCaptions.load(movie: fixture.movie))
        #expect(captions.plan.speakers?.map(\.id) == ["left", "middle", "right"])
        #expect(captions.speakerIcons.keys.sorted() == ["left", "right"])
        #expect(captions.nameplateIssue == nil)
        try FileManager.default.removeItem(
            at: fixture.folder.appending(path: "synthetic-multi.speaker-right.png"))
        let partial = try #require(try DeliveryCaptions.load(movie: fixture.movie))
        #expect(partial.speakerIcons.keys.sorted() == ["left"])
        #expect(partial.nameplateIssue == "話者アイコンを読めません: みぎ")
    }

    @Test func hidingASpeakerDropsItsTitlesAndIconWithoutMovingTheOthers() async throws {
        let fixture = try CaptionPlanFixture(speakers: true)
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = await Self.opened(fixture)
        #expect(model.speakers.map(\.name) == ["ひだり", "まんなか", "みぎ"])
        #expect(model.hasUnassigned)
        #expect(model.hasIcons)
        let all = try #require(model.placed)
        #expect(all.titles.count == 7)
        let iconsOfAll = try #require(model.underlay)
        model.show("left", false)
        #expect(!model.isShowing("left"))
        let shown = try #require(model.placed)
        #expect(shown.titles.count == 5)
        #expect(shown.titles.allSatisfy { $0.speaker != "left" })
        // The others keep exactly where they were placed with every speaker shown.
        #expect(shown.titles == all.titles.filter { $0.speaker != "left" })
        #expect(model.underlay != nil)
        #expect(model.underlay !== iconsOfAll)
        model.show(CaptionPlan.unassigned, false)
        #expect(model.placed?.titles.count == 2)
        model.show("left", true)
        model.show("right", false)
        #expect(model.placed?.titles.map(\.speaker) == ["left", "middle", "left"])
        // Only the hidden right one had an icon besides left.
        model.showsNameplate = false
        #expect(model.underlay == nil)
        #expect(model.speakerFills.isEmpty)
        model.usesGradient = false
        #expect(
            model.speakerFills["left"]
                == CaptionFill(top: "#E2C6FF", bottom: "#C48CFF", gradient: false))
        #expect(model.overlaySettings.speakerFills.count == 3)
        model.open(fixture.movie)
        #expect(model.hiddenSpeakers.isEmpty)
        #expect(!CaptionsModel().hasIcons)
        #expect(!CaptionsModel().hasUnassigned)
    }

    @Test func aPlacementThatCannotBeResolvedIsReportedAndDrawsNothing() async throws {
        let fixture = try CaptionPlanFixture(speakers: true)
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = await Self.opened(fixture)
        // 顔の上 needs the face track the movie does not have.
        model.place("left", .aboveFace)
        #expect(model.placed == nil)
        #expect(model.placementIssue?.hasPrefix("字幕を配置できません") == true)
        model.place("left", nil)
        #expect(model.placed != nil)
        #expect(model.placementIssue == nil)
    }

    @Test func eachSpeakersPositionIsSavedAsLayoutVersionTwoAndResetsToVersionOne() async throws {
        let fixture = try CaptionPlanFixture(
            siblings: ["caption-faces-v2.json": ".faces.json"], speakers: true)
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = await Self.opened(fixture)
        let target = CaptionLayoutFile.url(for: fixture.movie)
        model.place("right", .aboveFace)
        model.setOffset("right", y: -30)
        model.setOffset("right", x: 12)
        #expect(model.speakerOptions("right") == .init(mode: .aboveFace, offsetX: 12, offsetY: -30))
        #expect(model.speakerOptions("left") == nil)
        #expect(try CaptionLayout.decode(Data(contentsOf: target)).version == 2)
        model.place(.bottom)
        #expect(model.layout.speakers?["right"]?.mode == .aboveFace)
        model.resetPosition()
        #expect(model.layout.mode == .plan)
        #expect(model.layout.speakers?["right"] != nil)
        model.resetPosition("right")
        #expect(model.layout == .plan)
        #expect(try CaptionLayout.decode(Data(contentsOf: target)) == .plan)
    }

    @Test func aPlanWithoutSpeakersHasNoChips() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = await Self.opened(fixture)
        #expect(SpeakerChips(model: model).chips.isEmpty)
        #expect(throws: (any Error).self) {
            try SpeakerChips(model: model).inspect().find(ViewType.Toggle.self)
        }
    }

    @Test func chipsToggleSpeakersAndThePopoverEditsTheirPositions() async throws {
        let fixture = try CaptionPlanFixture(
            siblings: ["caption-faces-v2.json": ".faces.json"], speakers: true)
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = await Self.opened(fixture)
        let controls = SpeakerChips(model: model)
        #expect(controls.chips.map(\.name) == ["ひだり", "まんなか", "みぎ", "未割当"])
        #expect(controls.chips.map(\.label) == ["ひだり", "まんなか", "みぎ", "未割当"])
        let long = SpeakerChips.Chip(id: "x", name: "とてもながいなまえのはなしてさん", colour: "#FFFFFF")
        #expect(long.label == "とてもながいなま…")
        let chip = try controls.inspect().find(viewWithAccessibilityLabel: "話者 まんなか の字幕を表示")
            .toggle()
        try chip.tap()
        #expect(!model.isShowing("middle"))
        try chip.tap()
        #expect(model.isShowing("middle"))
        let options = CaptionStyleOptions(model: model)
        let speaker = try #require(model.speakers.first)
        let row = try options.speakerPosition(speaker).inspect()
        try row.find(ViewType.Picker.self).select(value: CaptionLayout.Mode?.some(.top))
        #expect(model.speakerOptions("left")?.mode == .top)
        try row.find(viewWithAccessibilityLabel: "ひだりの字幕の横のずらし（ピクセル）").textField()
            .setInput("15")
        try row.find(viewWithAccessibilityLabel: "ひだりの字幕の縦のずらし（ピクセル、下が正）").textField()
            .setInput("-20")
        #expect(model.speakerOptions("left") == .init(mode: .top, offsetX: 15, offsetY: -20))
        #expect(
            try row.find(viewWithAccessibilityLabel: "ひだりの字幕の横のずらし（ピクセル）").textField()
                .input() == "15")
        try row.find(button: "戻す").tap()
        #expect(model.speakerOptions("left") == nil)
        #expect(throws: Never.self) { try options.inspect().find(text: "話者ごとの位置") }
    }

    @Test(.timeLimit(.minutes(1)))
    func theOverlayAndTheExportDrawTheShownSpeakersOverTheirIcons() async throws {
        let fixture = try CaptionPlanFixture(speakers: true)
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        try await writeCaptionMovie(fixture.movie, seconds: 10)
        let model = await Self.opened(fixture)
        model.show("left", false)
        let overlay = CaptionOverlayView(frame: NSRect(x: 0, y: 0, width: 1920, height: 1080))
        overlay.apply(model.overlaySettings, player: AVPlayer())
        // Frame 55: middle and right speak, left (hidden) would too; the icons lie under them.
        overlay.draw(atSeconds: 55.0 / 30)
        #expect(overlay.placed.count == 3)
        #expect(overlay.placed.first?.rect == CGRect(x: 0, y: 0, width: 1920, height: 1080))
        overlay.stopObserving()
        let export = ExportModel()
        export.workspace = TestWorkspace()
        export.open(fixture.movie)
        export.ranges = [ExportRange(start: 2, end: 3), ExportRange(start: 0.5, end: 1)]
        let output = fixture.folder.appending(path: "speakers.mp4")
        export.export(to: output, captions: model)
        await export.task?.value
        #expect(export.status == "書き出しました: speakers.mp4")
        let frames = try await AVURLAsset(url: output).load(.duration).seconds * 30
        #expect(abs(frames - 45) < 1)
    }
}

/// No caption or export control is a pop-up menu: AppKit traps in the Swift runtime when it
/// highlights a menu item titled with a non-ASCII Swift string (TranscriptViewer crash of
/// 2026-11-03, `-[NSMenuItem _description:]`). The menus are never opened here.
@MainActor
struct MenuFreeControlsTests {
    private static func hosted(_ view: some View) -> NSView {
        let host = NSHostingView(rootView: view)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 600, height: 1200), styleMask: [.titled],
            backing: .buffered, defer: false)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        return host
    }

    private static func all(_ view: NSView) -> [NSView] { [view] + view.subviews.flatMap(all) }

    @Test func positionChoicesAreSegmentsWithoutFacesDisabled() async throws {
        let fixture = try CaptionPlanFixture(speakers: true)
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = CaptionsModel()
        model.open(fixture.movie)
        await model.loading?.value
        let views = Self.all(Self.hosted(CaptionStyleOptions(model: model).padding()))
        #expect(!views.contains { $0 is NSPopUpButton })
        let segments = views.compactMap { $0 as? NSSegmentedControl }
        #expect(segments.count == 4)
        let whole = try #require(segments.first)
        #expect(
            (0..<whole.segmentCount).map { whole.label(forSegment: $0) }
                == ["今の位置", "顔の上", "下部", "上部", "中央"])
        #expect(!whole.isEnabled(forSegment: 1))
        #expect(whole.isEnabled(forSegment: 2))
        #expect(segments.last?.label(forSegment: 0) == "全体と同じ")
    }

    @Test func clipsAreAddedFromButtonsNotAMenu() throws {
        let session = PlaybackSession()
        defer {
            session.player.pause()
            session.stopObserving()
        }
        session.export.open(URL(fileURLWithPath: "/v/a.mp4"))
        let panel = ExportPanel(session: session)
        let views = Self.all(Self.hosted(panel.frame(width: 600, height: 400)))
        #expect(!views.contains { $0 is NSPopUpButton })
        panel.showClips()
        #expect(throws: Never.self) { try panel.clipList().inspect().find(ViewType.VStack.self) }
    }
}
