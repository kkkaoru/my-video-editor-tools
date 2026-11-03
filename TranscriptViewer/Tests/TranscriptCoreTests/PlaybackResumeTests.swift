import AVFoundation
import AppKit
import SwiftUI
import Testing
import ViewInspector

@testable import TranscriptCore
@testable import TranscriptViewer

struct PlaybackResumeTests {
    @Test func savedPositionsResumeOnlyAwayFromBothEnds() {
        #expect(PlaybackResume.start(saved: nil, duration: 100) == 0)
        #expect(PlaybackResume.start(saved: 3, duration: 100) == 0)
        #expect(PlaybackResume.start(saved: 42, duration: 100) == 42)
        #expect(PlaybackResume.start(saved: 96, duration: 100) == 0)
        #expect(PlaybackResume.start(saved: 150, duration: 100) == 0)
        #expect(PlaybackResume.start(saved: .nan, duration: 100) == 0)
        #expect(PlaybackResume.start(saved: 42, duration: .nan) == 0)
        let delivery = URL(fileURLWithPath: "/d/youtube-a/x/../2026_JST_youtube-a-review.mp4")
        let original = URL(fileURLWithPath: "/o/2026_JST_youtube-a/2026_JST_youtube-a.mp4")
        #expect(PlaybackResume.key(for: delivery) == "/d/youtube-a/2026_JST_youtube-a-review.mp4")
        #expect(PlaybackResume.key(for: delivery) != PlaybackResume.key(for: original))
    }

    @Test func playerHeightStaysWithinItsRange() {
        #expect(Chrome.clampedPlayerHeight(400) == 400)
        #expect(Chrome.clampedPlayerHeight(10) == Chrome.playerHeightRange.lowerBound)
        #expect(Chrome.clampedPlayerHeight(5_000) == Chrome.playerHeightRange.upperBound)
        #expect(Chrome.clampedPlayerHeight(.infinity) == Double(Chrome.playerHeight))
    }

    @Test func mountPointsAreVolumesAndFoldersAreNot() throws {
        let folder = try transcriptTestWorkRoot()
        #expect(PlaybackSession.isMountPoint(URL(fileURLWithPath: "/")))
        #expect(!PlaybackSession.isMountPoint(folder))
        #expect(!PlaybackSession.isMountPoint(folder.appending(path: "missing-\(UUID())")))
    }
}

@MainActor
struct PlaybackResumeSessionTests {
    private static func suite() throws -> (UserDefaults, String) {
        let name = "PlaybackResumeTests-\(UUID().uuidString)"
        return (try #require(UserDefaults(suiteName: name)), name)
    }

    private static func box(_ height: Double) -> (Binding<Double>, () -> Double) {
        final class Box {
            var value: Double
            init(_ value: Double) { self.value = value }
        }
        let box = Box(height)
        return (Binding(get: { box.value }, set: { box.value = $0 }), { box.value })
    }

    @Test func positionsAreSavedOnSeekThrottledAndKeptPerMovie() throws {
        let fixture = try SummaryFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let (defaults, name) = try Self.suite()
        defer { defaults.removePersistentDomain(forName: name) }
        let application = NotificationCenter()
        let session = PlaybackSession(positionDefaults: defaults, application: application)
        defer { session.stopObserving() }
        let key = PlaybackResume.key(for: fixture.video.movieURL)
        session.recordPosition(at: 1)
        #expect(session.positions.isEmpty)
        session.videos = [fixture.video]
        session.selectedVideoID = fixture.video.id
        session.openSelection()
        session.seek(to: 0.5)
        session.player.pause()
        #expect(session.positions[key] == 0.5)
        session.recordPositionIfDue(3)
        #expect(session.positions[key] == 0.5)
        session.recordPositionIfDue(6)
        #expect(session.positions[key] == 6)
        session.recordPosition(at: .nan)
        #expect(session.positions[key] == 6)
        #expect(PlaybackSession.savedPositions(defaults) == [key: 6])
        application.post(name: NSApplication.willTerminateNotification, object: nil)
        #expect(session.positions[key] != nil)

        // A saved position is not overwritten before it is applied; an explicit seek wins.
        let reopened = PlaybackSession(positionDefaults: defaults)
        defer { reopened.stopObserving() }
        #expect(reopened.positions[key] != nil)
        let saved = reopened.positions[key]
        reopened.videos = [fixture.video]
        reopened.selectedVideoID = fixture.video.id
        reopened.openSelection()
        reopened.recordPosition(at: 0)
        #expect(reopened.positions[key] == saved)
        reopened.skip(by: 0)
        #expect(reopened.positions[key] == 0)
        reopened.player.pause()
    }

    @Test func anUnpluggedVolumeKeepsTheListAndReopensTheSelectionWhenMounted() throws {
        let fixture = try SummaryFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let workspace = NotificationCenter()
        var mounted = false
        var scans = 0
        let session = PlaybackSession(
            scanCatalog: { _, completion in
                scans += 1
                var scan = CatalogScan()
                scan.videos = [fixture.video]
                completion(.success(scan))
            },
            workspace: workspace, isMountPoint: { _ in mounted })
        defer {
            session.player.pause()
            session.stopObserving()
        }
        session.load(root: fixture.root)
        #expect(scans == 1)
        #expect(session.player.currentItem != nil)
        session.seek(to: 0.5)
        session.player.pause()

        let post = { (name: Notification.Name, volume: URL?) in
            workspace.post(
                name: name, object: nil,
                userInfo: volume.map { [NSWorkspace.volumeURLUserInfoKey: $0] })
        }
        post(NSWorkspace.didUnmountNotification, nil)
        post(NSWorkspace.didUnmountNotification, URL(fileURLWithPath: "/Volumes/other"))
        #expect(session.volumeNotice == nil)
        post(NSWorkspace.didUnmountNotification, fixture.root)
        #expect(session.player.currentItem == nil)
        #expect(session.isUnavailable(fixture.video))
        #expect(
            session.volumeNotice?.hasPrefix("SSD 未接続: \(fixture.root.lastPathComponent)") == true)
        #expect(session.selectedVideoID == fixture.video.id)
        #expect(session.positions[PlaybackResume.key(for: fixture.video.movieURL)] == 0.5)
        session.refreshLibrary()
        session.openSelection()
        #expect(scans == 1)
        #expect(session.player.currentItem == nil)

        post(NSWorkspace.didMountNotification, fixture.root)
        #expect(scans == 1)
        mounted = true
        post(NSWorkspace.didMountNotification, URL(fileURLWithPath: "/Volumes/other"))
        post(NSWorkspace.didMountNotification, fixture.root)
        #expect(scans == 2)
        #expect(session.volumeNotice == nil)
        #expect(session.selectedVideoID == fixture.video.id)
        let item = try #require(session.player.currentItem)
        #expect((item.asset as? AVURLAsset)?.url == fixture.video.movieURL)
        #expect(session.player.rate == 0)
    }

    @Test func theHandleResizesByDragAndAdjustmentWithinRange() throws {
        let (binding, height) = Self.box(320)
        let handle = PlayerHeightHandle(height: binding)
        let shape = try handle.inspect().find(
            viewWithAccessibilityIdentifier: "player-height-handle")
        // ViewInspector cannot reach this SwiftUI's adjustable action; VoiceOver calls `adjust`.
        handle.adjust(.increment)
        #expect(height() == 360)
        handle.adjust(.decrement)
        #expect(height() == 320)
        #expect(MemoryLayout<DragGesture.Value>.size == 56)
        let drag = try shape.gesture(DragGesture.self)
        let value = DragGesture.Value(
            time: Date(), location: CGPoint(x: 0, y: 2_000), startLocation: .zero,
            velocity: .zero)
        try drag.callOnChanged(value: value)
        #expect(height() == Chrome.playerHeightRange.upperBound)
        try drag.callOnEnded(value: value)
    }

    @Test func rootViewShowsTheHandleNoticesAndRoutesPanelActions() async throws {
        let fixture = try SummaryFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let workspace = NotificationCenter()
        let session = PlaybackSession(workspace: workspace)
        defer {
            session.player.pause()
            session.stopObserving()
        }
        session.videos = [fixture.video]
        session.selectedVideoID = fixture.video.id
        session.openSelection()
        session.detailTab = .clips
        let root = TranscriptRootView(session: session)
        let view = try root.inspect()
        #expect(throws: Never.self) {
            try view.find(viewWithAccessibilityIdentifier: "player-height-handle")
        }
        let clips = try view.find(ClipsPanel.self).actualView()
        // The unplayable fixture's timeline is its one-second transcript.
        clips.play(twoRangeClip, 0, true)
        #expect(session.clips.cursor?.clip == twoRangeClip)
        clips.seek(0.25)
        #expect(session.currentSeconds == 0.25)
        try view.find(ExcitementSection.self).actualView().seek(0.5)
        #expect(session.currentSeconds == 0.5)
        try await view.find(ViewType.List.self).callTask(id: session.videos.map(\.title))

        workspace.post(
            name: NSWorkspace.didUnmountNotification, object: nil,
            userInfo: [NSWorkspace.volumeURLUserInfoKey: fixture.root])
        let unplugged = try TranscriptRootView(session: session).inspect()
        #expect(throws: Never.self) {
            try unplugged.find(viewWithAccessibilityIdentifier: "volume-notice")
        }
        #expect(throws: Never.self) {
            try unplugged.find(text: "SSD 未接続のため再生できません。接続し直すと続きから開きます。")
        }
    }
}
