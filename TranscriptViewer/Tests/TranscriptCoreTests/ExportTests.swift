import AVFoundation
import AppKit
import CaptionRender
import SwiftUI
import Synchronization
import Testing
import TranscriptCore
import ViewInspector

@testable import TranscriptViewer

@MainActor
final class TestSavePanel: NSSavePanel {
    var chosenURL: URL?
    var response = NSApplication.ModalResponse.cancel
    override var url: URL? { chosenURL }
    override func runModal() -> NSApplication.ModalResponse { response }
}

/// Records what would be shown in Finder instead of showing it.
final class TestWorkspace: NSWorkspace {
    private let shown = Mutex<[URL]>([])
    var revealed: [URL] { shown.withLock { $0 } }
    override func activateFileViewerSelecting(_ fileURLs: [URL]) {
        shown.withLock { $0 += fileURLs }
    }
}

/// Collects what a fake exporter was asked to write.
actor ExportRecorder {
    private(set) var jobs: [ExportJob] = []
    func record(_ job: ExportJob) { jobs.append(job) }
}

@MainActor
struct ExportModelTests {
    static let roomy: @Sendable (URL) throws -> VolumeSpace = { _ in
        VolumeSpace(available: 1 << 40, total: 2 << 40, isStartup: true)
    }

    /// A model of `fixture`'s movie with its plan shown, and a recording fake exporter.
    private static func model(
        _ fixture: CaptionPlanFixture, recorder: ExportRecorder,
        space: @escaping @Sendable (URL) throws -> VolumeSpace = roomy,
        exporter: ExportModel.Exporter? = nil, workspace: TestWorkspace = TestWorkspace()
    ) async throws -> (ExportModel, CaptionsModel) {
        let captions = CaptionsModel()
        captions.open(fixture.movie)
        await captions.loading?.value
        let model = ExportModel(
            exporter: exporter ?? { job, progress in
                await recorder.record(job)
                progress(1, 2)
                progress(2, 2)
            }, space: space)
        model.workspace = workspace
        model.open(fixture.movie)
        return (model, captions)
    }

    @Test func rangesAreMarkedTakenFromClipsReorderedAndRemoved() {
        let model = ExportModel()
        model.open(URL(fileURLWithPath: "/v/a.mp4"))
        model.markEnd(at: 3)
        #expect(model.ranges.isEmpty)
        model.markStart(at: 10)
        model.markEnd(at: 10)
        #expect(model.ranges.isEmpty)
        model.markEnd(at: 4)
        #expect(model.ranges.map(\.start) == [4])
        #expect(model.ranges.map(\.end) == [10])
        #expect(model.pendingStart == nil)
        model.markStart(at: -1)
        #expect(model.pendingStart == 0)
        model.add(
            ClipPoint(
                id: "c", kind: "5-10", duration: 3,
                ranges: [ClipRange(start: 20, end: 21), ClipRange(start: 30, end: 32)],
                title: "t", summary: ""))
        #expect(model.ranges.map(\.start) == [4, 20, 30])
        model.move(from: [2], to: 0)
        #expect(model.ranges.map(\.start) == [30, 4, 20])
        #expect(model.duration == 9)
        let preview = model.preview(Array(model.ranges.prefix(2)))
        #expect(preview.ranges == [ClipRange(start: 30, end: 32), ClipRange(start: 4, end: 10)])
        #expect(preview.duration == 8)
        model.remove(model.ranges[1])
        #expect(model.ranges.map(\.start) == [30, 20])
        model.ranges[0].startMilliseconds = 30_500
        model.ranges[0].endMilliseconds = 31_250
        #expect(model.ranges[0].start == 30.5)
        #expect(model.ranges[0].endMilliseconds == 31_250)
        #expect(model.ranges[0].startMilliseconds == 30_500)
        model.open(URL(fileURLWithPath: "/v/a.mp4"))
        #expect(model.ranges.count == 2)
        model.open(URL(fileURLWithPath: "/v/b.mp4"))
        #expect(model.ranges.isEmpty)
    }

    @Test func exportNeedsRangesValidRangesShownCaptionsAndConsentForBurnedOnes() async throws {
        let burned = try CaptionPlanFixture([("\"burned\": false", "\"burned\": true")])
        defer { #expect(throws: Never.self) { try burned.remove() } }
        let (model, captions) = try await Self.model(burned, recorder: ExportRecorder())
        #expect(ExportModel().problem(captions: captions) == "動画が開かれていません")
        #expect(model.problem(captions: captions) == "書き出す範囲がありません")
        model.ranges = [ExportRange(start: 2, end: 1)]
        #expect(model.problem(captions: captions) == "開始が終了より前でない範囲があります")
        model.ranges = [ExportRange(start: 1, end: 2)]
        // Burned captions are off by default: the movie exports as it is.
        #expect(model.problem(captions: captions) == nil)
        captions.isShown = true
        #expect(model.problem(captions: captions)?.contains("二重焼き込みを許可") == true)
        model.allowsBurned = true
        #expect(model.problem(captions: captions) == nil)
    }

    @Test(.timeLimit(.minutes(1)))
    func anExportWritesTheShownCaptionsInOrderReportsProgressAndShowsTheResult() async throws {
        let fixture = try CaptionPlanFixture(try CaptionPlanFixture.realFont())
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        try await writeCaptionMovie(fixture.movie)
        let recorder = ExportRecorder()
        let workspace = TestWorkspace()
        let (model, captions) = try await Self.model(
            fixture, recorder: recorder, workspace: workspace)
        captions.usesGradient = false
        model.ranges = [ExportRange(start: 2, end: 3), ExportRange(start: 0, end: 1)]
        let output = fixture.folder.appending(path: "out.mp4")
        model.export(to: output, captions: captions)
        #expect(model.isExporting)
        #expect(model.problem(captions: captions) == "書き出し中です")
        model.open(URL(fileURLWithPath: "/v/other.mp4"))
        #expect(model.status == "書き出し中…")
        await model.task?.value
        #expect(!model.isExporting)
        #expect(model.status == "書き出しました: out.mp4")
        #expect(model.finished == output)
        #expect(workspace.revealed == [output])
        model.revealFinished()
        #expect(workspace.revealed.count == 2)
        let job = try #require(await recorder.jobs.first)
        #expect(job.ranges == [.init(start: 2, end: 3), .init(start: 0, end: 1)])
        #expect(job.plan == captions.placed)
        #expect(job.fill == CaptionFill(top: "#97D8F7", bottom: "#2FB0EE", gradient: false))
        #expect(job.nameplate != nil)
        #expect(job.speakerFills.isEmpty)
        #expect(!job.allowBurned)
        model.open(URL(fileURLWithPath: "/v/third.mp4"))
        #expect(model.status == nil)
        #expect(model.finished == nil)
        let idle = ExportModel()
        idle.workspace = workspace
        idle.revealFinished()
        #expect(workspace.revealed.count == 2)
    }

    @Test(.timeLimit(.minutes(1)))
    func exportsFailWithoutSpaceOrOnErrorsAndCanBeCancelled() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        try await writeCaptionMovie(fixture.movie)
        let output = fixture.folder.appending(path: "out.mp4")
        let full: @Sendable (URL) throws -> VolumeSpace = { _ in
            VolumeSpace(available: 31 << 30, total: 100 << 30, isStartup: true)
        }
        let (tight, captions) = try await Self.model(
            fixture, recorder: ExportRecorder(), space: full)
        tight.ranges = [ExportRange(start: 0, end: 4)]
        tight.export(to: output, captions: captions)
        await tight.task?.value
        // 31 GiB free still keeps the 30 GiB floor after this short export.
        #expect(tight.status == "書き出しました: out.mp4")
        let crowded: @Sendable (URL) throws -> VolumeSpace = { _ in
            VolumeSpace(available: 30 << 30, total: 100 << 30, isStartup: true)
        }
        let (refused, _) = try await Self.model(fixture, recorder: ExportRecorder(), space: crowded)
        refused.ranges = [ExportRange(start: 0, end: 4)]
        refused.export(to: output, captions: captions)
        await refused.task?.value
        #expect(refused.status?.hasPrefix("書き出せません: 保存先の空き容量が足りません") == true)
        let external: @Sendable (URL) throws -> VolumeSpace = { _ in
            VolumeSpace(available: 1 << 30, total: 2 << 40, isStartup: false)
        }
        let (failing, _) = try await Self.model(
            fixture, recorder: ExportRecorder(), space: external,
            exporter: { _, _ in throw CaptionRenderError.invalid("broken") })
        failing.ranges = [ExportRange(start: 0, end: 1)]
        failing.export(to: output, captions: captions)
        await failing.task?.value
        #expect(failing.status == "書き出せません: Invalid request: broken")
        let (cancelled, _) = try await Self.model(
            fixture, recorder: ExportRecorder(),
            exporter: { _, _ in try await Task.sleep(for: .seconds(30)) })
        cancelled.ranges = [ExportRange(start: 0, end: 1)]
        cancelled.export(to: output, captions: captions)
        cancelled.cancel()
        await cancelled.task?.value
        #expect(cancelled.status == "書き出しをキャンセルしました")
        #expect(!cancelled.isExporting)
        let (unreadable, _) = try await Self.model(fixture, recorder: ExportRecorder())
        unreadable.ranges = [ExportRange(start: 0, end: 1)]
        try FileManager.default.removeItem(at: fixture.movie)
        try Data().write(to: fixture.movie)
        unreadable.export(to: output, captions: captions)
        await unreadable.task?.value
        #expect(unreadable.status?.hasPrefix("書き出せません") == true)
    }

    @Test func spaceErrorsAndTheVolumeReadingAreReadable() throws {
        #expect(ExportSpaceError.unknownLength.errorDescription == "動画の長さか大きさを読めません")
        #expect(
            ExportSpaceError.insufficient(needed: 3 << 30, available: 1 << 29).errorDescription
                == "保存先の空き容量が足りません（必要 3.0 GiB・空き 0.5 GiB）")
        let space = try ExportModel.volumeSpace(
            try transcriptTestWorkRoot().deletingLastPathComponent())
        #expect(space.total > 0)
        #expect(space.available > 0)
    }

    @Test(.timeLimit(.minutes(1)))
    func theDefaultModelExportsThroughCaptionExportAndRevealsTheMovie() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        try await writeCaptionMovie(fixture.movie, seconds: 10)
        let captions = CaptionsModel()
        captions.open(fixture.movie)
        await captions.loading?.value
        let workspace = TestWorkspace()
        let model = ExportModel()
        model.workspace = workspace
        model.open(fixture.movie)
        model.ranges = [ExportRange(start: 6, end: 7), ExportRange(start: 2, end: 3)]
        let output = fixture.folder.appending(path: "default.mp4")
        model.export(to: output, captions: captions)
        await model.task?.value
        #expect(model.status == "書き出しました: default.mp4")
        #expect(workspace.revealed == [output])
        #expect(ExportSpaceError.unknownVolume.errorDescription == "保存先の空き容量を読めません")
        #expect(throws: CocoaError.self) {
            try ExportModel.volumeSpace(fixture.folder.appending(path: "missing"))
        }
    }

    @Test(.timeLimit(.minutes(1)))
    func theRealExportJoinsRangesInTheGivenOrderWithOrWithoutAPlan() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        try await writeCaptionMovie(fixture.movie, seconds: 10)
        let plan = try #require(try DeliveryCaptions.load(movie: fixture.movie)).plan
        let output = fixture.folder.appending(path: "joined.mp4")
        let job = ExportJob(
            movie: fixture.movie, plan: plan,
            ranges: [.init(start: 4, end: 5), .init(start: 1, end: 2)],
            fill: nil, speakerFills: [:], nameplate: nil,
            allowBurned: false, output: output)
        try await ExportModel.run(job) { _, _ in }
        let frames = try await AVURLAsset(url: output).load(.duration).seconds * 30
        #expect(abs(frames - 60) < 1)
        // Without a plan: no captions, the movie's own canvas.
        let bare = fixture.folder.appending(path: "bare.mp4")
        try await ExportModel.run(
            ExportJob(
                movie: fixture.movie, plan: nil, ranges: [.init(start: 3, end: 3.5)], fill: nil,
                speakerFills: [:], nameplate: nil, allowBurned: false, output: bare)
        ) { _, _ in }
        let bareFrames = try await AVURLAsset(url: bare).load(.duration).seconds * 30
        #expect(abs(bareFrames - 15) < 1)
    }
}

@MainActor
struct ExportPanelTests {
    @Test func panelMarksPreviewsRemovesAndExportsThroughTheSavePanel() async throws {
        let fixture = try CaptionPlanFixture([("\"burned\": false", "\"burned\": true")])
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let captions = CaptionsModel()
        let recorder = ExportRecorder()
        let export = ExportModel(
            exporter: { job, _ in await recorder.record(job) }, space: ExportModelTests.roomy)
        export.workspace = TestWorkspace()
        let session = PlaybackSession(captions: captions, export: export)
        defer {
            session.player.pause()
            session.stopObserving()
        }
        captions.open(fixture.movie)
        await captions.loading?.value
        export.open(fixture.movie)
        let panel = TestSavePanel()
        let view = ExportPanel(session: session, savePanel: { panel })
        session.currentSeconds = 3
        try view.inspect().find(button: "開始をマーク").tap()
        session.currentSeconds = 5
        try view.inspect().find(button: "終了をマーク").tap()
        #expect(export.ranges.map(\.start) == [3])
        try view.inspect().find(button: "開始をマーク").tap()
        #expect(
            try view.inspect().find(
                ViewType.Text.self, where: { try $0.string() == "開始 00:05.000" }
            )
            .string() == "開始 00:05.000")
        let clip = ClipPoint(
            id: "c", kind: "5-10", duration: 1, ranges: [ClipRange(start: 7, end: 8)], title: "山場",
            summary: "")
        try view.clipItems([clip]).inspect().find(button: "山場").tap()
        #expect(export.ranges.count == 2)
        let field = try view.row(
            Binding(get: { export.ranges[0] }, set: { export.ranges[0] = $0 }), number: 1
        )
        .inspect()
        try field.find(ViewType.TextField.self).setInput("3500")
        #expect(export.ranges[0].start == 3.5)
        try field.find(viewWithAccessibilityLabel: "範囲1を再生").button().tap()
        try view.inspect().find(button: "順に再生").tap()
        try field.find(viewWithAccessibilityLabel: "範囲1を削除").button().tap()
        #expect(export.ranges.map(\.start) == [7])
        // Burned captions are off by default: exportable as the movie is.
        #expect(try !view.inspect().find(button: "書き出し…").isDisabled())
        captions.isShown = true
        #expect(try view.inspect().find(button: "書き出し…").isDisabled())
        try view.inspect().find(ViewType.Toggle.self).tap()
        #expect(export.allowsBurned)
        try view.inspect().find(button: "書き出し…").tap()
        #expect(!export.isExporting)
        panel.response = .OK
        panel.chosenURL = fixture.folder.appending(path: "out.mp4")
        try view.inspect().find(button: "書き出し…").tap()
        #expect(export.isExporting)
        #expect(panel.nameFieldStringValue == "synthetic-review-export.mp4")
        try view.inspect().find(button: "キャンセル").tap()
        await export.task?.value
        #expect(export.status != nil)
        #expect(throws: Never.self) { try view.inspect().find(text: export.status ?? "") }
    }

    @Test func aFinishedExportOffersFinderAndProblemsShowWithoutAStatus() async throws {
        let fixture = try CaptionPlanFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        try await writeCaptionMovie(fixture.movie)
        let workspace = TestWorkspace()
        let export = ExportModel(exporter: { _, _ in }, space: ExportModelTests.roomy)
        export.workspace = workspace
        let session = PlaybackSession(export: export)
        defer {
            session.player.pause()
            session.stopObserving()
        }
        session.captions.open(fixture.movie)
        await session.captions.loading?.value
        export.open(fixture.movie)
        export.ranges = [ExportRange(start: 2, end: 1)]
        let view = ExportPanel(session: session)
        #expect(throws: Never.self) {
            try view.inspect().find(text: "開始が終了より前でない範囲があります")
        }
        export.ranges = [ExportRange(start: 0, end: 1)]
        export.export(to: fixture.folder.appending(path: "o.mp4"), captions: session.captions)
        await export.task?.value
        try view.inspect().find(button: "Finderで表示").tap()
        #expect(workspace.revealed.count == 2)
        _ = ExportPanel(session: session).savePanel()
        export.ranges = [ExportRange(start: 0, end: 1), ExportRange(start: 2, end: 3)]
        try view.inspect().find(ViewType.ForEach.self).callOnMove(IndexSet(integer: 1), 0)
        #expect(export.ranges.map(\.start) == [2, 0])
    }
}

@MainActor
struct TimelineSelectionTests {
    @Test(arguments: [
        (10.0, 30.0, 100.0, 0.1...0.3), (30.0, 10.0, 100.0, 0.1...0.3),
        (-20.0, 150.0, 100.0, 0.0...1.0),
    ])
    func aDragSpansTheFractionsBetweenItsEnds(
        from: Double, to: Double, width: Double, span: ClosedRange<Double>
    ) {
        #expect(TimelineBar.span(from, to, width: width) == span)
    }

    @Test func aBarWithoutWidthSpansNothing() {
        #expect(TimelineBar.span(1, 2, width: 0) == nil)
    }

    @Test func releasingADragSelectsItsSecondsAndAnEmptyOneNothing() throws {
        var selected: [ClosedRange<Double>] = []
        let bar = TimelineBar(
            utterances: [], duration: 200, currentSeconds: 0, selectedID: nil
        ) { start, end in selected.append(start...end) }
        bar.drag(from: 50, to: 10, width: 100, ended: false)
        bar.drag(from: 50, to: 10, width: 100, ended: true)
        bar.drag(from: 40, to: 40, width: 100, ended: true)
        bar.drag(from: 40, to: 60, width: 0, ended: true)
        #expect(selected == [20...100])
        let gesture = try bar.inspect().find(ViewType.ZStack.self).gesture(DragGesture.self)
        let value = DragGesture.Value(
            time: Date(), location: CGPoint(x: 5, y: 1), startLocation: CGPoint(x: 1, y: 1),
            velocity: .zero)
        try gesture.callOnChanged(value: value)
        try gesture.callOnEnded(value: value)
    }

    @Test func draggingOnTheRootTimelineAddsAnExportRangeAndShowsIt() throws {
        let fixture = try SummaryFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let session = PlaybackSession()
        defer {
            session.player.pause()
            session.stopObserving()
        }
        session.videos = [fixture.video]
        session.selectedVideoID = fixture.video.id
        let bar = try TranscriptRootView(session: session).inspect().find(TimelineBar.self)
            .actualView()
        bar.select(3, 4.5)
        #expect(session.export.ranges.map(\.start) == [3])
        #expect(session.export.ranges.map(\.end) == [4.5])
        #expect(session.detailTab == .export)
    }
}
