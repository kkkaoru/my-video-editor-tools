import AVFoundation
import AppKit
import CaptionRender
import Foundation
import Observation
import TranscriptCore

/// One range of the open movie to export, in seconds; ranges are joined in list order.
struct ExportRange: Identifiable, Equatable {
    let id: UUID
    var start: Double
    var end: Double

    init(start: Double, end: Double) {
        id = UUID()
        self.start = start
        self.end = end
    }

    var duration: Double { end - start }
    /// Whole milliseconds, for the numeric fields.
    var startMilliseconds: Int {
        get { Int((start * 1000).rounded()) }
        set { start = Double(newValue) / 1000 }
    }
    var endMilliseconds: Int {
        get { Int((end * 1000).rounded()) }
        set { end = Double(newValue) / 1000 }
    }
}

/// What one export writes: the movie's ranges joined in order, with the captions as the viewer
/// shows them (`plan` nil: none).
struct ExportJob: Sendable {
    let movie: URL
    let plan: CaptionPlan?
    let ranges: [CaptionExportRange]
    let fill: CaptionFill?
    let speakerFills: [String: CaptionFill]
    let nameplate: MotionCaptionBitmap?
    let allowBurned: Bool
    let output: URL
}

/// Free and total bytes of the volume holding a folder, and whether it is the startup volume.
struct VolumeSpace: Equatable, Sendable {
    let available: Int64
    let total: Int64
    let isStartup: Bool
}

/// The ranges chosen for export of the open movie, and the export itself: estimated against
/// the destination's free space, reported as it runs, cancellable, and shown in Finder after.
@MainActor @Observable
final class ExportModel {
    typealias Exporter =
        @Sendable (ExportJob, @escaping @Sendable (Int, Int) -> Void) async throws -> Void
    /// The startup volume keeps 30 GiB or 5 % of its size free, whichever is larger.
    nonisolated static let startupFloor: Int64 = 30 << 30
    nonisolated static let startupFloorFraction = 0.05
    /// Output size per source byte of the same length, with headroom for re-encoding.
    nonisolated static let sizeMargin = 1.25

    var ranges: [ExportRange] = []
    /// Where the next range starts, once marked.
    var pendingStart: Double?
    /// Burns the captions again into a movie that already has them (chosen explicitly).
    var allowsBurned = false
    private(set) var movie: URL?
    private(set) var progress: Double?
    private(set) var status: String?
    private(set) var finished: URL?
    @ObservationIgnored private(set) var task: Task<Void, Never>?
    @ObservationIgnored private let exporter: Exporter
    @ObservationIgnored private let space: @Sendable (URL) throws -> VolumeSpace
    /// Shows a finished export in Finder.
    @ObservationIgnored var workspace = NSWorkspace.shared

    init(
        exporter: @escaping Exporter = ExportModel.run,
        space: @escaping @Sendable (URL) throws -> VolumeSpace = ExportModel.volumeSpace
    ) {
        self.exporter = exporter
        self.space = space
    }

    var isExporting: Bool { progress != nil }
    var duration: Double { ranges.reduce(0) { $0 + $1.duration } }

    /// Another movie starts with no ranges; a running export carries on.
    func open(_ movie: URL?) {
        guard movie != self.movie else { return }
        self.movie = movie
        ranges = []
        pendingStart = nil
        allowsBurned = false
        if !isExporting {
            status = nil
            finished = nil
        }
    }

    func markStart(at seconds: Double) { pendingStart = max(0, seconds) }

    /// Adds the range from the marked start to `seconds` (either way round).
    func markEnd(at seconds: Double) {
        guard let start = pendingStart, seconds != start else { return }
        ranges.append(ExportRange(start: min(start, seconds), end: max(start, seconds)))
        pendingStart = nil
    }

    func add(_ clip: ClipPoint) {
        ranges += clip.ranges.map { ExportRange(start: $0.start, end: $0.end) }
    }

    func move(from source: IndexSet, to destination: Int) {
        ranges.move(fromOffsets: source, toOffset: destination)
    }

    func remove(_ range: ExportRange) { ranges.removeAll { $0.id == range.id } }

    /// The ranges as a clip, to preview them with the clips' playback.
    func preview(_ chosen: [ExportRange]) -> ClipPoint {
        ClipPoint(
            id: "export", kind: "export", duration: chosen.reduce(0) { $0 + $1.duration },
            ranges: chosen.map { ClipRange(start: $0.start, end: $0.end) }, title: "書き出し範囲",
            summary: "")
    }

    /// Why the ranges cannot be exported with these captions now; nil when they can.
    func problem(captions: CaptionsModel) -> String? {
        if isExporting { return "書き出し中です" }
        if movie == nil { return "動画が開かれていません" }
        if ranges.isEmpty { return "書き出す範囲がありません" }
        if ranges.contains(where: { !($0.start >= 0 && $0.start < $0.end) }) {
            return "開始が終了より前でない範囲があります"
        }
        if captions.isShown, captions.captions?.plan.burned == true, !allowsBurned {
            return "この動画には字幕が焼き込み済みです。二重に書き込むなら「二重焼き込みを許可」をオンにしてください"
        }
        return nil
    }

    /// Exports the ranges to `output` with the captions as `captions` shows them.
    func export(to output: URL, captions: CaptionsModel) {
        guard problem(captions: captions) == nil, let movie else { return }
        // The plan as the player shows it: placed, without hidden speakers.
        let shown = captions.isShown ? captions.placed : nil
        let job = ExportJob(
            movie: movie, plan: shown,
            ranges: ranges.map { CaptionExportRange(start: $0.start, end: $0.end) },
            fill: captions.fill, speakerFills: captions.speakerFills,
            nameplate: shown == nil ? nil : captions.underlay?.bitmap,
            allowBurned: allowsBurned, output: output)
        let seconds = duration
        progress = 0
        status = "書き出し中…"
        finished = nil
        task = Task { [exporter, space] in
            do {
                try await Self.checkSpace(job, seconds: seconds, space: space)
                let (fractions, continuation) = AsyncStream.makeStream(
                    of: Double.self, bufferingPolicy: .bufferingNewest(1))
                async let done: Void = Self.write(job, exporter, continuation)
                for await fraction in fractions { progress = fraction }
                try await done
                finished = output
                status = "書き出しました: \(output.lastPathComponent)"
                workspace.activateFileViewerSelecting([output])
            } catch is CancellationError {
                status = "書き出しをキャンセルしました"
            } catch {
                status = "書き出せません: \(Self.describe(error))"
            }
            progress = nil
        }
    }

    func cancel() { task?.cancel() }

    /// Our errors' Japanese text; CaptionRender's own description otherwise.
    nonisolated static func describe(_ error: any Error) -> String {
        (error as? LocalizedError)?.errorDescription ?? String(describing: error)
    }

    func revealFinished() {
        if let finished { workspace.activateFileViewerSelecting([finished]) }
    }

    nonisolated static func write(
        _ job: ExportJob, _ exporter: Exporter, _ progress: AsyncStream<Double>.Continuation
    ) async throws {
        defer { progress.finish() }
        try await exporter(job) { done, total in
            progress.yield(total > 0 ? Double(done) / Double(total) : 0)
        }
    }

    /// Refuses an export whose estimated size (the source's bytes per second over the
    /// exported length, with margin) would leave the destination below its floor.
    nonisolated static func checkSpace(
        _ job: ExportJob, seconds: Double, space: @Sendable (URL) throws -> VolumeSpace
    ) async throws {
        let length = try await AVURLAsset(url: job.movie).load(.duration).seconds
        guard let source = try job.movie.resourceValues(forKeys: [.fileSizeKey]).fileSize,
            length.isFinite, length > 0
        else { throw ExportSpaceError.unknownLength }
        let estimate = Int64(Double(source) / length * seconds * sizeMargin)
        let volume = try space(job.output.deletingLastPathComponent())
        let floor =
            volume.isStartup
            ? max(startupFloor, Int64(Double(volume.total) * startupFloorFraction)) : 0
        guard volume.available - estimate >= floor else {
            throw ExportSpaceError.insufficient(
                needed: estimate + floor, available: volume.available)
        }
    }

    nonisolated static func volumeSpace(_ folder: URL) throws -> VolumeSpace {
        let values = try folder.resourceValues(forKeys: [
            .volumeAvailableCapacityForImportantUsageKey, .volumeTotalCapacityKey,
            .volumeIsRootFileSystemKey,
        ])
        guard let available = values.volumeAvailableCapacityForImportantUsage,
            let total = values.volumeTotalCapacity, let isStartup = values.volumeIsRootFileSystem
        else { throw ExportSpaceError.unknownVolume }
        return VolumeSpace(available: available, total: Int64(total), isStartup: isStartup)
    }

    /// The real export: `CaptionExport` (no plan: without captions), which never overwrites
    /// `output`.
    nonisolated static func run(
        _ job: ExportJob, progress: @escaping @Sendable (Int, Int) -> Void
    ) async throws {
        _ = try await CaptionExport.export(
            movie: job.movie, plan: job.plan, ranges: job.ranges, fill: job.fill,
            speakerFills: job.speakerFills,
            nameplate: job.nameplate, output: job.output, allowBurned: job.allowBurned,
            progress: progress)
    }
}

enum ExportSpaceError: Error, LocalizedError, Equatable {
    case unknownLength
    case unknownVolume
    case insufficient(needed: Int64, available: Int64)

    var errorDescription: String? {
        switch self {
        case .unknownLength: "動画の長さか大きさを読めません"
        case .unknownVolume: "保存先の空き容量を読めません"
        case .insufficient(let needed, let available):
            "保存先の空き容量が足りません（必要 \(Self.gib(needed))・空き \(Self.gib(available))）"
        }
    }

    static func gib(_ bytes: Int64) -> String {
        String(format: "%.1f GiB", Double(bytes) / Double(1 << 30))
    }
}
