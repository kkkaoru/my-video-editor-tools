import Foundation

/// Why a caption could not be drawn.
public enum CaptionRenderError: Error, Equatable, Sendable, CustomStringConvertible {
  /// The input breaks its contract (style, title, plan or nameplate).
  case invalid(String)
  /// A system resource (font, bitmap context, file) could not be provided.
  case unavailable(String)

  /// Worded as ProAppsCore's errors, so `caption-alpha-render` reports read the same.
  public var description: String {
    switch self {
    case .invalid(let reason): "Invalid request: \(reason)"
    case .unavailable(let reason): "Unavailable: \(reason)"
    }
  }
}

/// An sRGB-intent colour parsed from `#RRGGBB`, components in [0, 1].
struct CaptionRGB: Equatable, Sendable {
  let red: Double
  let green: Double
  let blue: Double

  init?(hex: String) {
    let digits = Array(hex.utf8)
    guard digits.count == 7, digits[0] == UInt8(ascii: "#"),
      let value = UInt32(String(hex.dropFirst()), radix: 16)
    else { return nil }
    red = Double((value >> 16) & 0xFF) / 255
    green = Double((value >> 8) & 0xFF) / 255
    blue = Double(value & 0xFF) / 255
  }
}

/// Caption style 20 as Motion renders its finished three-object titles, in output pixels.
/// Motion grows an outline by an axis-aligned square of the outline's width, not a disc: the
/// band is that wide beside horizontal and vertical edges and √2 times as wide across
/// diagonal ones, convex corners stay square and holes shrink into diamonds (measured on
/// Motion renders of 〇, 口, ◆, ＼ and ／).
public struct MotionCaptionStyle: Codable, Equatable, Sendable {
  public let fontName: String
  public let fontSize: Double
  /// Growth of the face beyond CoreText's outline.
  public let faceGrow: Double
  /// Black keyline, white band and outer black rim growth beyond the outline.
  public let keyline: Double
  public let whiteBorder: Double
  public let rim: Double
  /// The rim object's shadow is cast by the glyphs alone: Gaussian, then moved down.
  public let shadowOpacity: Double
  public let shadowOffsetY: Double
  public let shadowSigma: Double
  /// sRGB intent of the gradient's two colours (#RRGGBB).
  public let gradientTop: String
  public let gradientBottom: String
  /// Gradient stops as fractions of each line's text box (ascent + descent), from its top.
  public let topLocation: Double
  public let bottomLocation: Double
  /// Motion writes linear light with this pure power law into its BT.709-tagged output.
  public let outputGamma: Double

  public init(
    fontName: String, fontSize: Double, faceGrow: Double, keyline: Double, whiteBorder: Double,
    rim: Double, shadowOpacity: Double, shadowOffsetY: Double, shadowSigma: Double,
    gradientTop: String, gradientBottom: String, topLocation: Double, bottomLocation: Double,
    outputGamma: Double
  ) {
    self.fontName = fontName
    self.fontSize = fontSize
    self.faceGrow = faceGrow
    self.keyline = keyline
    self.whiteBorder = whiteBorder
    self.rim = rim
    self.shadowOpacity = shadowOpacity
    self.shadowOffsetY = shadowOffsetY
    self.shadowSigma = shadowSigma
    self.gradientTop = gradientTop
    self.gradientBottom = gradientBottom
    self.topLocation = topLocation
    self.bottomLocation = bottomLocation
    self.outputGamma = outputGamma
  }

  /// Throws `CaptionRenderError.invalid` unless the lengths are finite and non-negative, the
  /// outlines nest (face ≤ keyline ≤ white band ≤ rim), the stops are ordered fractions and
  /// both colours are `#RRGGBB`.
  public func validate() throws {
    let lengths = [
      fontSize, faceGrow, keyline, whiteBorder, rim, shadowOffsetY, shadowSigma, outputGamma,
    ]
    guard lengths.allSatisfy({ $0.isFinite && $0 >= 0 }), fontSize > 0, outputGamma > 0,
      faceGrow <= keyline, keyline <= whiteBorder, whiteBorder <= rim,
      (0...1).contains(shadowOpacity),
      (0...1).contains(topLocation), (topLocation...1).contains(bottomLocation),
      CaptionRGB(hex: gradientTop) != nil, CaptionRGB(hex: gradientBottom) != nil
    else { throw CaptionRenderError.invalid("Invalid Motion caption style") }
  }

  /// This style with the Viewer's fill colours; validated like any style.
  public func applying(_ fill: CaptionFill) throws -> MotionCaptionStyle {
    let style = MotionCaptionStyle(
      fontName: fontName, fontSize: fontSize, faceGrow: faceGrow, keyline: keyline,
      whiteBorder: whiteBorder, rim: rim, shadowOpacity: shadowOpacity,
      shadowOffsetY: shadowOffsetY, shadowSigma: shadowSigma, gradientTop: fill.top,
      // A gradient between two equal stops is the single fill, drawn by the same code path.
      gradientBottom: fill.gradient ? fill.bottom : fill.top, topLocation: topLocation,
      bottomLocation: bottomLocation, outputGamma: outputGamma)
    try style.validate()
    return style
  }
}

/// The Viewer's choice of caption face colours (sRGB intent, `#RRGGBB`): the top and bottom
/// gradient stops, or, with `gradient` off, a single fill in the **top** colour (`bottom` is
/// kept but unused, so switching the gradient back on restores it).
public struct CaptionFill: Codable, Equatable, Sendable {
  public let top: String
  public let bottom: String
  public let gradient: Bool

  public init(top: String, bottom: String, gradient: Bool) {
    self.top = top
    self.bottom = bottom
    self.gradient = gradient
  }
}

/// Motion's paragraph alignment of a title's lines.
public enum MotionCaptionAlignment: String, Codable, Equatable, Sendable {
  /// Every line starts at the title's position.
  case left
}

/// One revealing title: its final text, frames [startFrame, endFrame) and the frame each
/// grapheme (line breaks included) appears on, as `replay.motion_reveal_stages` records them.
/// The position is Motion's: the first line's baseline centre (its start when left-aligned),
/// origin at the canvas centre, y up.
public struct MotionCaptionTitle: Codable, Equatable, Sendable {
  public let text: String
  public let startFrame: Int
  public let endFrame: Int
  public let x: Double
  public let y: Double
  public let graphemeFrames: [Int]
  /// This title's own style when it differs from the part's (a title edited in Motion).
  public let style: MotionCaptionStyle?
  /// Left-aligned lines; nil centres each line on the position (the template's default).
  public let alignment: MotionCaptionAlignment?
  /// Who speaks the title (caption plans only); never affects drawing.
  public let speaker: String?

  public init(
    text: String, startFrame: Int, endFrame: Int, x: Double, y: Double, graphemeFrames: [Int],
    style: MotionCaptionStyle? = nil, alignment: MotionCaptionAlignment? = nil,
    speaker: String? = nil
  ) {
    self.text = text
    self.startFrame = startFrame
    self.endFrame = endFrame
    self.x = x
    self.y = y
    self.graphemeFrames = graphemeFrames
    self.style = style
    self.alignment = alignment
    self.speaker = speaker
  }

  /// This title at another position (Motion's: first baseline, canvas centre origin, y up),
  /// for a placement that overrides the plan's.
  public func moved(x: Double, y: Double) -> MotionCaptionTitle {
    MotionCaptionTitle(
      text: text, startFrame: startFrame, endFrame: endFrame, x: x, y: y,
      graphemeFrames: graphemeFrames, style: style, alignment: alignment, speaker: speaker)
  }

  /// Graphemes shown on `frame`: Type On End shows each from its onset frame onward.
  public func visible(at frame: Int) -> Int {
    guard (startFrame..<endFrame).contains(frame) else { return 0 }
    return graphemeFrames.count { $0 <= frame }
  }

  /// Throws `CaptionRenderError.invalid` unless the text is non-empty, the frames are ordered
  /// within the title and there is one onset per grapheme.
  public func validate() throws {
    guard !text.isEmpty, startFrame >= 0, startFrame < endFrame,
      graphemeFrames.count == text.count,
      graphemeFrames.allSatisfy({ (startFrame..<endFrame).contains($0) }),
      zip(graphemeFrames, graphemeFrames.dropFirst()).allSatisfy({ $0 <= $1 }),
      x.isFinite, y.isFinite
    else { throw CaptionRenderError.invalid("Invalid Motion caption title") }
  }
}
