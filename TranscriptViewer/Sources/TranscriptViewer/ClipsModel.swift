import Foundation
import Observation
import TranscriptCore

/// The selected video's clip points, read from the web viewer's Worker (where the Mac stores them
/// per movie content), and the clip being played range by range.
@MainActor @Observable
final class ClipsModel {
    enum State: Equatable {
        case idle
        case loading
        case ready([ClipPoint])
        case pending
        case mismatch
        case missing
        case noExcitement
        /// No API configuration in this checkout.
        case unavailable
        case failed(String)
    }

    typealias Fetch = @Sendable (URLRequest) async throws -> (Data, Int)
    typealias MovieKey = @Sendable (URL) throws -> String
    typealias Recompute = @Sendable (String) async -> (status: Int32, output: String)

    static let pollInterval: Duration = .seconds(15)
    /// How often a video without clips for its movie asks again: the Mac makes a published
    /// video's clips after the upload (an hour or so), and a republished movie replaces the
    /// record, so a video opened before then showed none until it was selected again (10-30).
    static let settledInterval: Duration = .seconds(60)

    private(set) var state = State.idle
    private(set) var cursor: ClipCursor?
    let api: ClipsAPI?
    @ObservationIgnored var fetch: Fetch = ClipsModel.download
    @ObservationIgnored var movieKey: MovieKey = BGMLevel.movieKey
    @ObservationIgnored var recompute: Recompute = {
        await ClipsModel.recomputeOnMac($0, checkout: BGMLibraryModel.checkout)
    }
    @ObservationIgnored var pollInterval = ClipsModel.pollInterval
    @ObservationIgnored var settledInterval = ClipsModel.settledInterval
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var movie: URL?
    @ObservationIgnored private var key: String?

    /// The API of this dist app's checkout, when it has one.
    static var configured: ClipsAPI? { BGMLibraryModel.checkout.flatMap(ClipsAPI.load(checkout:)) }

    init(api: ClipsAPI?) {
        self.api = api
    }

    /// The HTTP status, or 0 for a response that is not HTTP.
    nonisolated static func download(_ request: URLRequest) async throws -> (Data, Int) {
        let (data, response) = try await URLSession.shared.data(for: request)
        guard let http = response as? HTTPURLResponse else { return (data, 0) }
        return (data, http.statusCode)
    }

    var clips: [ClipPoint] {
        if case .ready(let clips) = state { return clips }
        return []
    }

    var canReload: Bool { api != nil && movie != nil && state != .pending && state != .loading }

    /// Loads the clips of `movie`, polling while they are being computed, and less often while
    /// the Worker has none for it yet (missing, another movie's or no excitement yet).
    func open(_ movie: URL?) {
        task?.cancel()
        cursor = nil
        self.movie = movie
        key = nil
        guard let movie else {
            state = .idle
            return
        }
        guard api != nil else {
            state = .unavailable
            return
        }
        state = .loading
        task = Task { [weak self] in await self?.poll(movie) }
    }

    /// Computes the clips again on the Mac (the Worker runs no model), then reads the new ones.
    func reload() {
        guard api != nil, let movie else { return }
        task?.cancel()
        cursor = nil
        state = .pending
        task = Task { [weak self] in
            guard let self else { return }
            let result = await recompute(ClipsAPI.videoID(movie))
            guard result.status == 0 else {
                state = .failed("Macでの再計算に失敗しました（終了コード \(result.status)）")
                return
            }
            await poll(movie)
        }
    }

    /// `bun run clips:mac -- --force <id>` in the checkout's web viewer: the local models, about
    /// an hour per video. No shell; PATH reaches bun and uv.
    nonisolated static func recomputeOnMac(_ videoID: String, checkout: URL?) async
        -> (status: Int32, output: String)
    {
        guard let checkout else { return (-1, "checkout not found") }
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        return await BGMLibraryModel.run(
            URL(fileURLWithPath: "/usr/bin/env"),
            ["bun", "run", "clips:mac", "--", "--force", videoID],
            in: checkout.appending(path: "web-viewer", directoryHint: .isDirectory),
            environment: [
                "HOME": home, "PATH": "\(home)/.bun/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin",
            ])
    }

    private func poll(_ movie: URL) async {
        guard let api else { return }
        do {
            let key = try await keyOf(movie)
            while !Task.isCancelled, self.movie == movie {
                let (data, status) = try await fetch(
                    api.lookup(videoID: ClipsAPI.videoID(movie), movieKey: key))
                guard status == 200 || status == 404 else {
                    state = .failed("切り抜きを読み込めませんでした（HTTP \(status)）")
                    return
                }
                switch try ClipStatus.decode(data) {
                case .ready(let clips):
                    state = .ready(clips)
                    return
                case .mismatch: state = .mismatch
                case .missing: state = .missing
                case .noExcitement: state = .noExcitement
                case .pending: state = .pending
                }
                try await Task.sleep(for: state == .pending ? pollInterval : settledInterval)
            }
        } catch is CancellationError {
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    /// The content key reads 2 MiB of the movie, so it runs off the main actor once per video.
    private func keyOf(_ movie: URL) async throws -> String {
        if let key { return key }
        let read = movieKey
        let key = try await Task.detached { try read(movie) }.value
        self.key = key
        return key
    }

    /// Starts `clip` at range `index`; returns where to seek.
    func start(_ clip: ClipPoint, range index: Int = 0, loop: Bool = true) -> Double? {
        let cursor = ClipCursor(clip: clip, index: index, loop: loop)
        self.cursor = cursor
        return cursor.range?.start
    }

    func stop() { cursor = nil }

    /// Advances the playing clip; the session seeks or pauses as told.
    func step(at seconds: Double) -> ClipCursor.Step {
        guard var cursor else { return .keep }
        let step = cursor.step(at: seconds)
        self.cursor = step == .finish || step == .leave ? nil : cursor
        return step
    }
}
