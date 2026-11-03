import CoreGraphics
import Foundation

/// The speakers' round icons of a version-2 plan (`docs/caption-plan.md` "Speaker icons"):
/// each in a white ring, no name, centred on its lane at the bottom edge, for the whole movie.
public enum CaptionSpeakerIcons {
  /// One icon's square in canvas pixels (origin top left) and its ring width.
  public struct Placement: Decodable, Equatable, Sendable {
    public let id: String
    public let x: Int
    public let y: Int
    public let size: Int
    public let ring: Int
  }

  static let ringColour = "#FFFFFF"

  /// Where the plan's speakers with an icon draw it, in lane order.
  public static func placements(_ plan: CaptionPlan) -> [Placement] {
    let width = Double(plan.width)
    let height = Double(plan.height)
    let size = Int((0.08 * height + 0.5).rounded(.down))
    let ring = max(2, Int((Double(size) * 4 / 150 + 0.5).rounded(.down)))
    let gap = Int((0.02 * height + 0.5).rounded(.down))
    return (plan.speakers ?? []).filter { $0.icon != nil }.map { speaker in
      let centre = width / 2 + speaker.laneX
      let left = Int((centre - Double(size) / 2 + 0.5).rounded(.down))
      return Placement(
        id: speaker.id, x: min(max(left, 0), plan.width - size), y: plan.height - gap - size,
        size: size, ring: ring)
    }
  }

  /// The icons of the speakers in `showing` (all when nil) as one full-canvas straight-alpha
  /// bitmap, or nil when none has an icon. `icons` are the files the plan names, by speaker id,
  /// refused unless their SHA-256 matches.
  public static func bitmap(_ plan: CaptionPlan, icons: [String: Data], showing: Set<String>? = nil)
    throws -> MotionCaptionBitmap?
  {
    let shown = placements(plan).filter { showing?.contains($0.id) ?? true }
    guard !shown.isEmpty, let all = plan.speakers else { return nil }
    let speakers = Dictionary(uniqueKeysWithValues: all.map { ($0.id, $0) })
    let (context, space) = try CaptionNameplateRenderer.canvas(
      width: plan.width, height: plan.height)
    for placement in shown {
      guard let speaker = speakers[placement.id], let name = speaker.icon,
        let digest = speaker.iconSha256, let icon = icons[placement.id]
      else { throw CaptionRenderError.invalid("No icon file for speaker \(placement.id)") }
      try CaptionNameplateRenderer.requireDigest(icon, digest, name)
      try CaptionNameplateRenderer.drawIcon(
        icon, named: name,
        disc: CGRect(x: placement.x, y: placement.y, width: placement.size, height: placement.size),
        ring: CGFloat(placement.ring), ringColour: ringColour, space: space, in: context)
    }
    guard let image = context.makeImage() else {
      throw CaptionRenderError.unavailable("Cannot read the speaker icon canvas")
    }
    return try CaptionNameplateRenderer.straight(image, width: plan.width, height: plan.height)
  }
}

/// The nameplate and speaker icons as one bitmap under the captions (nil when there is none).
public enum CaptionUnderlay {
  public static func bitmap(
    nameplate: MotionCaptionBitmap?, icons: MotionCaptionBitmap?, width: Int, height: Int
  ) -> MotionCaptionBitmap? {
    let layers = [nameplate, icons].compactMap { $0 }
    guard layers.count > 1 else { return layers.first }
    return MotionCaptionBitmap(
      originX: 0, originY: 0, width: width, height: height,
      rgba: CaptionFrame.composite(layers, width: width, height: height))
  }
}
