import AVKit
import AppKit
import CaptionRender
import QuartzCore
import TranscriptCore

/// What the player draws over the movie: the placed plan's captions over the nameplate and
/// speaker icons, when shown.
struct CaptionOverlaySettings {
    var captions: DeliveryCaptions?
    /// `captions`' plan placed by its layout, without hidden speakers (as exported).
    var plan: CaptionPlan?
    /// Changes whenever `plan` or `underlay` is rebuilt.
    var revision = 0
    var fill: CaptionFill?
    var speakerFills: [String: CaptionFill] = [:]
    var showsCaptions = false
    var underlay: CaptionUnderlayImage?
}

/// The live style-20 captions over `AVPlayerView`'s video, below its controls: one layer per
/// visible caption bitmap, placed on the displayed video rect and redrawn only when the set
/// of bitmaps changes. It never takes mouse events.
@MainActor
final class CaptionOverlayView: NSView {
    /// Receives a drawing failure (the captions are then hidden).
    var report: (String) -> Void = { _ in }
    private(set) var settings = CaptionOverlaySettings()
    private var overlay: CaptionOverlay?
    /// The drawn layers and their canvas rects, in drawing order (nameplate first).
    private(set) var placed: [(layer: CALayer, rect: CGRect)] = []
    private(set) var images: [CGImage] = []
    private weak var player: AVPlayer?
    private var timeObserver: Any?

    override init(frame: NSRect) {
        super.init(frame: frame)
        wantsLayer = true
        autoresizingMask = [.width, .height]
    }

    required init?(coder: NSCoder) { nil }

    override func hitTest(_: NSPoint) -> NSView? { nil }

    override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        place()
    }

    /// Applies `settings` for `player`, rebuilding the renderer only when the plan, fill or
    /// position or shown speakers changed.
    func apply(_ settings: CaptionOverlaySettings, player: AVPlayer) {
        let replaced =
            settings.captions !== self.settings.captions
            || settings.revision != self.settings.revision || settings.fill != self.settings.fill
            || settings.speakerFills != self.settings.speakerFills
        self.settings = settings
        if replaced || self.player !== player { stopObserving() }
        if replaced {
            overlay = nil
            do {
                // Already placed: no layout, so hiding a speaker moves no other caption.
                overlay = try settings.plan.map {
                    try CaptionOverlay(
                        plan: $0, fill: settings.fill, speakerFills: settings.speakerFills)
                }
            } catch {
                report("字幕を描けません: \(error)")
            }
        }
        guard let rate = settings.plan?.rate, settings.showsCaptions, overlay != nil else {
            stopObserving()
            show([])
            return
        }
        self.player = player
        if timeObserver == nil {
            // Media time: once per movie frame at any speed, and on every seek.
            timeObserver = player.addPeriodicTimeObserver(
                forInterval: CMTime(value: 1, timescale: CMTimeScale(rate)), queue: .main
            ) { [weak self] time in
                MainActor.assumeIsolated { self?.draw(atSeconds: time.seconds) }
            }
        }
        draw(atSeconds: player.currentTime().seconds)
    }

    func stopObserving() {
        if let timeObserver { player?.removeTimeObserver(timeObserver) }
        timeObserver = nil
    }

    func draw(atSeconds seconds: Double) {
        guard settings.showsCaptions, var overlay, let plan = settings.plan else { return }
        let frame = CaptionGeometry.frame(atSeconds: seconds, rate: plan.rate)
        let layers: [CaptionLayer]
        do {
            layers = try overlay.layers(at: frame)
        } catch {
            self.overlay = nil
            show([])
            report("字幕を描けません: \(error)")
            return
        }
        self.overlay = overlay
        let canvas = CGRect(x: 0, y: 0, width: plan.width, height: plan.height)
        let under = settings.underlay.map { [($0.image, canvas)] } ?? []
        show(under + layers.map { ($0.image, $0.rect) })
        place()
    }

    private func show(_ drawn: [(image: CGImage, rect: CGRect)]) {
        guard drawn.count != images.count || !zip(drawn, images).allSatisfy({ $0.image === $1 })
        else { return }
        images = drawn.map(\.image)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        for (index, item) in drawn.enumerated() {
            if index == placed.count {
                let sublayer = CALayer()
                sublayer.contentsGravity = .resize
                layer?.addSublayer(sublayer)
                placed.append((sublayer, item.rect))
            }
            // As is: the image's colour space is the one the video is decoded in.
            placed[index].layer.contents = item.image
            placed[index].rect = item.rect
        }
        for unused in placed.dropFirst(drawn.count) { unused.layer.removeFromSuperlayer() }
        placed.removeLast(placed.count - drawn.count)
    }

    /// Lays the drawn layers on the displayed video: the plan's canvas (the movie's size)
    /// fitted in this view, which covers the player's view as its aspect-fit video does.
    /// AVPlayerView's `videoBounds` is not used: it is empty until the movie is ready, and
    /// AVKit raises when its controller is freed while that property is observed.
    func place() {
        guard let plan = settings.plan else { return }
        let canvas = CGSize(width: plan.width, height: plan.height)
        let video = AVMakeRect(aspectRatio: canvas, insideRect: bounds)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        for item in placed {
            item.layer.frame = CaptionGeometry.frame(
                of: item.rect, canvas: canvas, in: video, flipped: isFlipped)
        }
    }
}
