import CoreGraphics
import CoreML
import Foundation
import Vision

/// Where Vision's text request runs; a named device is required, never replaced. `fastest`
/// takes, per compute stage, the Neural Engine, else the GPU (Metal), else the CPU. The default
/// is `cpu`: measured 2026-11-01 (61 tiled caption bands, 8 workers), all devices took the same
/// time and read identical text, but GPU and Neural Engine runs crashed (SIGSEGV) in 2 of 20 and
/// 1 of 15 sessions, CPU runs in none of 20.
public enum TextComputeDevice: String, Codable, Sendable {
  case fastest
  case cpu
  case gpu
  case neuralEngine
}

/// OCR preparation; no CoreImage context.
enum TextRecognition {
  static func kind(_ device: MLComputeDevice) -> TextComputeDevice {
    switch device {
    case .cpu: .cpu
    case .gpu: .gpu
    case .neuralEngine: .neuralEngine
    @unknown default: .fastest
    }
  }

  static func device(_ wanted: TextComputeDevice, in devices: [MLComputeDevice]) throws
    -> MLComputeDevice
  {
    let order: [TextComputeDevice] = wanted == .fastest ? [.neuralEngine, .gpu, .cpu] : [wanted]
    for preferred in order {
      if let device = devices.first(where: { kind($0) == preferred }) { return device }
    }
    throw ProAppsError.unavailable("OCR compute stage has no \(wanted.rawValue) device")
  }

  static func request(_ wanted: TextComputeDevice = .cpu) throws -> RecognizeTextRequest {
    var request = RecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.recognitionLanguages = [
      Locale.Language(identifier: "ja-JP"), Locale.Language(identifier: "en-US"),
    ]
    request.usesLanguageCorrection = false
    let stages = request.supportedComputeStageDevices
    guard !stages.isEmpty else {
      throw ProAppsError.unavailable("OCR exposes no configurable compute stages")
    }
    for (stage, devices) in stages {
      request.setComputeDevice(try device(wanted, in: devices), for: stage)
    }
    return request
  }

  /// The device each compute stage of `request` uses, as sorted "stage=device" names.
  static func devices(of request: RecognizeTextRequest) -> String {
    request.supportedComputeStageDevices.keys
      .map { stage in
        "\(stage)=\(request.computeDevice(for: stage).map { kind($0).rawValue } ?? "none")"
      }
      .sorted().joined(separator: ",")
  }

  /// Selection is in CoreImage bottom-left pixels; CGImage crops use top-left pixels.
  static func bitmap(_ image: CGImage, selected: CGRect) throws -> CGImage {
    let bounds = CGRect(x: 0, y: 0, width: image.width, height: image.height)
    guard !selected.isEmpty, bounds.contains(selected) else {
      throw ProAppsError.invalid("OCR crop exceeds the decoded image")
    }
    if selected == bounds { return image }
    let crop = CGRect(
      x: selected.minX, y: Double(image.height) - selected.maxY,
      width: selected.width, height: selected.height)
    guard let bitmap = image.cropping(to: crop) else {
      throw ProAppsError.unavailable("Cannot prepare the selected OCR frame")
    }
    return bitmap
  }
}
