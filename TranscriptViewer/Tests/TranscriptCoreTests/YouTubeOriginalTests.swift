import Foundation
import SwiftUI
import Testing
import ViewInspector

@testable import TranscriptCore
@testable import TranscriptSummaries
@testable import TranscriptViewer

/// Unedited YouTube originals: listed playable-only, replaced by any delivery, never annotated.
struct YouTubeOriginalTests {
    private static let stamp = "2026-11-02_23-00-00"
    private static let folder = "\(stamp)_JST_youtube-eWgX9jkjNpA"

    private static func makeRoot(_ name: String) throws -> URL {
        let root = try transcriptTestWorkRoot().appending(
            path: "\(name)-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        return root
    }

    private static func original(_ name: String, in root: URL, files: [String]) throws -> URL {
        let folder = root.appending(path: name, directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for file in files { try Data().write(to: folder.appending(path: file)) }
        return folder
    }

    static func originalVideo(_ id: String = "eWgX9jkjNpA") -> VideoTranscript {
        let name = "\(stamp)_JST_youtube-\(id)"
        return VideoTranscript(
            title: "youtube-\(id) [未編集・字幕なし]",
            movieURL: URL(fileURLWithPath: "/originals/\(name)/\(name).mp4"),
            transcriptURL: URL(fileURLWithPath: "/dev/null"), cues: [],
            broadcastStartJST: "2026-11-02 23:00:00 JST", isOriginalOnly: true)
    }

    private static func delivery(_ title: String, movie: String) -> VideoTranscript {
        VideoTranscript(
            title: title, movieURL: URL(fileURLWithPath: "/deliveries/\(title)/\(movie).mp4"),
            transcriptURL: URL(fileURLWithPath: "/deliveries/\(title)/\(movie).srt"), cues: [],
            broadcastStartJST: "2026-11-02 23:00:00 JST")
    }

    @Test func originalsRootListsOnlyPlayableOriginalsAndReportsNothing() throws {
        let root = try Self.makeRoot("youtube-originals")
        defer { #expect(throws: Never.self) { try FileManager.default.removeItem(at: root) } }
        let listed = try Self.original(
            Self.folder, in: root,
            files: ["\(Self.folder).mp4", "publication.json", "channel.json"])
        // A delivered original keeps work files beside its movie.
        let worked = try Self.original(
            "2026-10-12_22-29-14_JST_youtube-grcUT0ZMuKU", in: root,
            files: ["2026-10-12_22-29-14_JST_youtube-grcUT0ZMuKU.mp4"])
        _ = try Self.original(
            "work", in: worked, files: ["2026-10-12_22-29-14_JST_youtube-grcUT0ZMuKU-x.mp4"])
        _ = try Self.original(
            "2026-09-29_22-00-14_JST_youtube-3MpwpTk1wxs", in: root,
            files: ["2026-09-29_22-00-14_JST_youtube-3MpwpTk1wxs.mkv"])
        _ = try Self.original(
            "2026-10-06_07-30-09_JST_youtube-Lp6Wje9ktVs", in: root, files: ["publication.json"])
        _ = try Self.original(
            "2026-10-07_07-30-09_JST_youtube-twoMoviesAA", in: root,
            files: [
                "2026-10-07_07-30-09_JST_youtube-twoMoviesAA.mp4",
                "2026-10-07_07-30-09_JST_youtube-twoMoviesAA.mov",
            ])
        _ = try Self.original(
            ".\(Self.folder).downloading.partial", in: root, files: ["\(Self.folder).mp4"])
        try Data().write(to: root.appending(path: ".youtube-eWgX9jkjNpA.lock"))
        try Data().write(to: root.appending(path: "lL4opb4iB3w.mkv"))

        let scan = try TranscriptCatalog.scan(in: root)

        #expect(scan.issues.isEmpty)
        #expect(
            scan.videos.map(\.title) == [
                "youtube-eWgX9jkjNpA [未編集・字幕なし]", "youtube-grcUT0ZMuKU [未編集・字幕なし]",
            ])
        let video = try #require(scan.videos.first)
        #expect(video.isOriginalOnly)
        #expect(video.cues.isEmpty)
        #expect(video.movieURL == listed.appending(path: "\(Self.folder).mp4"))
        #expect(video.transcriptURL == URL(fileURLWithPath: "/dev/null"))
        #expect(video.broadcastStartJST == "2026-11-02 23:00:00 JST")
        #expect(video.recordingID == nil)
    }

    @Test func anOriginalFolderWithATranscriptStaysAnOrdinaryFolder() throws {
        let root = try Self.makeRoot("youtube-original-transcript")
        defer { #expect(throws: Never.self) { try FileManager.default.removeItem(at: root) } }
        let folder = try Self.original(Self.folder, in: root, files: ["\(Self.folder).mp4"])
        try Data(
            "start_seconds,end_seconds,start,end,text\n0.000,1.000,00:00:00.000,00:00:01.000,hi\n"
                .utf8
        ).write(to: folder.appending(path: "\(Self.folder).csv"))

        let scan = try TranscriptCatalog.scan(in: root)

        #expect(scan.videos.map(\.title) == [Self.folder])
        #expect(scan.videos.first?.isOriginalOnly == false)
        #expect(scan.videos.first?.cues.count == 1)
    }

    @Test func onlyCanonicalOriginalFolderNamesAreOriginals() {
        #expect(TranscriptCatalog.originalYouTubeRecording(Self.folder) == "youtube-eWgX9jkjNpA")
        for name in [
            "2026-13-02_23-00-00_JST_youtube-eWgX9jkjNpA",
            "2026-11-02_23-00-00_UTC_youtube-eWgX9jkjNpA",
            "2026-11-02_23-00-00_JST_replay-123", "2026-11-02_23-00-00_JST_youtube-short",
            "2026-11-02_23-00-00_JST_youtube-eWgX9jkjNpA-edit", "youtube-eWgX9jkjNpA",
        ] {
            #expect(TranscriptCatalog.originalYouTubeRecording(name) == nil)
        }
    }

    @Test(arguments: [false, true])
    func anyDeliveryOfTheRecordingHidesItsOriginalInEitherOrder(originalsFirst: Bool) {
        var originals = CatalogScan()
        originals.videos = [
            Self.originalVideo(), Self.originalVideo("grcUT0ZMuKU"),
            Self.originalVideo("AdX0AIxo4Ww"),
            Self.originalVideo("keptKeptKep"),
        ]
        var deliveries = CatalogScan()
        deliveries.videos = [
            Self.delivery(
                "youtube-eWgX9jkjNpA/pipeline-1",
                movie: "\(Self.stamp)_JST_youtube-eWgX9jkjNpA-review"),
            // An `-edit-` delivery has no canonical name; its folder names the recording.
            Self.delivery(
                "youtube-grcUT0ZMuKU",
                movie: "2026-10-12_22-29-14_JST_youtube-grcUT0ZMuKU-edit-dagq3qvz"),
            // An excerpt is not a delivery of the whole original.
            Self.delivery(
                "youtube-AdX0AIxo4Ww-vertical-0358-1400",
                movie: "\(Self.stamp)_JST_youtube-AdX0AIxo4Ww-vertical-0358-1400-review"),
        ]
        var merged = CatalogScan()
        for scan in originalsFirst ? [originals, deliveries] : [deliveries, originals] {
            merged.merge(scan)
        }
        #expect(
            Set(merged.videos.filter(\.isOriginalOnly).map(\.title)) == [
                "youtube-AdX0AIxo4Ww [未編集・字幕なし]", "youtube-keptKeptKep [未編集・字幕なし]",
            ])
        #expect(merged.videos.count == 5)
        #expect(merged.issues.isEmpty)
    }

    @Test func summariesNeverStartOrWriteForAnOriginal() async throws {
        let root = try Self.makeRoot("youtube-original-summary")
        defer { #expect(throws: Never.self) { try FileManager.default.removeItem(at: root) } }
        _ = try Self.original(Self.folder, in: root, files: ["\(Self.folder).mp4"])
        let video = try #require(try TranscriptCatalog.scan(in: root).videos.first)
        let calls = OriginalCalls()
        let service = AppleSummaryService(store: SummaryStore()) { _ in
            await calls.add()
            return "要約"
        }
        await #expect(throws: SummaryStoreError.unsafePath) {
            try await service.generate(video: video, force: true) { _, _ in }
        }
        #expect(await calls.count == 0)
        #expect(!FileManager.default.fileExists(atPath: "/dev/null.summaries"))

        let command = try SummaryCommand(arguments: ["generate", "--library", root.path])
        await #expect(throws: SummaryCommandError.self) {
            _ = try await command.run(respond: { _ in "要約" }, output: { _ in })
        }
        #expect(
            try FileManager.default.contentsOfDirectory(
                atPath: root.appending(path: Self.folder).path
            ) == ["\(Self.folder).mp4"])
    }
}

actor OriginalCalls {
    private(set) var count = 0
    func add() { count += 1 }
}

@MainActor
struct YouTubeOriginalViewerTests {
    @Test func selectingAnOriginalStartsNoSummaryClipsOrBGM() async throws {
        let calls = OriginalCalls()
        let summary = NarrativeSummaryModel(
            service: AppleSummaryService { _ in
                await calls.add()
                return "要約"
            })
        let fixture = try BGMFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        try fixture.add()
        let library = BGMLibraryModel(root: fixture.root, volume: 1, defaultID: "CYzeUrG3p9o")
        var saved: [[String: VideoBGMSetting]] = []
        let bgm = VideoBGMModel(library: library, settings: [:])
        bgm.store = { saved.append($0) }
        bgm.loadLevels = { _ in nil }
        let clips = ClipsModel(api: ClipsModelTests.api)
        let session = PlaybackSession(narrativeSummary: summary, videoBGM: bgm, clips: clips)
        defer {
            session.player.pause()
            session.stopObserving()
        }
        let original = YouTubeOriginalTests.originalVideo()
        session.videos = [original]
        session.selectedVideoID = original.id
        session.openSelection()

        #expect(clips.state == .idle)
        #expect(!clips.canReload)
        #expect(bgm.key == nil)
        bgm.videoPlaybackChanged(true)
        #expect(bgm.playingID == nil)
        bgm.select(.track("CYzeUrG3p9o"))
        #expect(saved.isEmpty)

        await summary.load(video: original)
        #expect(summary.state == .idle)
        await summary.generateLibrary(videos: [original])
        #expect(summary.libraryProgress?.total == 0)
        #expect(await calls.count == 0)

        let root = TranscriptRootView(session: session)
        // The default panel factory is only constructed, never shown.
        #expect(root.directoryPanel().canChooseFiles)
        let view = try root.inspect()
        #expect(throws: Never.self) {
            try view.find(viewWithAccessibilityIdentifier: "original-only-note")
        }
        #expect(throws: (any Error).self) { try view.find(NarrativeSummaryView.self) }
        #expect(throws: (any Error).self) { try view.find(VideoBGMControls.self) }
    }
}
