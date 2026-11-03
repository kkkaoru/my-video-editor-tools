import AVKit
import AppKit
import CaptionRender
import SwiftUI
import Testing
import TranscriptCore
import ViewInspector

@testable import TranscriptViewer

/// A silent 1920×1080 30 fps colour movie (10 s: the fixture plan's length); never a recording.
@concurrent
func writeCaptionMovie(_ movie: URL, seconds: Int = 4) async throws {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
    process.arguments = [
        "ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i",
        "color=c=gray:s=1920x1080:r=30:d=\(seconds)", "-c:v", "libx264", "-pix_fmt", "yuv420p",
        movie.path,
    ]
    process.standardOutput = FileHandle.nullDevice
    process.standardError = FileHandle.nullDevice
    let status = try await withCheckedThrowingContinuation {
        (continuation: CheckedContinuation<Int32, any Error>) in
        process.terminationHandler = { continuation.resume(returning: $0.terminationStatus) }
        do {
            try process.run()
        } catch {
            process.terminationHandler = nil
            continuation.resume(throwing: error)
        }
    }
    #expect(status == 0)
}

@MainActor
struct CaptionsModelTests {
    private static func suite() throws -> (UserDefaults, String) {
        let name = "CaptionsModelTests-\(UUID().uuidString)"
        return (try #require(UserDefaults(suiteName: name)), name)
    }

    /// The model with `movie`'s plan opened and loaded.
    private static func opened(_ movie: URL, defaults: UserDefaults? = nil) async throws
        -> CaptionsModel
    {
        let model = CaptionsModel(defaults: defaults)
        model.open(movie)
        await model.loading?.value
        return model
    }

    @Test func captionsDefaultOnUnlessBurnedAndEachMovieKeepsItsChoice() async throws {
        let plain = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try plain.remove() } }
        let burned = try CaptionPlanFixture([("\"burned\": false", "\"burned\": true")])
        defer { #expect(throws: Never.self) { try burned.remove() } }
        let (defaults, name) = try Self.suite()
        defer { defaults.removePersistentDomain(forName: name) }
        let model = try await Self.opened(plain.movie, defaults: defaults)
        #expect(model.isShown)
        model.isShown = false
        model.open(burned.movie)
        #expect(!model.isShown)
        await model.loading?.value
        #expect(!model.isShown)
        model.isShown = true
        let reopened = try await Self.opened(plain.movie, defaults: defaults)
        #expect(!reopened.isShown)
        reopened.open(burned.movie)
        await reopened.loading?.value
        #expect(reopened.isShown)
        reopened.open(nil)
        #expect(reopened.captions == nil)
        reopened.isShown = true
        #expect(!reopened.isShown)
    }

    @Test func aFailedLoadIsReportedAndAStaleLoadIsDropped() async throws {
        let fixture = try CaptionPlanFixture([("synthetic-review.mp4", "other.mp4")])
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = try await Self.opened(fixture.movie)
        #expect(model.captions == nil)
        #expect(model.issue == "字幕プランが別の動画（other.mp4）のものです")
        model.report("描画失敗")
        #expect(model.issue == "描画失敗")
        let plain = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try plain.remove() } }
        model.open(plain.movie)
        let stale = model.loading
        model.open(nil)
        await stale?.value
        #expect(model.captions == nil)
        #expect(model.issue == nil)
    }

    @Test func fillStaysThePlansUntilChangedAndResetsToIt() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let (defaults, name) = try Self.suite()
        defer { defaults.removePersistentDomain(forName: name) }
        let model = try await Self.opened(fixture.movie, defaults: defaults)
        #expect(model.fill == nil)
        #expect(model.top == "#97D8F7")
        #expect(model.bottom == "#2FB0EE")
        model.usesGradient = false
        #expect(model.fill == CaptionFill(top: "#97D8F7", bottom: "#2FB0EE", gradient: false))
        model.topColour = "#FF0000"
        model.showsNameplate = false
        let saved = CaptionsModel(defaults: defaults)
        #expect(saved.topColour == "#FF0000")
        #expect(!saved.usesGradient)
        #expect(!saved.showsNameplate)
        #expect(
            model.overlaySettings.fill
                == CaptionFill(top: "#FF0000", bottom: "#2FB0EE", gradient: false))
        #expect(model.overlaySettings.showsCaptions)
        #expect(model.overlaySettings.underlay == nil)
        model.reset()
        #expect(model.fill == nil)
        #expect(model.showsNameplate)
        #expect(CaptionsModel(defaults: defaults).topColour == nil)
        model.open(nil)
        #expect(model.top == "#FFFFFF")
        #expect(model.bottom == "#FFFFFF")
    }

    @Test func positionEditsAreSavedBesideTheMovieAndFailuresReported() async throws {
        let fixture = try CaptionPlanFixture(siblings: ["caption-faces-v1.json": ".faces.json"])
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = CaptionsModel()
        model.place(.bottom)
        #expect(model.layout == .plan)
        model.open(fixture.movie)
        await model.loading?.value
        #expect(model.hasFaces)
        model.place(.bottom)
        model.nudge(x: 10)
        model.nudge(y: -10)
        model.setOffset(y: 25)
        #expect(model.layout == CaptionLayout(mode: .bottom, offsetX: 10, offsetY: 25))
        #expect(model.overlaySettings.plan == model.placed)
        let saved = try CaptionLayout.decode(
            Data(contentsOf: CaptionLayoutFile.url(for: fixture.movie)))
        #expect(saved == model.layout)
        model.setOffset(x: -5)
        #expect(model.layout.offsetX == -5)
        model.resetPosition()
        #expect(model.layout == .plan)
        let reopened = CaptionsModel()
        reopened.open(fixture.movie)
        await reopened.loading?.value
        #expect(reopened.layout == .plan)
        // A folder that cannot be written keeps the position and says why.
        try FileManager.default.setAttributes(
            [.posixPermissions: 0o500], ofItemAtPath: fixture.folder.path)
        defer {
            #expect(throws: Never.self) {
                try FileManager.default.setAttributes(
                    [.posixPermissions: 0o700], ofItemAtPath: fixture.folder.path)
            }
        }
        reopened.place(.center)
        #expect(reopened.layout == .plan)
        #expect(reopened.issue?.hasPrefix("字幕位置を保存できません") == true)
    }

    @Test func storedColoursMustBeHexAndRoundTripThroughThePicker() throws {
        let (defaults, name) = try Self.suite()
        defer { defaults.removePersistentDomain(forName: name) }
        defaults.set("red", forKey: CaptionsModel.topKey)
        defaults.set("#00FF00", forKey: CaptionsModel.bottomKey)
        let model = CaptionsModel(defaults: defaults)
        #expect(model.topColour == nil)
        #expect(model.bottomColour == "#00FF00")
        #expect(CaptionsModel.colour("#97D8F7").flatMap(CaptionsModel.hex) == "#97D8F7")
        #expect(CaptionsModel.colour("#2FB0EE").flatMap(CaptionsModel.hex) == "#2FB0EE")
        #expect(CaptionsModel.colour("97D8F7") == nil)
        #expect(CaptionsModel.colour("#GGGGGG") == nil)
        #expect(CaptionsModel.hex(NSColor(srgbRed: 2, green: -1, blue: 0.5, alpha: 1)) == "#FF0080")
        #expect(
            CaptionsModel.hex(NSColor(patternImage: NSImage(size: NSSize(width: 1, height: 1))))
                == nil)
    }
}

@MainActor
struct CaptionOverlayViewTests {
    private static func loaded(_ fixture: CaptionPlanFixture) throws -> DeliveryCaptions {
        try #require(
            try DeliveryCaptions.load(
                movie: fixture.movie,
                fontRoots: [CaptionPlanFixture.font.deletingLastPathComponent()]))
    }

    /// What the captions model hands the player: the plan placed by `layout`.
    private static func settings(
        _ captions: DeliveryCaptions, layout: CaptionLayout = .plan, shows: Bool = true,
        underlay: Bool = true, fill: CaptionFill? = nil, revision: Int = 0
    ) throws -> CaptionOverlaySettings {
        CaptionOverlaySettings(
            captions: captions,
            plan: try CaptionPlacement.resolve(
                plan: captions.plan, layout: layout, faces: captions.faces),
            revision: revision, fill: fill, showsCaptions: shows,
            underlay: underlay ? try captions.nameplate.map(CaptionUnderlayImage.init) : nil)
    }

    @Test func layersFollowTheVisibleTitlesOnTheVideoAndNeverTakeClicks() throws {
        let fixture = try CaptionPlanFixture(try CaptionPlanFixture.realFont())
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let captions = try Self.loaded(fixture)
        let player = AVPlayer()
        let overlay = CaptionOverlayView(frame: NSRect(x: 0, y: 0, width: 960, height: 540))
        #expect(overlay.hitTest(NSPoint(x: 10, y: 10)) == nil)
        overlay.apply(
            try Self.settings(captions), player: player)
        // Frame 0: only the nameplate.
        #expect(overlay.placed.count == 1)
        overlay.draw(atSeconds: 60.0 / 30)
        #expect(overlay.placed.count == 2)
        let caption = try #require(overlay.placed.last)
        // A 1920×1080 canvas fills the 960×540 view.
        let video = CGRect(x: 0, y: 0, width: 960, height: 540)
        #expect(
            caption.layer.frame
                == CaptionGeometry.frame(
                    of: caption.rect, canvas: CGSize(width: 1920, height: 1080), in: video,
                    flipped: overlay.isFlipped))
        let image = try #require(overlay.images.last)
        overlay.draw(atSeconds: 61.0 / 30)
        #expect(overlay.images.last === image)
        // A wider view pillarboxes the canvas, centred.
        overlay.setFrameSize(NSSize(width: 1200, height: 540))
        #expect(caption.layer.frame.minX == 120 + caption.rect.minX / 2)
        overlay.draw(atSeconds: 200.0 / 30)
        #expect(overlay.placed.count == 1)
        overlay.apply(try Self.settings(captions, underlay: false), player: player)
        #expect(overlay.placed.isEmpty)
        overlay.apply(try Self.settings(captions, shows: false), player: player)
        #expect(overlay.placed.isEmpty)
        overlay.draw(atSeconds: 2)
        #expect(overlay.placed.isEmpty)
        overlay.stopObserving()
    }

    @Test(.timeLimit(.minutes(1)))
    func aPlayingMovieDrivesTheCaptionsFrameByFrame() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        try await writeCaptionMovie(fixture.movie)
        let captions = try Self.loaded(fixture)
        let item = AVPlayerItem(url: fixture.movie)
        let player = AVPlayer(playerItem: item)
        let overlay = CaptionOverlayView(frame: NSRect(x: 0, y: 0, width: 480, height: 270))
        overlay.apply(try Self.settings(captions), player: player)
        // Frame 60 shows the second title; the seek fires the frame observer.
        _ = await player.seek(
            to: CMTime(value: 2, timescale: 1), toleranceBefore: .zero, toleranceAfter: .zero)
        while item.presentationSize == .zero { await Task.yield() }
        for _ in 0..<100 where overlay.placed.isEmpty { await Task.yield() }
        #expect(overlay.placed.count == 1)
        #expect(overlay.placed.first?.rect.minX ?? 0 > 0)
        overlay.stopObserving()
    }

    @Test func aNewPlacementRebuildsTheCaptionsAndNoPlanDrawsNothing() throws {
        let fixture = try CaptionPlanFixture(try CaptionPlanFixture.realFont())
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let captions = try Self.loaded(fixture)
        let overlay = CaptionOverlayView(frame: NSRect(x: 0, y: 0, width: 1920, height: 1080))
        overlay.apply(try Self.settings(captions, underlay: false), player: AVPlayer())
        overlay.draw(atSeconds: 2)
        let planned = try #require(overlay.placed.first?.rect)
        let moved = CaptionLayout(mode: .plan, offsetX: 30, offsetY: 40)
        overlay.apply(
            try Self.settings(captions, layout: moved, underlay: false, revision: 1),
            player: AVPlayer())
        overlay.draw(atSeconds: 2)
        let shifted = try #require(overlay.placed.first?.rect)
        #expect(abs(shifted.minX - planned.minX - 30) < 1)
        #expect(abs(shifted.minY - planned.minY - 40) < 1)
        // The model could not place the plan: nothing is drawn.
        var unplaced = try Self.settings(captions, revision: 2)
        unplaced.plan = nil
        overlay.apply(unplaced, player: AVPlayer())
        #expect(overlay.placed.isEmpty)
        overlay.stopObserving()
    }

    @Test func aTitleThatCannotBeDrawnIsReportedAndHidden() throws {
        let fixture = try CaptionPlanFixture([
            ("\"fontName\": \"NotoSansJP-Thin_Bold\"", "\"fontName\": \"NoSuchCaptionFont-Bold\"")
        ])
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let captions = try Self.loaded(fixture)
        var reported: [String] = []
        let overlay = CaptionOverlayView(frame: .zero)
        CaptionOverlayView(frame: .zero).report("ignored")
        overlay.report = { reported.append($0) }
        overlay.apply(
            try Self.settings(captions, underlay: false), player: AVPlayer())
        overlay.draw(atSeconds: 2)
        #expect(reported.count == 1)
        #expect(overlay.placed.isEmpty)
        overlay.draw(atSeconds: 2.1)
        #expect(reported.count == 1)
    }

    @Test func anUndrawableFillIsReportedAndShowsNothing() throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let captions = try Self.loaded(fixture)
        var reported: [String] = []
        let overlay = CaptionOverlayView(frame: .zero)
        overlay.report = { reported.append($0) }
        overlay.apply(
            try Self.settings(
                captions, fill: CaptionFill(top: "red", bottom: "#000000", gradient: true)),
            player: AVPlayer())
        #expect(reported.count == 1)
        #expect(overlay.placed.isEmpty)
        let archiver = NSKeyedArchiver(requiringSecureCoding: false)
        archiver.finishEncoding()
        #expect(
            CaptionOverlayView(coder: try NSKeyedUnarchiver(forReadingFrom: archiver.encodedData))
                == nil)
    }

    @Test func thePlayerSurfaceHostsTheOverlayAboveTheVideoAndBelowTheControls() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let captions = try Self.loaded(fixture)
        let player = AVPlayer()
        let host = NSHostingView(
            rootView: PlayerSurface(
                player: player,
                captions: try Self.settings(captions)))
        host.setFrameSize(NSSize(width: 320, height: 180))
        host.layoutSubtreeIfNeeded()
        let native = try #require(find(AVPlayerView.self, in: host))
        let content = try #require(native.contentOverlayView)
        let overlay = try #require(content.subviews.first as? CaptionOverlayView)
        #expect(overlay.frame == content.bounds)
        #expect(overlay.settings.captions === captions)
        host.rootView = PlayerSurface(player: player)
        host.layoutSubtreeIfNeeded()
        #expect(overlay.settings.captions == nil)
        host.rootView = PlayerSurface(
            player: player,
            captions: try Self.settings(captions))
        host.layoutSubtreeIfNeeded()
        let coordinator = PlayerSurface.Coordinator()
        PlayerSurface.dismantleNSView(NSStackView(), coordinator: coordinator)
    }

    private func find<View: NSView>(_ type: View.Type, in root: NSView) -> View? {
        if let view = root as? View { return view }
        return root.subviews.lazy.compactMap { find(type, in: $0) }.first
    }
}

@MainActor
struct CaptionControlsTests {
    @Test func controlsAppearOnlyWithAPlanAndBindItsSettings() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = CaptionsModel()
        #expect(throws: (any Error).self) {
            try CaptionControls(model: model).inspect().find(ViewType.Toggle.self)
        }
        model.open(fixture.movie)
        await model.loading?.value
        let controls = CaptionControls(model: model)
        let toggle = try controls.inspect().find(ViewType.Toggle.self)
        #expect(try toggle.accessibilityLabel().string() == "字幕表示")
        try toggle.tap()
        #expect(!model.isShown)
        try controls.inspect().find(button: "字幕の見た目").tap()
        // The fixture's placeholder font digest leaves an icon problem to show.
        #expect(throws: Never.self) {
            try controls.inspect().find(ViewType.Image.self) {
                try $0.accessibilityLabel().string().hasPrefix("字幕の問題")
            }
        }
    }

    @Test func positionControlsMoveNudgeTypeAndResetTheCaptions() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = CaptionsModel()
        model.open(fixture.movie)
        await model.loading?.value
        let options = CaptionStyleOptions(model: model)
        let view = try options.inspect()
        #expect(throws: Never.self) {
            try view.find(text: "顔の位置データ（faces.json）がないため「顔の上」は使えません")
        }
        try view.find(ViewType.Picker.self).select(value: CaptionLayout.Mode.top)
        #expect(model.layout.mode == .top)
        for label in ["字幕を右へ10px", "字幕を下へ10px", "字幕を下へ10px", "字幕を左へ10px", "字幕を上へ10px"] {
            try view.find(viewWithAccessibilityLabel: label).button().tap()
        }
        #expect(model.layout.offsetX == 0)
        #expect(model.layout.offsetY == 10)
        options.offsetX.wrappedValue = 30
        options.offsetY.wrappedValue = -20
        #expect(options.offsetX.wrappedValue == 30)
        #expect(options.offsetY.wrappedValue == -20)
        try view.find(button: "位置をリセット").tap()
        #expect(model.layout == .plan)
        #expect(options.offsetX.wrappedValue == 0)
        #expect(CaptionStyleOptions.modes.map(\.id) == ["今の位置", "顔の上", "下部", "上部", "中央"])
        #expect(CaptionStyleOptions.nudges.map(\.id).count == 4)
    }

    @Test func styleOptionsEditAndResetTheLook() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let model = CaptionsModel()
        model.open(fixture.movie)
        await model.loading?.value
        #expect(throws: Never.self) {
            try CaptionControls(model: model).styleOptions().inspect().find(
                CaptionStyleOptions.self)
        }
        let options = CaptionStyleOptions(model: model)
        let view = try options.inspect()
        try view.find(
            ViewType.Toggle.self, where: { try $0.labelView().text().string() == "グラデーション" }
        )
        .tap()
        #expect(!model.usesGradient)
        let top = options.colour(\.top, \.topColour)
        #expect(CaptionsModel.hex(NSColor(top.wrappedValue)) == "#97D8F7")
        top.wrappedValue = Color(nsColor: NSColor(srgbRed: 1, green: 0, blue: 0, alpha: 1))
        #expect(model.topColour == "#FF0000")
        top.wrappedValue = Color(
            nsColor: NSColor(patternImage: NSImage(size: NSSize(width: 1, height: 1))))
        #expect(model.topColour == "#FF0000")
        model.bottomColour = "invalid"
        #expect(
            CaptionsModel.hex(NSColor(options.colour(\.bottom, \.bottomColour).wrappedValue))
                == "#FFFFFF")
        try view.find(button: "リセット").tap()
        #expect(model.topColour == nil)
        #expect(model.usesGradient)
        #expect(try view.find(ViewType.ColorPicker.self).accessibilityLabel().string() == "字幕の上の色")
    }
}

@MainActor
struct CaptionsSessionTests {
    @Test func selectionOpensThePlanAndALostVolumeOrOriginalClearsIt() throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let session = PlaybackSession(isMountPoint: { _ in true })
        defer {
            session.player.pause()
            session.stopObserving()
        }
        let video = VideoTranscript(
            title: "t", movieURL: fixture.movie, transcriptURL: fixture.plan, cues: [])
        let original = VideoTranscript(
            title: "o", movieURL: fixture.folder.appending(path: "o.mp4"),
            transcriptURL: fixture.plan, cues: [], isOriginalOnly: true)
        session.videos = [video, original]
        session.selectedVideoID = video.id
        session.openSelection()
        #expect(session.captions.movie == fixture.movie)
        session.volumeUnmounted(fixture.folder)
        #expect(session.captions.movie == nil)
        session.openSelection()
        #expect(session.captions.movie == nil)
        session.volumeMounted(fixture.folder)
        session.selectedVideoID = original.id
        session.openSelection()
        #expect(session.captions.movie == nil)
    }
}
