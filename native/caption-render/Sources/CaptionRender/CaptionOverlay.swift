import CoreGraphics
import Foundation

/// One visible caption on a frame: its image and where it lies on the canvas.
public struct CaptionLayer: Sendable {
  public let bitmap: MotionCaptionBitmap
  public let image: CGImage
  /// Canvas pixels, origin at the top left.
  public var rect: CGRect { bitmap.rect }
}

/// The captions of one canvas for live playback: titles are laid out only while they show and
/// each keeps only its current (visible graphemes) bitmap, so memory stays bounded by the
/// titles on screen at once, however long the plan.
///
/// To look like the burned delivery, show each layer's `image` as is (do not re-tag or
/// convert it: its `videoColorSpace` is the space AVFoundation decodes the video in) at `rect`
/// scaled by the same factor as the video's canvas. Opaque pixels then match the burned video;
/// partly transparent edges differ slightly, because the burned path blends straight alpha on
/// the encoded code values while the window server blends in its own working space.
public struct CaptionOverlay {
  struct State {
    let renderer: MotionCaptionRenderer
    var visible: Int
    var layer: CaptionLayer
  }

  public let titles: [MotionCaptionTitle]
  public let style: MotionCaptionStyle
  public let fill: CaptionFill?
  /// Face colours per `MotionCaptionTitle.speaker`, over `fill` for that speaker's titles.
  public let speakerFills: [String: CaptionFill]
  /// Drawn under the captions on every frame by `draw(frame:in:)`: the nameplate and speaker
  /// icons (`CaptionUnderlay.bitmap`); `layers(at:)` leaves it to the caller.
  public let underlay: MotionCaptionBitmap?
  public let canvasWidth: Int
  public let canvasHeight: Int
  var states: [Int: State] = [:]
  /// Bitmaps drawn so far (cache misses).
  public private(set) var drawn = 0

  /// `fill` replaces every title's face colours (its own style's included); nil keeps them.
  /// `speakerFills` gives a speaker's titles their own colours instead (multiple speakers;
  /// titles without a speaker, or of a speaker not listed, use `fill`).
  public init(
    titles: [MotionCaptionTitle], style: MotionCaptionStyle, fill: CaptionFill? = nil,
    speakerFills: [String: CaptionFill] = [:], underlay: MotionCaptionBitmap? = nil,
    canvasWidth: Int, canvasHeight: Int
  ) throws {
    try style.validate()
    for chosen in [fill].compactMap({ $0 }) + speakerFills.values {
      _ = try style.applying(chosen)
    }
    for title in titles { try title.validate() }
    self.titles = titles
    self.style = style
    self.fill = fill
    self.speakerFills = speakerFills
    self.underlay = underlay
    self.canvasWidth = canvasWidth
    self.canvasHeight = canvasHeight
  }

  /// The plan's titles on its canvas, placed by `layout` (`faces` is needed by `aboveFace`).
  /// nil keeps the titles where they are: pass it only for a plan already resolved
  /// (`CaptionPlacement.resolve`); a version-2 plan as written needs at least `.plan` for its
  /// stacking. A version-2 plan's speakers are drawn in their own colours unless
  /// `speakerFills` says otherwise.
  public init(
    plan: CaptionPlan, layout: CaptionLayout? = nil, faces: CaptionFaces? = nil,
    fill: CaptionFill? = nil, speakerFills: [String: CaptionFill] = [:],
    underlay: MotionCaptionBitmap? = nil
  ) throws {
    let placed = try layout.map {
      try CaptionPlacement.resolve(plan: plan, layout: $0, faces: faces)
    }
    let plan = placed ?? plan
    try self.init(
      titles: plan.titles, style: plan.style, fill: fill,
      speakerFills: plan.speakerFills().merging(speakerFills) { $1 }, underlay: underlay,
      canvasWidth: plan.width,
      canvasHeight: plan.height)
  }

  /// The captions showing on `frame`, in title order (later ones are drawn on top).
  public mutating func layers(at frame: Int) throws -> [CaptionLayer] {
    var shown: [CaptionLayer] = []
    for (index, title) in titles.enumerated() {
      let visible = title.visible(at: frame)
      guard visible > 0 else {
        states[index] = nil
        continue
      }
      if var state = states[index] {
        if state.visible != visible {
          state.layer = try layer(state.renderer, visible: visible)
          state.visible = visible
          states[index] = state
        }
        shown.append(state.layer)
        continue
      }
      let renderer = try MotionCaptionRenderer(
        title, style: try resolved(title.style ?? style, speaker: title.speaker),
        canvasWidth: canvasWidth,
        canvasHeight: canvasHeight)
      let layer = try layer(renderer, visible: visible)
      states[index] = State(renderer: renderer, visible: visible, layer: layer)
      shown.append(layer)
    }
    return shown
  }

  /// Draws `frame`'s captions, composited exactly as the burned movie's frame, into a context
  /// of the canvas size (CoreGraphics converts `MotionCaptionBitmap.videoColorSpace` to the context's space).
  public mutating func draw(frame: Int, in context: CGContext) throws {
    let bitmaps = (underlay.map { [$0] } ?? []) + (try layers(at: frame).map(\.bitmap))
    let image = try CaptionFrame.image(bitmaps, width: canvasWidth, height: canvasHeight)
    context.draw(image, in: CGRect(x: 0, y: 0, width: canvasWidth, height: canvasHeight))
  }

  func resolved(_ style: MotionCaptionStyle, speaker: String?) throws -> MotionCaptionStyle {
    guard let chosen = speaker.flatMap({ speakerFills[$0] }) ?? fill else { return style }
    return try style.applying(chosen)
  }

  mutating func layer(_ renderer: MotionCaptionRenderer, visible: Int) throws -> CaptionLayer {
    let bitmap = try renderer.bitmap(visible: visible)
    drawn += 1
    return CaptionLayer(bitmap: bitmap, image: try bitmap.image())
  }
}
