import CoreGraphics
import CoreText
import CryptoKit
import Foundation
import ImageIO

/// Draws a plan's nameplate as `replay.caption_layout.badge_image` does with Pillow: the icon
/// cut to a disc inside a ring, and the name centred below it with an outward stroke. The
/// geometry is the same; edge anti-aliasing and the icon's resampling (Pillow: LANCZOS at 4×,
/// then reduced) are CoreGraphics', so pixels differ slightly at edges.
public enum CaptionNameplateRenderer {
  static let weightAxis = 0x7767_6874  // 'wght'
  /// shortcut: only the `Bold` named instance, as OpenType weight 700 (what caption_layout uses
  /// and Noto Sans JP's Bold is); read the font's fvar instances if other names appear.
  static let namedWeights = ["Bold": 700.0]

  /// The full-canvas nameplate (premultiplied RGBA8 in `MotionCaptionBitmap.videoColorSpace`, as the burned badge
  /// movie is tagged with; colours and icon bytes are written as code values, untransformed,
  /// as Pillow writes them). `icon` and `font` are the files the plan names, refused unless
  /// their SHA-256 matches.
  public static func image(
    _ plate: CaptionNameplate, icon: Data, font: Data, canvasWidth: Int, canvasHeight: Int
  ) throws -> CGImage {
    try plate.validate()
    try requireDigest(icon, plate.iconSha256, plate.icon)
    try requireDigest(font, plate.nameFontSha256, plate.nameFont)
    let (context, space) = try canvas(width: canvasWidth, height: canvasHeight)
    try drawIcon(
      icon, named: plate.icon,
      disc: CGRect(x: plate.iconX, y: plate.iconY, width: plate.iconSize, height: plate.iconSize),
      ring: CGFloat(plate.ringWidth), ringColour: plate.ringColour, space: space, in: context)
    let name = try namePath(plate, font: font)
    let stroke = CGFloat(plate.nameStrokeWidth)
    let drawn = name.boundingBoxOfPath.insetBy(dx: -stroke, dy: -stroke)
    guard drawn.minX >= 0, drawn.maxX <= CGFloat(canvasWidth),
      drawn.maxY <= CGFloat(canvasHeight)
    else {
      throw CaptionRenderError.invalid("The caption nameplate's name does not fit its canvas")
    }
    context.addPath(name)
    context.setLineWidth(2 * stroke)
    context.setLineJoin(.round)
    context.setStrokeColor(try colour(plate.nameStrokeColour, space))
    context.strokePath()
    context.addPath(name)
    context.setFillColor(try colour(plate.nameColour, space))
    context.fillPath()
    guard let image = context.makeImage() else {
      throw CaptionRenderError.unavailable("Cannot read the nameplate canvas")
    }
    return image
  }

  /// The nameplate as a full-canvas straight-alpha bitmap, to composite under the captions
  /// (`CaptionFrame.composite`).
  public static func bitmap(
    _ plate: CaptionNameplate, icon: Data, font: Data, canvasWidth: Int, canvasHeight: Int
  ) throws -> MotionCaptionBitmap {
    try straight(
      image(plate, icon: icon, font: font, canvasWidth: canvasWidth, canvasHeight: canvasHeight),
      width: canvasWidth, height: canvasHeight)
  }

  /// A clear canvas context in top-left canvas coordinates (as the plan's), and its space.
  static func canvas(width: Int, height: Int) throws -> (CGContext, CGColorSpace) {
    guard let space = MotionCaptionBitmap.videoColorSpace,
      let context = CGContext(
        data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
        space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
    else { throw CaptionRenderError.unavailable("Cannot allocate a nameplate canvas") }
    context.translateBy(x: 0, y: CGFloat(height))
    context.scaleBy(x: 1, y: -1)
    context.interpolationQuality = .high
    return (context, space)
  }

  /// A full-canvas premultiplied image as a straight-alpha bitmap.
  static func straight(_ drawn: CGImage, width canvasWidth: Int, height canvasHeight: Int) throws
    -> MotionCaptionBitmap
  {
    guard let data = drawn.dataProvider?.data as Data?,
      data.count == canvasWidth * canvasHeight * 4
    else { throw CaptionRenderError.unavailable("Cannot read the nameplate canvas") }
    var rgba = [UInt8](data)
    for pixel in stride(from: 0, to: rgba.count, by: 4) {
      let alpha = Int(rgba[pixel + 3])
      guard alpha > 0, alpha < 255 else { continue }
      for channel in 0..<3 {
        rgba[pixel + channel] = UInt8(
          min(255, (Int(rgba[pixel + channel]) * 255 + alpha / 2) / alpha))
      }
    }
    return MotionCaptionBitmap(
      originX: 0, originY: 0, width: canvasWidth, height: canvasHeight, rgba: rgba)
  }

  static func digest(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
  }

  static func requireDigest(_ data: Data, _ expected: String, _ name: String) throws {
    guard digest(data) == expected else {
      throw CaptionRenderError.invalid("\(name) does not have its recorded SHA-256")
    }
  }

  static func colour(_ hex: String, _ space: CGColorSpace) throws -> CGColor {
    guard let rgb = CaptionRGB(hex: hex),
      let colour = CGColor(colorSpace: space, components: [rgb.red, rgb.green, rgb.blue, 1])
    else { throw CaptionRenderError.invalid("Invalid nameplate colour \(hex)") }
    return colour
  }

  /// A ring-coloured disc with the icon, scaled to fill it, inside the ring.
  static func drawIcon(
    _ icon: Data, named name: String, disc: CGRect, ring: CGFloat, ringColour: String,
    space: CGColorSpace, in context: CGContext
  ) throws {
    guard let source = CGImageSourceCreateWithData(icon as CFData, nil),
      let picture = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { throw CaptionRenderError.invalid("\(name) is not a readable image") }
    context.setFillColor(try colour(ringColour, space))
    context.fillEllipse(in: disc)
    context.saveGState()
    defer { context.restoreGState() }
    context.addEllipse(in: disc.insetBy(dx: ring, dy: ring))
    context.clip()
    // Pillow copies the icon's RGB bytes untransformed; a non-RGB icon is colour-converted.
    // shortcut: transparent icon pixels show the ring colour, where Pillow's convert("RGB")
    // shows their stored RGB; flatten the alpha if transparent icons appear.
    let untagged = picture.copy(colorSpace: space) ?? picture
    // Undo the canvas flip for the image, which CoreGraphics draws bottom-up.
    context.translateBy(x: 0, y: disc.maxY + disc.minY)
    context.scaleBy(x: 1, y: -1)
    context.draw(untagged, in: disc)
  }

  /// The name's outline: its advance centred on `nameX` and its ink top at `nameY` (Pillow's
  /// `mt` anchor: `t` is the top of the drawn text, not the ascender line).
  static func namePath(_ plate: CaptionNameplate, font data: Data) throws -> CGPath {
    guard let weight = namedWeights[plate.nameFontVariation] else {
      throw CaptionRenderError.invalid("Unsupported nameplate font variation")
    }
    guard
      let descriptor =
        (CTFontManagerCreateFontDescriptorsFromData(data as CFData)
        as? [CTFontDescriptor])?.first
    else { throw CaptionRenderError.invalid("\(plate.nameFont) is not a readable font") }
    let font = CTFontCreateWithFontDescriptor(
      CTFontDescriptorCreateCopyWithVariation(
        descriptor, weightAxis as CFNumber, CGFloat(weight)),
      CGFloat(plate.nameSize), nil)
    // CoreText drops a variation the font has no axis for.
    guard let applied = CTFontCopyVariation(font) as? [NSNumber: NSNumber],
      applied[NSNumber(value: weightAxis)]?.doubleValue == weight
    else { throw CaptionRenderError.invalid("\(plate.nameFont) has no weight axis") }
    let glyphs = try MotionCaptionRenderer.glyphs(
      plate.name, font: font, anchorX: plate.nameX, alignment: nil, baseline: 0,
      firstGrapheme: 0)
    let laid = CGMutablePath()
    for glyph in glyphs { laid.addPath(glyph.path) }
    var down = CGAffineTransform(translationX: 0, y: plate.nameY - laid.boundingBoxOfPath.minY)
    guard let path = laid.copy(using: &down) else {
      throw CaptionRenderError.unavailable("Cannot place the nameplate's name")
    }
    return path
  }
}
