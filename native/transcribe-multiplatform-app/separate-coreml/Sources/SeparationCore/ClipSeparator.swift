import Foundation

/// A separation model with a fixed input length.
public protocol WindowModel {
  /// How many samples `tracks(of:)` takes.
  var windowSamples: Int { get }
  /// Two tracks of `windowSamples` each, for exactly `windowSamples` samples.
  func tracks(of window: [Float]) throws -> [[Float]]
}

/// Cuts a clip into windows, pads the last with zeros, and joins the tracks back.
///
/// Windows overlap because a model's track order is arbitrary: the shared second tells which new track
/// continues which, and is cross-faded so the join does not click.
struct ClipSeparator {
  private static let trackCount = 2
  private static let overlapSamples = 16_000
  private static let silenceFloor: Float = 1e-12

  let model: any WindowModel

  func separate(_ wave: [Float]) throws -> [[Float]] {
    guard !wave.isEmpty else { throw SeparationError.invalidWave("the clip has no samples") }
    let size = model.windowSamples
    guard size > Self.overlapSamples else {
      throw SeparationError.invalidModel("a window of \(size) samples is too short")
    }
    var end = min(size, wave.count)
    var joined = try separateWindow(Array(wave[0..<end]))
    while end < wave.count {
      let start = end - Self.overlapSamples
      end = min(start + size, wave.count)
      let tracks = try separateWindow(Array(wave[start..<end]))
      joined = Self.join(joined, inOrder(tracks, continuing: joined))
    }
    return joined
  }

  /// Silence skips the model: there is nothing to separate, and no level to restore.
  private func separateWindow(_ window: [Float]) throws -> [[Float]] {
    let level = Self.rms(window)
    if level == 0 {
      return Array(repeating: Array(repeating: 0, count: window.count), count: Self.trackCount)
    }
    let padding = Array(repeating: Float(0), count: model.windowSamples - window.count)
    let tracks = try model.tracks(of: window + padding)
    guard tracks.count == Self.trackCount, tracks.allSatisfy({ $0.count == model.windowSamples }) else {
      throw SeparationError.invalidModel("expected two tracks of \(model.windowSamples) samples")
    }
    guard tracks.allSatisfy({ $0.allSatisfy(\.isFinite) }) else {
      throw SeparationError.invalidModel("the tracks are not finite numbers")
    }
    return tracks.map { Self.atLevel(Array($0.prefix(window.count)), level: level) }
  }

  /// `tracks` swapped when that matches the end of `joined` better.
  private func inOrder(_ tracks: [[Float]], continuing joined: [[Float]]) -> [[Float]] {
    let tails = joined.map { $0.suffix(Self.overlapSamples) }
    let heads = tracks.map { $0.prefix(Self.overlapSamples) }
    let straight = Self.dot(tails[0], heads[0]) + Self.dot(tails[1], heads[1])
    let swapped = Self.dot(tails[0], heads[1]) + Self.dot(tails[1], heads[0])
    return swapped > straight ? [tracks[1], tracks[0]] : tracks
  }

  private static func join(_ joined: [[Float]], _ tracks: [[Float]]) -> [[Float]] {
    zip(joined, tracks).map { old, new in
      let kept = old.count - overlapSamples
      let faded = (0..<overlapSamples).map { index -> Float in
        let weight = Float(index + 1) / Float(overlapSamples + 1)
        return old[kept + index] * (1 - weight) + new[index] * weight
      }
      return old.prefix(kept) + faded + new.dropFirst(overlapSamples)
    }
  }

  private static func dot(_ first: ArraySlice<Float>, _ second: ArraySlice<Float>) -> Float {
    zip(first, second).reduce(0) { $0 + $1.0 * $1.1 }
  }

  private static func rms(_ samples: [Float]) -> Float {
    (dot(ArraySlice(samples), ArraySlice(samples)) / Float(samples.count)).squareRoot()
  }

  /// At the window's level, which the model leaves to its caller.
  private static func atLevel(_ track: [Float], level: Float) -> [Float] {
    let gain = level / (rms(track) + silenceFloor)
    return track.map { $0 * gain }
  }
}
