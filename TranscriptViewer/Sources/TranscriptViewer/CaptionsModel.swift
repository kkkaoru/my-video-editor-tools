import AppKit
import CaptionRender
import Foundation
import Observation
import TranscriptCore

/// The nameplate and the shown speakers' icons as one canvas under the captions.
final class CaptionUnderlayImage: Sendable {
    let bitmap: MotionCaptionBitmap
    let image: CGImage

    init(_ bitmap: MotionCaptionBitmap) throws {
        self.bitmap = bitmap
        image = try bitmap.image()
    }
}

/// The open video's caption plan and how the viewer draws it: shown per video (off by default
/// when the delivery already has them burned), its position and shown speakers, and an
/// icon/fill style shared by every video.
@MainActor @Observable
final class CaptionsModel {
    typealias Loader = @Sendable (URL) async throws -> DeliveryCaptions?
    static let shownKey = "TranscriptViewer.captionsShown"
    static let nameplateKey = "TranscriptViewer.captionsNameplate"
    static let gradientKey = "TranscriptViewer.captionsGradient"
    static let topKey = "TranscriptViewer.captionsTop"
    static let bottomKey = "TranscriptViewer.captionsBottom"

    private(set) var captions: DeliveryCaptions?
    private(set) var issue: String?
    private(set) var movie: URL?
    /// The open movie's caption position, as saved beside it (`<stem>.caption-layout.json`).
    private(set) var layout = CaptionLayout.plan
    /// Speakers hidden in the open movie (`CaptionPlan.unassigned`: titles without one).
    var hiddenSpeakers: Set<String> = [] { didSet { refresh() } }
    /// The plan placed by `layout` without the hidden speakers: what is drawn and exported.
    private(set) var placed: CaptionPlan?
    /// アイコン表示: the nameplate and the shown speakers' icons.
    private(set) var underlay: CaptionUnderlayImage?
    /// Bumped whenever `placed` or `underlay` is rebuilt.
    private(set) var revision = 0
    /// Why the plan could not be placed or its icons drawn (nothing is drawn then).
    private(set) var placementIssue: String?
    /// Per-movie overrides of the burned-based default (`PlaybackResume.key`).
    private var shownOverrides: [String: Bool]
    var showsNameplate: Bool {
        didSet {
            defaults?.set(showsNameplate, forKey: Self.nameplateKey)
            refresh()
        }
    }
    var usesGradient: Bool { didSet { defaults?.set(usesGradient, forKey: Self.gradientKey) } }
    /// `#RRGGBB` overrides of the plan's face colours; nil keeps the plan's.
    var topColour: String? { didSet { defaults?.set(topColour, forKey: Self.topKey) } }
    var bottomColour: String? { didSet { defaults?.set(bottomColour, forKey: Self.bottomKey) } }
    @ObservationIgnored private let defaults: UserDefaults?
    @ObservationIgnored private let load: Loader
    @ObservationIgnored private(set) var loading: Task<Void, Never>?

    init(
        defaults: UserDefaults? = nil,
        load: @escaping Loader = { try DeliveryCaptions.load(movie: $0) }
    ) {
        self.defaults = defaults
        self.load = load
        shownOverrides = defaults?.dictionary(forKey: Self.shownKey) as? [String: Bool] ?? [:]
        showsNameplate = defaults?.object(forKey: Self.nameplateKey) as? Bool ?? true
        usesGradient = defaults?.object(forKey: Self.gradientKey) as? Bool ?? true
        topColour = defaults?.string(forKey: Self.topKey).flatMap(Self.validHex)
        bottomColour = defaults?.string(forKey: Self.bottomKey).flatMap(Self.validHex)
    }

    /// Reads `movie`'s plan off the main actor; nil clears it (no movie, or its volume is gone).
    func open(_ movie: URL?) {
        loading?.cancel()
        self.movie = movie
        captions = nil
        issue = nil
        layout = .plan
        hiddenSpeakers = []
        guard let movie else { return }
        let load = load
        loading = Task { [weak self] in
            let result: Result<DeliveryCaptions?, Error>
            do {
                result = .success(try await load(movie))
            } catch {
                result = .failure(error)
            }
            guard !Task.isCancelled, let self, self.movie == movie else { return }
            switch result {
            case .success(let captions):
                self.captions = captions
                layout = captions?.layout ?? .plan
                refresh()
            case .failure(let error): issue = error.localizedDescription
            }
        }
    }

    var isShown: Bool {
        get {
            guard let movie, let captions else { return false }
            return shownOverrides[PlaybackResume.key(for: movie)] ?? !captions.plan.burned
        }
        set {
            guard let movie else { return }
            shownOverrides[PlaybackResume.key(for: movie)] = newValue
            defaults?.set(shownOverrides, forKey: Self.shownKey)
        }
    }

    // Without a plan nothing is drawn; white only gives the hidden pickers a value.
    var top: String { topColour ?? captions?.plan.style.gradientTop ?? "#FFFFFF" }
    var bottom: String { bottomColour ?? captions?.plan.style.gradientBottom ?? "#FFFFFF" }

    /// The viewer's fill; nil while untouched, so every title keeps its own style's colours.
    var fill: CaptionFill? {
        guard topColour != nil || bottomColour != nil || !usesGradient else { return nil }
        return CaptionFill(top: top, bottom: bottom, gradient: usesGradient)
    }

    /// Single colours for every speaker while グラデーション is off (version 2).
    var speakerFills: [String: CaptionFill] {
        usesGradient ? [:] : captions?.plan.speakerFills(gradient: false) ?? [:]
    }

    var overlaySettings: CaptionOverlaySettings {
        CaptionOverlaySettings(
            captions: captions, plan: placed, revision: revision, fill: fill,
            speakerFills: speakerFills, showsCaptions: isShown, underlay: underlay)
    }

    /// The plan's speakers, and unassigned speech when it has any (version 2).
    var speakers: [CaptionSpeaker] { captions?.plan.speakers ?? [] }
    var hasUnassigned: Bool {
        !speakers.isEmpty && captions?.plan.titles.contains { $0.speaker == nil } == true
    }

    /// アイコン表示 has something to show: a nameplate or a speaker icon.
    var hasIcons: Bool {
        captions.map { $0.nameplate != nil || !$0.speakerIcons.isEmpty } ?? false
    }

    func isShowing(_ speaker: String) -> Bool { !hiddenSpeakers.contains(speaker) }

    func show(_ speaker: String, _ shown: Bool) {
        if shown { hiddenSpeakers.remove(speaker) } else { hiddenSpeakers.insert(speaker) }
    }

    /// Re-places the plan and redraws the icons after the plan, position, speakers or
    /// アイコン表示 changed; a failure is reported and draws nothing.
    func refresh() {
        revision += 1
        placed = nil
        underlay = nil
        placementIssue = nil
        guard let captions else { return }
        do {
            placed = try CaptionPlacement.resolve(
                plan: captions.plan, layout: layout, faces: captions.faces
            ).showing(hiding: hiddenSpeakers)
            guard showsNameplate else { return }
            let icons = try CaptionSpeakerIcons.bitmap(
                captions.plan, icons: captions.speakerIcons,
                showing: Set(captions.speakerIcons.keys).subtracting(hiddenSpeakers))
            underlay = try CaptionUnderlay.bitmap(
                nameplate: captions.nameplate, icons: icons, width: captions.plan.width,
                height: captions.plan.height
            ).map(CaptionUnderlayImage.init)
        } catch {
            placementIssue = "字幕を配置できません: \(error)"
        }
    }

    /// 顔の上 needs the movie's face track.
    var hasFaces: Bool { captions?.faces != nil }

    /// Moves the captions to `mode` and saves it beside the movie.
    func place(_ mode: CaptionLayout.Mode) { save(layout.overriding(mode: mode)) }

    /// Shifts the captions by canvas pixels (+y down) and saves it.
    func nudge(x: Double = 0, y: Double = 0) {
        save(
            layout.overriding(
                offsetX: (layout.offsetX ?? 0) + x, offsetY: (layout.offsetY ?? 0) + y))
    }

    func setOffset(x: Double? = nil, y: Double? = nil) {
        save(layout.overriding(offsetX: x, offsetY: y))
    }

    /// The plan's own positions again, without offsets (each speaker's own stay).
    func resetPosition() {
        save(
            CaptionLayout(
                mode: .plan, margin: layout.margin, faceGap: layout.faceGap,
                minScore: layout.minScore, maxDrift: layout.maxDrift, speakers: layout.speakers))
    }

    /// A speaker's own position, nil when it follows the whole layout.
    func speakerOptions(_ speaker: String) -> CaptionLayout.SpeakerOptions? {
        layout.speakers?[speaker]
    }

    /// Gives a speaker its own kind of position; nil follows the whole layout again.
    func place(_ speaker: String, _ mode: CaptionLayout.Mode?) {
        update(speaker) {
            CaptionLayout.SpeakerOptions(
                mode: mode, offsetX: $0?.offsetX, offsetY: $0?.offsetY, margin: $0?.margin,
                faceGap: $0?.faceGap, minScore: $0?.minScore, maxDrift: $0?.maxDrift)
        }
    }

    func setOffset(_ speaker: String, x: Double? = nil, y: Double? = nil) {
        update(speaker) {
            CaptionLayout.SpeakerOptions(
                mode: $0?.mode, offsetX: x ?? $0?.offsetX, offsetY: y ?? $0?.offsetY,
                margin: $0?.margin, faceGap: $0?.faceGap, minScore: $0?.minScore,
                maxDrift: $0?.maxDrift)
        }
    }

    /// The speaker follows the whole layout's kind and offsets again.
    func resetPosition(_ speaker: String) {
        update(speaker) {
            CaptionLayout.SpeakerOptions(
                margin: $0?.margin, faceGap: $0?.faceGap, minScore: $0?.minScore,
                maxDrift: $0?.maxDrift)
        }
    }

    /// The layout with one speaker's options changed; a speaker left with none is removed,
    /// and a layout left without speakers is version 1 again.
    private func update(
        _ speaker: String,
        _ change: (CaptionLayout.SpeakerOptions?) -> CaptionLayout.SpeakerOptions
    ) {
        var speakers = layout.speakers ?? [:]
        let options = change(speakers[speaker])
        speakers[speaker] = options == CaptionLayout.SpeakerOptions() ? nil : options
        save(
            CaptionLayout(
                mode: layout.mode, offsetX: layout.offsetX, offsetY: layout.offsetY,
                margin: layout.margin, faceGap: layout.faceGap, minScore: layout.minScore,
                maxDrift: layout.maxDrift, speakers: speakers.isEmpty ? nil : speakers))
    }

    private func save(_ wanted: CaptionLayout) {
        guard let movie, captions != nil else { return }
        do {
            try CaptionLayoutFile.write(wanted, movie: movie)
            layout = wanted
            refresh()
        } catch {
            issue = "字幕位置を保存できません: \(error.localizedDescription)"
        }
    }

    /// A drawing failure of the open plan, shown beside the caption controls.
    func report(_ issue: String) { self.issue = issue }

    /// The plan's look again (whether captions show stays as chosen).
    func reset() {
        showsNameplate = true
        usesGradient = true
        topColour = nil
        bottomColour = nil
    }

    /// `#RRGGBB` of `colour` in sRGB, which the colour picker shows.
    static func hex(_ colour: NSColor) -> String? {
        guard let rgb = colour.usingColorSpace(.sRGB) else { return nil }
        let channels = [rgb.redComponent, rgb.greenComponent, rgb.blueComponent].map {
            Int((min(max($0, 0), 1) * 255).rounded())
        }
        return "#" + channels.map { String(format: "%02X", $0) }.joined()
    }

    /// The sRGB colour of a `#RRGGBB` string; nil for anything else.
    static func colour(_ hex: String) -> NSColor? {
        guard hex.utf8.count == 7, hex.hasPrefix("#"),
            let value = UInt32(hex.dropFirst(), radix: 16)
        else { return nil }
        let channel = { (shift: UInt32) in CGFloat((value >> shift) & 0xFF) / 255 }
        return NSColor(srgbRed: channel(16), green: channel(8), blue: channel(0), alpha: 1)
    }

    static func validHex(_ text: String) -> String? { colour(text) == nil ? nil : text }
}
