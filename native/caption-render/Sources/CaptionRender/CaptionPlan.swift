import Foundation

/// A delivery's caption plan (`<movie stem>.captions.json`, written by `replay.caption_plan`):
/// the style-20 titles to draw live over the delivery movie, every frame on the delivery's
/// clock. Version 1 has one speaker; version 2 adds a `speakers` table (`docs/caption-plan.md`
/// "Multiple speakers").
public struct CaptionPlan: Codable, Equatable, Sendable {
  public static let versions = 1...2
  public static let clock = "delivery"
  /// How titles without a speaker are named where speakers are chosen (`--speaker`).
  public static let unassigned = "unassigned"
  static let maximumSide = 8192

  public let version: Int
  public let clock: String
  /// The delivery movie's file name, beside the plan.
  public let movie: String
  /// Whether the delivery already has these captions burned in.
  public let burned: Bool
  public let width: Int
  public let height: Int
  public let rate: Int
  public let frames: Int
  public let style: MotionCaptionStyle
  /// In display order. Version 1: none overlaps the next; version 2: none overlaps another
  /// title of its own speaker (unassigned titles may overlap and are stacked).
  public let titles: [MotionCaptionTitle]
  public let nameplate: CaptionNameplate?
  /// Version 2 only: the speakers in lane order, left to right.
  public let speakers: [CaptionSpeaker]?
  /// The margins captions keep clear of, in canvas pixels (`replay.caption_safe_zone`). Both or
  /// neither: a plan without them keeps only each title's anchor inside the canvas.
  public let safeLeft: Int?
  public let safeRight: Int?

  /// This plan with its titles replaced (a resolved placement), validated like the plan.
  public func replacing(titles: [MotionCaptionTitle]) throws -> CaptionPlan {
    let plan = CaptionPlan(
      version: version, clock: clock, movie: movie, burned: burned, width: width,
      height: height, rate: rate, frames: frames, style: style, titles: titles,
      nameplate: nameplate, speakers: speakers, safeLeft: safeLeft, safeRight: safeRight)
    try plan.validate()
    return plan
  }

  /// This plan with only the titles of `only` (every speaker when nil) that are not in
  /// `hiding`; `CaptionPlan.unassigned` names the titles without a speaker. Resolve positions
  /// before choosing speakers, so that shown titles keep their places.
  public func showing(only: Set<String>? = nil, hiding: Set<String> = []) throws -> CaptionPlan {
    let known = Set((speakers ?? []).map(\.id)).union([Self.unassigned])
    guard only.map({ $0.isSubset(of: known) }) ?? true, hiding.isSubset(of: known) else {
      throw CaptionRenderError.invalid(
        "Unknown speaker: the plan has \(known.sorted().joined(separator: ", "))")
    }
    return try replacing(
      titles: titles.filter {
        let name = $0.speaker ?? Self.unassigned
        return (only?.contains(name) ?? true) && !hiding.contains(name)
      })
  }

  /// Each speaker's own colours (version 2), single fills of `colorTop` when `gradient` is off.
  public func speakerFills(gradient: Bool = true) -> [String: CaptionFill] {
    Dictionary(
      uniqueKeysWithValues: (speakers ?? []).map {
        ($0.id, CaptionFill(top: $0.colorTop, bottom: $0.colorBottom, gradient: gradient))
      })
  }

  static let keys: Set<String> = [
    "version", "clock", "movie", "burned", "width", "height", "rate", "frames", "style",
    "titles", "nameplate",
  ]
  static let safeKeys: Set<String> = ["safeLeft", "safeRight"]
  static let styleKeys: Set<String> = [
    "fontName", "fontSize", "faceGrow", "keyline", "whiteBorder", "rim", "shadowOpacity",
    "shadowOffsetY", "shadowSigma", "gradientTop", "gradientBottom", "topLocation",
    "bottomLocation", "outputGamma",
  ]
  static let titleKeys: Set<String> = [
    "text", "startFrame", "endFrame", "x", "y", "graphemeFrames",
  ]
  static let titleOptions: Set<String> = ["style", "alignment", "speaker"]
  static let speakerKeys: Set<String> = ["id", "name", "colorTop", "colorBottom", "laneX"]
  static let speakerOptions: Set<String> = ["icon", "iconSha256"]
  static let nameplateKeys: Set<String> = [
    "burned", "icon", "iconSha256", "iconX", "iconY", "iconSize", "ringWidth", "ringColour",
    "name", "nameFont", "nameFontSha256", "nameFontVariation", "nameSize", "nameColour",
    "nameStrokeWidth", "nameStrokeColour", "nameX", "nameY", "nameAnchor",
  ]

  /// The plan in `data`, validated; unknown keys are refused, as `replay.caption_plan` does.
  public static func decode(_ data: Data) throws -> CaptionPlan {
    let shape = try JSONShape.of(data)
    let speakers = shape.has("speakers")
    let required = speakers ? keys.union(["speakers"]) : keys
    try shape.requireKeys(required, allowed: required.union(safeKeys), in: "A caption plan")
    for speaker in shape["speakers"].items {
      try speaker.requireKeys(
        speakerKeys, allowed: speakerKeys.union(speakerOptions), in: "A caption speaker")
    }
    try shape["style"].requireKeys(styleKeys, in: "A caption style")
    for title in shape["titles"].items {
      try title.requireKeys(
        titleKeys, allowed: titleKeys.union(titleOptions), in: "A caption title")
      try title["style"].requireKeys(styleKeys, in: "A caption style")
    }
    try shape["nameplate"].requireKeys(nameplateKeys, in: "A caption nameplate")
    let plan: CaptionPlan
    do {
      plan = try JSONDecoder().decode(CaptionPlan.self, from: data)
    } catch {
      throw CaptionRenderError.invalid("Unreadable caption plan: \(error)")
    }
    guard speakers == (plan.version == 2) else {
      throw CaptionRenderError.invalid("Only a version-2 caption plan has speakers")
    }
    try plan.validate()
    return plan
  }

  /// Throws `CaptionRenderError.invalid` unless this is a delivery-clock plan whose style,
  /// titles and (version 2) speakers are valid and whose titles keep their order within the
  /// movie: version 1 titles never overlap; version 2 titles start in order and never overlap
  /// another title of their own speaker (unassigned titles may).
  public func validate() throws {
    guard Self.versions.contains(version), clock == Self.clock, !movie.isEmpty,
      URL(fileURLWithPath: movie).lastPathComponent == movie,
      (1...Self.maximumSide).contains(width), (1...Self.maximumSide).contains(height), rate > 0,
      frames > 0, (version == 2) == (speakers != nil),
      (safeLeft == nil) == (safeRight == nil),
      (safeLeft ?? 0) >= 0, (safeRight ?? 0) >= 0, (safeLeft ?? 0) + (safeRight ?? 0) < width
    else { throw CaptionRenderError.invalid("Invalid caption plan header") }
    try style.validate()
    let ids = try validateSpeakers()
    var previous = 0
    var ends: [String: Int] = [:]
    for title in titles {
      try title.validate()
      try title.style?.validate()
      guard title.endFrame <= frames else {
        throw CaptionRenderError.invalid("Caption title \(title.text) runs past the end")
      }
      if version == 1 {
        guard title.startFrame >= previous else {
          throw CaptionRenderError.invalid("Caption title \(title.text) overlaps another")
        }
        previous = title.endFrame
        continue
      }
      guard title.startFrame >= previous, title.speaker.map(ids.contains) ?? true else {
        throw CaptionRenderError.invalid("Caption title \(title.text) is out of order or unknown")
      }
      previous = title.startFrame
      guard let speaker = title.speaker else { continue }
      guard title.startFrame >= ends[speaker, default: 0] else {
        throw CaptionRenderError.invalid("Caption title \(title.text) overlaps its speaker's")
      }
      ends[speaker] = title.endFrame
    }
    try nameplate?.validate()
  }

  /// The speaker ids, refused unless there are at least two, unique and valid.
  func validateSpeakers() throws -> Set<String> {
    guard let speakers else { return [] }
    let ids = Set(speakers.map(\.id))
    guard speakers.count >= 2, ids.count == speakers.count, !ids.contains(Self.unassigned) else {
      throw CaptionRenderError.invalid("A version-2 plan has two or more distinct speakers")
    }
    for speaker in speakers { try speaker.validate() }
    return ids
  }
}

/// One speaker of a version-2 plan, in lane order.
public struct CaptionSpeaker: Codable, Equatable, Sendable {
  public let id: String
  public let name: String
  /// sRGB `#RRGGBB` gradient stops of this speaker's captions.
  public let colorTop: String
  public let colorBottom: String
  /// The lane's Motion anchor x (canvas-centre origin).
  public let laneX: Double
  /// Optional icon file beside the plan and its SHA-256, given together.
  public let icon: String?
  public let iconSha256: String?

  public func validate() throws {
    let iconValid: Bool =
      switch (icon, iconSha256) {
      case (nil, nil): true
      case (let icon?, let digest?):
        !icon.isEmpty && URL(fileURLWithPath: icon).lastPathComponent == icon
          && CaptionNameplate.isDigest(digest)
      default: false
      }
    guard !id.isEmpty, !name.isEmpty, laneX.isFinite, iconValid,
      CaptionRGB(hex: colorTop) != nil, CaptionRGB(hex: colorBottom) != nil
    else { throw CaptionRenderError.invalid("Invalid caption speaker \(id)") }
  }
}

/// The recording speaker's nameplate, as `replay.caption_layout.badge_image` draws it: the
/// round icon in a ring and the name below it. Lengths are canvas pixels, origin top left.
public struct CaptionNameplate: Codable, Equatable, Sendable {
  public let burned: Bool
  /// File name of the icon beside the plan, and its SHA-256 (lowercase hex).
  public let icon: String
  public let iconSha256: String
  public let iconX: Int
  public let iconY: Int
  public let iconSize: Int
  public let ringWidth: Int
  public let ringColour: String
  public let name: String
  /// File name of the name's font and its SHA-256; the Viewer supplies the file.
  public let nameFont: String
  public let nameFontSha256: String
  /// The font's named instance (`Bold`).
  public let nameFontVariation: String
  /// Pillow's truetype size: pixels per em.
  public let nameSize: Int
  public let nameColour: String
  public let nameStrokeWidth: Int
  public let nameStrokeColour: String
  public let nameX: Double
  public let nameY: Double
  /// Pillow's text anchor; only `mt` (advance middle, top of the ink) is drawn.
  public let nameAnchor: String

  public func validate() throws {
    guard Self.isDigest(iconSha256), Self.isDigest(nameFontSha256),
      !icon.isEmpty, URL(fileURLWithPath: icon).lastPathComponent == icon,
      !nameFont.isEmpty, URL(fileURLWithPath: nameFont).lastPathComponent == nameFont,
      iconSize > 0, ringWidth >= 0, 2 * ringWidth < iconSize, nameSize > 0,
      nameStrokeWidth >= 0, nameX.isFinite, nameY.isFinite, !name.isEmpty, nameAnchor == "mt",
      [ringColour, nameColour, nameStrokeColour].allSatisfy({ CaptionRGB(hex: $0) != nil })
    else { throw CaptionRenderError.invalid("Invalid caption nameplate") }
  }

  static func isDigest(_ text: String) -> Bool {
    text.utf8.count == 64
      && text.utf8.allSatisfy { (0x30...0x39).contains($0) || (0x61...0x66).contains($0) }
  }
}
