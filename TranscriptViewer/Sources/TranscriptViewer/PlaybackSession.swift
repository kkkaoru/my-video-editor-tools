import AVFoundation
import AppKit
import Foundation
import Observation
import SwiftUI
import TranscriptCore

/// What fills the detail's lower half under the always-visible player.
enum DetailTab: Hashable {
    case summary
    case utterances
    case clips
    case export
}

@MainActor
@Observable
final class PlaybackSession {
    typealias CatalogLoader =
        @MainActor (
            URL, @escaping @MainActor @Sendable (Result<CatalogScan, Error>) -> Void
        ) -> Void
    typealias ManifestLoader =
        @MainActor (
            URL, @escaping @MainActor @Sendable (Result<[URL], Error>) -> Void
        ) -> Void
    static let catalogRefreshInterval: Duration = .seconds(30)
    static let positionsKey = "TranscriptViewer.playbackPositions"
    /// Playback saves its position at most this often (and on pause, seek, switch and quit).
    static let positionSaveInterval = 5.0
    /// The readout's own observer: periodic observers fire only while time moves, so a paused
    /// player costs nothing, and only the readout reads `displaySeconds`.
    static let clockInterval = CMTime(value: 1, timescale: 30)

    var videos: [VideoTranscript] = []
    var selectedVideoID: URL?
    private(set) var utterances: [TranscriptCue] = []
    var selectedUtteranceID: Int?
    var browseAnchorID: Int?
    var timelineFollowsPlayback = true
    var currentSeconds = 0.0
    /// The playhead for the millisecond readout, refreshed every `clockInterval` while playing.
    private(set) var displaySeconds = 0.0
    var searchQuery = ""
    var searchScope = SearchScope.currentVideo
    var searchHits: [SearchHit] = []
    var isSearching = false
    private(set) var catalogRevision = 0
    private(set) var usesDefaultLibraries = false
    var showsSearch = false
    var columnVisibility: NavigationSplitViewVisibility = .all
    var detailTab = DetailTab.summary
    var summaryBatchRun: UUID?
    var errorText = ""
    var showsError = false
    var scanIssues: [String] = []
    let player = AVPlayer()
    /// Plays the opened movie again from its start each time it ends; off for a new movie.
    var loopsVideo = false {
        didSet {
            guard loopsVideo != oldValue else { return }
            applyLoop()
        }
    }
    let narrativeSummary: NarrativeSummaryModel
    let videoBGM: VideoBGMModel
    let clips: ClipsModel
    let channels: ChannelsModel
    let captions: CaptionsModel
    let export: ExportModel
    var playbackSpeed = PlaybackSpeed.normal {
        didSet {
            player.defaultRate = playbackSpeed.rawValue
            if player.rate != 0 {
                player.rate = playbackSpeed.rawValue
            }
        }
    }

    private var libraryRoot: URL?
    private var scanningRoot: URL?
    private var scanGeneration = 0
    private var openedVideoID: URL?
    /// Last position per movie (`PlaybackResume.key`); persisted only with `positionDefaults`.
    private(set) var positions: [String: Double]
    private let positionDefaults: UserDefaults?
    /// The opened movie whose saved position is still being applied: its player time is not
    /// its position yet, so nothing is recorded for it meanwhile.
    private var pendingResume: URL?
    private var lastSavedSeconds = 0.0
    /// Volumes (standardized paths) that went away while holding listed videos.
    private(set) var missingVolumes: Set<String> = []
    private let isMountPoint: (URL) -> Bool
    @ObservationIgnored private var observers: [NSObjectProtocol] = []
    @ObservationIgnored private var pauseObservation: NSKeyValueObservation?
    @ObservationIgnored private var loopObserver: NSObjectProtocol?

    var libraryName: String { libraryRoot?.lastPathComponent ?? "未選択" }
    var isScanning: Bool { scanningRoot != nil }

    private var timeObserver: Any?
    private var clockObserver: Any?
    /// Captions are burned into the video; an embedded subtitle track would show twice.
    private(set) var legibleSelection: Task<Void, Never>?
    private var utterancePanel: NSWindow?
    private let readLibraryRoot: () -> URL?
    private let saveLibraryRoot: (URL) -> Void
    private let defaultLibrary: URL
    private let defaultAdditionalLibraries: [URL]
    private let defaultManifest: URL
    private let readManifest: ManifestLoader
    /// Explicit additions and configured ordinary-startup libraries are kept separate.
    private var additionalRoots: [URL] = []
    private let presentWindow: (NSWindow) -> Void
    private let scanCatalog: CatalogLoader
    private let launchArguments: [String]

    init(
        readLibraryRoot: @escaping () -> URL? = {
            UserDefaults.standard.url(forKey: "TranscriptViewer.libraryRoot")
        },
        saveLibraryRoot: @escaping (URL) -> Void = {
            UserDefaults.standard.set($0, forKey: "TranscriptViewer.libraryRoot")
        },
        defaultLibrary: URL? = nil,
        defaultAdditionalLibraries: [URL]? = nil,
        defaultManifest: URL? = nil,
        readManifest: @escaping ManifestLoader = DefaultLibraryManifest.load(at:completion:),
        presentWindow: @escaping (NSWindow) -> Void = { $0.makeKeyAndOrderFront(nil) },
        scanCatalog: @escaping CatalogLoader = TranscriptCatalog.scan(in:completion:),
        narrativeSummary: NarrativeSummaryModel = NarrativeSummaryModel(),
        videoBGM: VideoBGMModel? = nil,
        clips: ClipsModel = ClipsModel(api: nil),
        channels: ChannelsModel = ChannelsModel(api: nil, originals: nil),
        captions: CaptionsModel = CaptionsModel(),
        export: ExportModel = ExportModel(),
        launchArguments: [String] = [],
        positionDefaults: UserDefaults? = nil,
        workspace: NotificationCenter = NSWorkspace.shared.notificationCenter,
        application: NotificationCenter = .default,
        isMountPoint: @escaping (URL) -> Bool = PlaybackSession.isMountPoint
    ) {
        self.readLibraryRoot = readLibraryRoot
        self.saveLibraryRoot = saveLibraryRoot
        self.defaultLibrary = defaultLibrary ?? Self.defaultRoot
        self.defaultAdditionalLibraries = defaultAdditionalLibraries ?? []
        self.defaultManifest =
            defaultManifest
            ?? (defaultLibrary ?? Self.defaultRoot).appending(path: DefaultLibraryManifest.filename)
        self.readManifest = readManifest
        self.presentWindow = presentWindow
        self.scanCatalog = scanCatalog
        self.narrativeSummary = narrativeSummary
        // The app passes the saved library; a bare session reads no BGM settings.
        self.videoBGM =
            videoBGM
            ?? VideoBGMModel(
                library: BGMLibraryModel(root: nil, volume: 1, defaultID: nil), settings: [:])
        self.clips = clips
        self.channels = channels
        self.captions = captions
        self.export = export
        self.launchArguments = launchArguments
        self.positionDefaults = positionDefaults
        positions = positionDefaults.map(Self.savedPositions) ?? [:]
        self.isMountPoint = isMountPoint
        self.videoBGM.follow(player)
        observe(workspace: workspace, application: application)
    }

    private func observe(workspace: NotificationCenter, application: NotificationCenter) {
        observers = [
            workspace.addObserver(
                forName: NSWorkspace.didUnmountNotification, object: nil, queue: .main
            ) { [weak self] note in
                let url = Self.volume(note)
                MainActor.assumeIsolated { self?.volumeUnmounted(url) }
            },
            workspace.addObserver(
                forName: NSWorkspace.didMountNotification, object: nil, queue: .main
            ) { [weak self] note in
                let url = Self.volume(note)
                MainActor.assumeIsolated { self?.volumeMounted(url) }
            },
            application.addObserver(
                forName: NSApplication.willTerminateNotification, object: nil, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.recordPosition() }
            },
        ]
        pauseObservation = player.observe(\.timeControlStatus, options: [.new]) {
            [weak self] observed, _ in
            guard observed.timeControlStatus == .paused else { return }
            Task { @MainActor in self?.recordPosition() }
        }
    }

    nonisolated static func volume(_ note: Notification) -> URL? {
        note.userInfo?[NSWorkspace.volumeURLUserInfoKey] as? URL
    }

    static func savedPositions(_ defaults: UserDefaults) -> [String: Double] {
        defaults.dictionary(forKey: positionsKey) as? [String: Double] ?? [:]
    }

    /// A mount point reports itself as a volume; a plain folder at that path does not.
    nonisolated static func isMountPoint(_ url: URL) -> Bool {
        do {
            return try url.resourceValues(forKeys: [.isVolumeKey]).isVolume == true
        } catch {
            return false
        }
    }

    /// The opened movie's position, saved for the next time it opens. Skipped while its saved
    /// position is still being applied or once its item is gone, so 0 never overwrites it.
    func recordPosition(at seconds: Double? = nil) {
        guard let openedVideoID, pendingResume != openedVideoID,
            let asset = player.currentItem?.asset as? AVURLAsset, asset.url == openedVideoID
        else { return }
        let position = seconds ?? player.currentTime().seconds
        guard position.isFinite else { return }
        positions[PlaybackResume.key(for: openedVideoID)] = position
        lastSavedSeconds = position
        positionDefaults?.set(positions, forKey: Self.positionsKey)
    }

    /// Periodic saving: only once playback moved `positionSaveInterval` from the last save.
    func recordPositionIfDue(_ seconds: Double) {
        if abs(seconds - lastSavedSeconds) >= Self.positionSaveInterval {
            recordPosition(at: seconds)
        }
    }

    private static func prefix(_ volume: URL) -> String {
        let path = volume.standardizedFileURL.path
        return path.hasSuffix("/") ? path : "\(path)/"
    }

    func isUnavailable(_ video: VideoTranscript) -> Bool {
        missingVolumes.contains { video.movieURL.standardizedFileURL.path.hasPrefix($0) }
    }

    var volumeNotice: String? {
        missingVolumes.isEmpty
            ? nil
            : "SSD 未接続: \(missingVolumes.sorted().map { URL(fileURLWithPath: $0).lastPathComponent }.joined(separator: "、"))。接続し直すと自動で再読み込みします。"
    }

    /// A volume holding listed videos went away: keep the list and selection, save the open
    /// movie's position and release it; library refreshes wait until the volume returns.
    func volumeUnmounted(_ volume: URL?) {
        guard let volume else { return }
        let prefix = Self.prefix(volume)
        guard videos.contains(where: { $0.movieURL.standardizedFileURL.path.hasPrefix(prefix) })
        else { return }
        missingVolumes.insert(prefix)
        guard let openedVideoID, openedVideoID.standardizedFileURL.path.hasPrefix(prefix) else {
            return
        }
        recordPosition()
        stopObserving()
        loopsVideo = false
        player.pause()
        player.replaceCurrentItem(with: nil)
        captions.open(nil)
        self.openedVideoID = nil
    }

    /// The volume is back as a real mount: rescan, keep the selection and reopen it paused at
    /// its saved position.
    func volumeMounted(_ volume: URL?) {
        guard let volume, missingVolumes.contains(Self.prefix(volume)), isMountPoint(volume)
        else { return }
        missingVolumes.remove(Self.prefix(volume))
        refreshLibrary()
    }

    var selectedVideo: VideoTranscript? {
        videos.first { $0.id == selectedVideoID }
    }

    var duration: Double {
        if let item = player.currentItem {
            let seconds = item.duration.seconds
            if seconds.isFinite, seconds > 0 {
                return seconds
            }
        }
        return CueTimeline.duration(of: utterances)
    }

    var sliderUpperBound: Double {
        max(duration, 0.001)
    }

    var currentUtterance: TranscriptCue? {
        utterances.first { $0.id == selectedUtteranceID }
    }

    var nearbyUtterances: [TranscriptCue] {
        let anchor = TimelineFollow.anchorID(
            followsPlayback: timelineFollowsPlayback,
            playbackID: selectedUtteranceID,
            browseID: browseAnchorID
        )
        let index = utterances.firstIndex { $0.id == anchor }
        return CueTimeline.window(around: index, in: utterances, radius: 24)
    }

    func loadDefaultLibrary() {
        do {
            let launchRoot = try LaunchLibrary.resolve(arguments: launchArguments)
            // A directory Open Document event can arrive before this view's task.
            guard libraryRoot == nil else { return }
            if let launchRoot {
                load(root: launchRoot)
            } else {
                load(
                    root: readLibraryRoot() ?? defaultLibrary,
                    additional: defaultAdditionalLibraries, includeConfiguredLibraries: true)
            }
        } catch {
            errorText = error.localizedDescription
            showsError = true
        }
    }

    /// Open an explicitly supplied library without changing the saved preference.
    func openLibrary(url: URL) {
        do {
            load(root: try LaunchLibrary.resolve(url: url))
        } catch {
            errorText = error.localizedDescription
            showsError = true
        }
    }

    static var defaultRoot: URL {
        if let edits = TranscriptCatalog.checkoutLibrary(near: Bundle.main.bundleURL) {
            return edits
        }
        return FileManager.default.homeDirectoryForCurrentUser
            .appending(path: "Movies", directoryHint: .isDirectory)
            .appending(path: "Apple-Pro-Apps-Verification", directoryHint: .isDirectory)
    }

    func chooseFolder(panel: NSOpenPanel = NSOpenPanel()) {
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.message = "動画とCSV/SRT字幕があるフォルダを選んでください"
        guard panel.runModal() == .OK, let url = panel.url else {
            return
        }
        load(root: url)
        saveLibraryRoot(url)
    }

    func load(root: URL, additional: [URL] = []) {
        load(root: root, additional: additional, includeConfiguredLibraries: false)
    }

    private func load(root: URL, additional: [URL], includeConfiguredLibraries: Bool) {
        libraryRoot = root
        usesDefaultLibraries = includeConfiguredLibraries
        additionalRoots = additional.filter { $0.standardizedFileURL != root.standardizedFileURL }
        rescan(root: root, keepSelection: false)
    }

    /// Adds deliveries that appeared after the last scan. Keeps playing the
    /// current video when it is still present.
    func refreshLibrary() {
        // A missing volume's libraries would drop their videos and the selection.
        guard let libraryRoot, scanningRoot != libraryRoot, missingVolumes.isEmpty else {
            return
        }
        rescan(root: libraryRoot, keepSelection: true)
    }

    /// Each window's lifecycle cancels its observer. The shared session coalesces
    /// concurrent ticks while a scan is running, so closing one window cannot stop others.
    func observeDefaultLibraryUpdates(
        waitForUpdate: () async throws -> Void = {
            try await Task.sleep(for: catalogRefreshInterval)
        }
    ) async {
        guard usesDefaultLibraries else { return }
        do {
            while usesDefaultLibraries {
                try await waitForUpdate()
                try Task.checkCancellation()
                guard usesDefaultLibraries else { return }
                refreshLibrary()
            }
        } catch is CancellationError {
            return
        } catch {
            errorText = error.localizedDescription
            showsError = true
        }
    }

    private func rescan(root: URL, keepSelection: Bool) {
        scanGeneration += 1
        let generation = scanGeneration
        scanningRoot = root
        if usesDefaultLibraries {
            readManifest(defaultManifest) { [weak self] result in
                guard let self, self.scanGeneration == generation, self.libraryRoot == root else {
                    return
                }
                var initial = CatalogScan()
                let configured: [URL]
                switch result {
                case .success(let libraries): configured = libraries
                case .failure(let error):
                    configured = []
                    initial.unreadableTranscriptLabels.append(
                        "\(DefaultLibraryManifest.filename) (\(error.localizedDescription))")
                }
                self.scanLibraries(
                    root: root, additions: self.additionalRoots + configured, initial: initial,
                    generation: generation, keepSelection: keepSelection)
            }
        } else {
            scanLibraries(
                root: root, additions: additionalRoots, initial: CatalogScan(),
                generation: generation, keepSelection: keepSelection)
        }
    }

    private func scanLibraries(
        root: URL, additions: [URL], initial: CatalogScan, generation: Int, keepSelection: Bool
    ) {
        var roots = [root]
        for addition in additions
        where !roots.contains(where: {
            $0.standardizedFileURL.path == addition.standardizedFileURL.path
        }) {
            roots.append(addition)
        }
        scanAll(roots: roots, merged: initial) { [weak self] result in
            guard let self, self.scanGeneration == generation, self.libraryRoot == root else {
                return
            }
            self.scanningRoot = nil
            switch result {
            case .success(let scan):
                self.apply(scan: scan, keepSelection: keepSelection)
            case .failure(let error):
                self.errorText =
                    (error as? LocalizedError)?.errorDescription
                    ?? "読み込みに失敗しました"
                self.showsError = true
            }
        }
    }

    /// Scans the libraries one after another and folds them into one catalog.
    /// An explicit primary library must scan. Ordinary startup can retain valid
    /// additions when its saved primary is missing; failed roots are reported as issues.
    private func scanAll(
        roots: [URL], merged: CatalogScan,
        completion: @escaping @MainActor @Sendable (Result<CatalogScan, Error>) -> Void
    ) {
        guard let next = roots.first else {
            completion(.success(merged))
            return
        }
        let isPrimary = next == libraryRoot
        scanCatalog(next) { [weak self] result in
            guard let self else { return }
            var combined = merged
            switch result {
            case .success(let scan):
                combined.merge(scan)
            case .failure(let error):
                if isPrimary && (!usesDefaultLibraries || roots.count == 1) {
                    completion(.failure(error))
                    return
                }
                combined.unreadableTranscriptLabels.append(
                    "\(next.lastPathComponent) (\(error.localizedDescription))")
            }
            self.scanAll(roots: Array(roots.dropFirst()), merged: combined, completion: completion)
        }
    }

    private func apply(scan: CatalogScan, keepSelection: Bool) {
        scanIssues = scan.issues
        let previousID = selectedVideoID
        let previousVideo = selectedVideo
        videos = scan.videos
        catalogRevision += 1
        if keepSelection, let previousID,
            let selected = scan.videos.first(where: { $0.id == previousID })
        {
            // A movie released while its volume was away opens again.
            guard openedVideoID == selected.id else {
                openSelection()
                return
            }
            utterances = CueTimeline.utterances(from: selected.cues)
            updateCue(at: currentSeconds)
            return
        }
        if keepSelection, let previousVideo, let recordID = previousVideo.recordingID,
            let current = scan.videos.first(where: {
                $0.recordingID == recordID
                    && $0.broadcastStartJST == previousVideo.broadcastStartJST
            })
        {
            // A new revision can have a different edit clock: select the same recording,
            // but open it paused at zero instead of inventing a cross-revision time mapping.
            selectedVideoID = current.id
            openSelection()
            return
        }
        selectedVideoID = scan.videos.first?.id
        openSelection()
    }

    func handleVideoChange() {
        // All windows share this session. Repeated view notifications must not
        // replace an already opened item or reset a search/summary seek.
        guard openedVideoID != selectedVideo?.id else { return }
        openSelection()
    }

    func openSelection(startingAt seconds: Double? = nil) {
        recordPosition()
        stopObserving()
        loopsVideo = false
        openedVideoID = selectedVideo?.id
        utterances = CueTimeline.utterances(from: selectedVideo?.cues ?? [])
        selectedUtteranceID = nil
        browseAnchorID = nil
        timelineFollowsPlayback = true
        currentSeconds = seconds ?? 0
        videoBGM.open(selectedVideo)
        // An unedited original has no clips, so no lookup or Mac recompute can start for it.
        clips.open(selectedVideo?.isOriginalOnly == true ? nil : selectedVideo?.movieURL)
        guard let video = selectedVideo, !isUnavailable(video) else {
            // Not opened: the returning volume reopens it.
            openedVideoID = nil
            player.replaceCurrentItem(with: nil)
            captions.open(nil)
            return
        }
        captions.open(video.isOriginalOnly ? nil : video.movieURL)
        export.open(video.movieURL)
        // An explicit start (search hit) wins over the saved position.
        pendingResume =
            seconds == nil && positions[PlaybackResume.key(for: video.movieURL)] != nil
            ? video.id : nil
        let asset = AVURLAsset(url: video.movieURL)
        let item = AVPlayerItem(asset: asset)
        item.preferredForwardBufferDuration = 2
        item.audioTimePitchAlgorithm = .timeDomain
        player.appliesMediaSelectionCriteriaAutomatically = false
        legibleSelection?.cancel()
        legibleSelection = Task { [weak self] in
            // An unreadable movie (for example one still being written) must be
            // reported as a movie problem; its transcript stays listed regardless.
            let playable: Bool
            do {
                try Task.checkCancellation()
                playable = try await asset.load(.isPlayable)
                try Task.checkCancellation()
            } catch is CancellationError {
                return
            } catch {
                guard !Task.isCancelled else { return }
                self?.errorText = "動画を開けません（書き出し中または破損）: \(error.localizedDescription)"
                self?.showsError = true
                return
            }
            guard playable else {
                self?.errorText = "この動画は再生できない形式です: \(video.movieURL.lastPathComponent)"
                self?.showsError = true
                return
            }
            await self?.resume(asset, movie: video.id)
            do {
                if let group = try await asset.loadMediaSelectionGroup(for: .legible) {
                    item.select(nil, in: group)
                }
            } catch is CancellationError {
                return
            } catch {
                guard !Task.isCancelled else { return }
                self?.errorText = "字幕トラックを無効化できません: \(error.localizedDescription)"
                self?.showsError = true
            }
        }
        player.automaticallyWaitsToMinimizeStalling = true
        player.replaceCurrentItem(with: item)
        let interval = CMTime(value: 1, timescale: 10)
        timeObserver = player.addPeriodicTimeObserver(forInterval: interval, queue: .main) {
            [weak self] time in
            Task { @MainActor in
                self?.updateCue(at: time.seconds)
                self?.recordPositionIfDue(time.seconds)
            }
        }
        clockObserver = player.addPeriodicTimeObserver(
            forInterval: Self.clockInterval, queue: .main
        ) {
            [weak self] time in
            Task { @MainActor in self?.displaySeconds = time.seconds }
        }
        if let seconds {
            seek(to: seconds)
        } else {
            player.pause()
            player.seek(to: .zero, toleranceBefore: .zero, toleranceAfter: .zero)
            updateCue(at: 0)
        }
    }

    /// Opens the movie paused at its saved position once its duration is known, unless the
    /// user sought or switched meanwhile.
    private func resume(_ asset: AVURLAsset, movie: URL) async {
        guard pendingResume == movie else { return }
        let duration: Double
        do {
            duration = try await asset.load(.duration).seconds
        } catch {
            if pendingResume == movie { pendingResume = nil }
            return
        }
        guard pendingResume == movie, !Task.isCancelled else { return }
        pendingResume = nil
        let start = PlaybackResume.start(
            saved: positions[PlaybackResume.key(for: movie)], duration: duration)
        guard start > 0 else { return }
        updateCue(at: start)
        _ = await player.seek(
            to: CMTime(seconds: start, preferredTimescale: 600), toleranceBefore: .zero,
            toleranceAfter: .zero)
    }

    func seek(toUtterance identifier: Int) {
        guard let utterance = utterances.first(where: { $0.id == identifier }) else {
            return
        }
        resumeTimelineFollow()
        seek(to: utterance.startSeconds)
    }

    func beginTimelineBrowse() {
        timelineFollowsPlayback = false
        browseAnchorID = TimelineFollow.anchorID(
            followsPlayback: false,
            playbackID: selectedUtteranceID,
            browseID: browseAnchorID
        )
    }

    func resumeTimelineFollow() {
        timelineFollowsPlayback = true
        browseAnchorID = nil
    }

    /// Moves by `offset` seconds within the movie, keeping play or pause as it is.
    func skip(by offset: Double) {
        let now = player.currentTime().seconds
        let target = min(max(0, (now.isFinite ? now : currentSeconds) + offset), sliderUpperBound)
        pendingResume = nil
        player.seek(
            to: CMTime(seconds: target, preferredTimescale: 600), toleranceBefore: .zero,
            toleranceAfter: .zero)
        updateCue(at: target)
        recordPosition(at: target)
    }

    func seek(to seconds: Double) {
        let clamped = min(max(0, seconds), sliderUpperBound)
        let time = CMTime(seconds: clamped, preferredTimescale: 600)
        pendingResume = nil
        player.seek(to: time, toleranceBefore: .zero, toleranceAfter: .zero)
        player.play()
        updateCue(at: clamped)
        recordPosition(at: clamped)
    }

    func play(_ hit: SearchHit) {
        showsSearch = false
        resumeTimelineFollow()
        if selectedVideoID != hit.videoID {
            selectedVideoID = hit.videoID
            openSelection(startingAt: hit.cue.startSeconds)
            return
        }
        seek(to: hit.cue.startSeconds)
    }

    /// Plays `clip` from range `index`; its later ranges follow on their own, over and over
    /// when `loop` is set.
    /// Clips loop by default: a clip is short, and watched again and again.
    func play(_ clip: ClipPoint, range index: Int = 0, loop: Bool = true) {
        guard let start = clips.start(clip, range: index, loop: loop) else { return }
        resumeTimelineFollow()
        seek(to: start)
    }

    func updateCue(at seconds: Double) {
        currentSeconds = seconds
        displaySeconds = seconds
        videoBGM.update(at: seconds)
        switch clips.step(at: seconds) {
        case .jump(let start):
            player.seek(
                to: CMTime(seconds: start, preferredTimescale: 600), toleranceBefore: .zero,
                toleranceAfter: .zero)
        case .finish: player.pause()
        case .keep, .leave: break
        }
        if let index = CueTimeline.index(at: seconds, in: utterances) {
            selectedUtteranceID = utterances[index].id
        } else {
            selectedUtteranceID = nil
        }
        let visible = Set(nearbyUtterances.map(\.id))
        if TimelineFollow.shouldFollowPlayback(
            browsing: !timelineFollowsPlayback,
            currentID: selectedUtteranceID,
            visibleIDs: visible
        ) {
            timelineFollowsPlayback = true
            browseAnchorID = nil
        }
    }

    func showUtteranceWindow() {
        if let utterancePanel, utterancePanel.isVisible {
            presentWindow(utterancePanel)
            return
        }
        let hosting = NSHostingController(rootView: UtteranceWindow(session: self))
        let window = NSWindow(contentViewController: hosting)
        window.title = "Transcripts"
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        window.setContentSize(NSSize(width: 560, height: 720))
        window.minSize = NSSize(width: 400, height: 300)
        window.isReleasedWhenClosed = false
        presentWindow(window)
        utterancePanel = window
    }

    /// Watches the opened movie's end while looping: it starts again from its first frame at
    /// the chosen speed. (`AVPlayerLooper` needs a queue player, which drops an unplayable
    /// movie's item and with it the media problem the session reports.)
    private func applyLoop() {
        if let loopObserver {
            NotificationCenter.default.removeObserver(loopObserver)
        }
        loopObserver = nil
        player.actionAtItemEnd = loopsVideo ? .none : .pause
        guard loopsVideo, let item = player.currentItem else { return }
        loopObserver = NotificationCenter.default.addObserver(
            forName: AVPlayerItem.didPlayToEndTimeNotification, object: item, queue: .main
        ) { [weak self] _ in
            Task { @MainActor in self?.restartLoop() }
        }
    }

    func restartLoop() {
        player.seek(to: .zero, toleranceBefore: .zero, toleranceAfter: .zero)
        player.rate = playbackSpeed.rawValue
        updateCue(at: 0)
    }

    func stopObserving() {
        legibleSelection?.cancel()
        for observer in [timeObserver, clockObserver].compactMap(\.self) {
            player.removeTimeObserver(observer)
        }
        timeObserver = nil
        clockObserver = nil
    }
}
