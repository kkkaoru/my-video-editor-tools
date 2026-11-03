import CoreGraphics
import CoreVideo
import Foundation

/// A whole canvas frame of captions, composited exactly as `MotionCaptionMovie` fills each
/// ProRes frame: cleared, then each bitmap laid over it in order with straight alpha.
public enum CaptionFrame {
  /// Straight-alpha RGBA8 of the canvas with `bitmaps` composited in order (later on top).
  public static func composite(_ bitmaps: [MotionCaptionBitmap], width: Int, height: Int)
    -> [UInt8]
  {
    var pixels = [UInt8](repeating: 0, count: width * height * 4)
    for bitmap in bitmaps {
      for row in 0..<bitmap.height {
        let y = bitmap.originY + row
        guard (0..<height).contains(y) else { continue }
        for column in 0..<bitmap.width {
          let x = bitmap.originX + column
          let source = (row * bitmap.width + column) * 4
          let alpha = bitmap.rgba[source + 3]
          guard (0..<width).contains(x), alpha > 0 else { continue }
          let target = (y * width + x) * 4
          guard pixels[target + 3] == 0 else {
            over(bitmap.rgba, at: source, into: &pixels, at: target)
            continue
          }
          pixels[target..<(target + 4)] = bitmap.rgba[source..<(source + 4)]
        }
      }
    }
    return pixels
  }

  /// The composited canvas as an image (see `MotionCaptionBitmap.image()` for its colours).
  public static func image(_ bitmaps: [MotionCaptionBitmap], width: Int, height: Int) throws
    -> CGImage
  {
    try MotionCaptionBitmap.image(
      composite(bitmaps, width: width, height: height), width: width, height: height)
  }

  /// Straight-alpha RGBA "over" straight-alpha RGBA, rounded as the movie writer rounds.
  static func over(_ rgba: [UInt8], at source: Int, into pixels: inout [UInt8], at target: Int) {
    let top = Float(rgba[source + 3]) / 255
    let under = Float(pixels[target + 3]) / 255 * (1 - top)
    let alpha = top + under
    for channel in 0..<3 {
      pixels[target + channel] = mixed(
        rgba[source + channel], top, pixels[target + channel], under, alpha)
    }
    pixels[target + 3] = UInt8(min(alpha, 1) * 255 + 0.5)
  }

  static func mixed(_ source: UInt8, _ top: Float, _ target: UInt8, _ under: Float, _ alpha: Float)
    -> UInt8
  {
    let value = (Float(source) * top + Float(target) * under) / alpha
    return UInt8(min(max(value, 0), 255) + 0.5)
  }

  /// Lays a straight-alpha RGBA canvas (`composite`'s) over an opaque 32BGRA video frame of the
  /// same size, with the same "over" and rounding: what the burned path's alpha caption movie
  /// does to the video's code values.
  public static func burn(_ canvas: [UInt8], into frame: CVPixelBuffer) throws {
    try withPixels(of: frame, matching: canvas) { pixels, rowBytes, width, height in
      for y in 0..<height {
        for x in 0..<width {
          let source = (y * width + x) * 4
          let alpha = canvas[source + 3]
          guard alpha > 0 else { continue }
          let target = y * rowBytes + x * 4
          let top = Float(alpha) / 255
          let under = 1 - top  // the video is opaque
          let total = top + under
          for channel in 0..<3 {
            pixels[target + 2 - channel] = mixed(
              canvas[source + channel], top, pixels[target + 2 - channel], under, total)
          }
        }
      }
    }
  }

  /// Writes a straight-alpha RGBA canvas (`composite`'s) as every pixel of a 32BGRA frame of
  /// the same size: the caption alpha movie's frame (`MotionCaptionMovie`).
  public static func store(_ canvas: [UInt8], into frame: CVPixelBuffer) throws {
    try withPixels(of: frame, matching: canvas) { pixels, rowBytes, width, height in
      for y in 0..<height {
        for x in 0..<width {
          let source = (y * width + x) * 4
          let target = y * rowBytes + x * 4
          pixels[target] = canvas[source + 2]
          pixels[target + 1] = canvas[source + 1]
          pixels[target + 2] = canvas[source]
          pixels[target + 3] = canvas[source + 3]
        }
      }
    }
  }

  /// Runs `body` on the locked bytes of a 32BGRA frame the size of `canvas`.
  static func withPixels(
    of frame: CVPixelBuffer, matching canvas: [UInt8],
    _ body: (UnsafeMutablePointer<UInt8>, Int, Int, Int) -> Void
  ) throws {
    let width = CVPixelBufferGetWidth(frame)
    let height = CVPixelBufferGetHeight(frame)
    guard CVPixelBufferGetPixelFormatType(frame) == kCVPixelFormatType_32BGRA,
      canvas.count == width * height * 4
    else { throw CaptionRenderError.invalid("A caption canvas must match its 32BGRA frame") }
    guard CVPixelBufferLockBaseAddress(frame, []) == kCVReturnSuccess else {
      throw CaptionRenderError.unavailable("Cannot lock a video frame")
    }
    defer { CVPixelBufferUnlockBaseAddress(frame, []) }
    guard let base = CVPixelBufferGetBaseAddress(frame) else {
      throw CaptionRenderError.unavailable("Cannot address a video frame")
    }
    let rowBytes = CVPixelBufferGetBytesPerRow(frame)
    // The locked buffer holds `rowBytes * height` bytes until the deferred unlock; the pointer
    // does not escape `body`.
    body(base.bindMemory(to: UInt8.self, capacity: rowBytes * height), rowBytes, width, height)
  }
}
