import AVFoundation
import Dispatch
import Foundation

/// Full-frame samples retain their original rational presentation clocks.
public struct OCRFrameSessionRequest: Sendable {
  public let path: String
  public let samples: [ForegroundTime]
  public let width: Int
  public let height: Int
  public let maximumSeconds: Double
  public let computeDevice: TextComputeDevice

  public init(
    path: String, samples: [ForegroundTime], width: Int, height: Int,
    maximumSeconds: Double, computeDevice: TextComputeDevice = .cpu
  ) {
    self.path = path
    self.samples = samples
    self.width = width
    self.height = height
    self.maximumSeconds = maximumSeconds
    self.computeDevice = computeDevice
  }

  func validate() throws {
    guard (1...OCRFrameSession.maximumSamples).contains(samples.count),
      (1...OCRFrameSession.maximumCanvasDimension).contains(width),
      (1...OCRFrameSession.maximumCanvasDimension).contains(height),
      width * height <= OCRFrameSession.maximumPixels,
      maximumSeconds.isFinite, maximumSeconds > 0,
      maximumSeconds <= OCRFrameSession.maximumSeconds
    else { throw ProAppsError.invalid("Invalid ordered OCR session budget") }
    let times = try samples.map { try $0.time() }
    guard zip(times, times.dropFirst()).allSatisfy({ $0.0 < $0.1 }) else {
      throw ProAppsError.invalid("Ordered OCR samples must be strictly increasing")
    }
  }
}

public struct OCRFrameSessionResult: Sendable {
  public let measurements: [FrameTextMeasurement]
  public let actual: [ForegroundTime]
  let frames: [SourceTextSessionFrame]
  let setup: SourceTextSessionTiming
}

protocol OCRFrameSessionWorker: Actor {
  func process(
    _ request: OCRFrameSessionRequest, indices: Range<Int>, sink: OCRFrameSessionSink
  ) async throws
}

/// Independent native contexts stay local to one actor call, including across awaits.
actor NativeOCRFrameSessionWorker: OCRFrameSessionWorker {
  nonisolated private let executor = DispatchSerialQueue(label: "apple-pro-apps.ordered-ocr")
  nonisolated var unownedExecutor: UnownedSerialExecutor { executor.asUnownedSerialExecutor() }

  func process(
    _ request: OCRFrameSessionRequest, indices: Range<Int>, sink: OCRFrameSessionSink
  ) async throws {
    try await sink.check()
    let clock = ContinuousClock()
    let assetStart = clock.now
    let generator = try await FrameProbe().sessionSource(
      path: request.path, samples: indices.map { request.samples[$0] })
    defer { generator.cancelAllCGImageGeneration() }
    var setup = SourceTextSessionTiming()
    setup.assetSetupSeconds = SourceTextSession.seconds(assetStart.duration(to: clock.now))
    let recognitionStart = clock.now
    let recognition = try TextRecognition.request(request.computeDevice)
    setup.ocrSetupSeconds = SourceTextSession.seconds(recognitionStart.duration(to: clock.now))
    setup.computeDevices = TextRecognition.devices(of: recognition)
    try await sink.prepared(setup)
    for index in indices {
      try await sink.check()
      let sample = request.samples[index]
      let requested = try sample.time()
      let start = clock.now
      let image = try await generator.image(at: requested)
      let actual = try ForegroundTime.actual(image.actualTime, requested: requested)
      let frame = try await NativeSourceTextSessionWorker.textFrame(
        image: image.image, sample: sample, actual: actual,
        imageGenerationSeconds: SourceTextSession.seconds(start.duration(to: clock.now)),
        recognition: recognition)
      try await sink.receive(frame, index: index)
    }
  }
}

/// At most `maximumWorkers` structured children, each over one contiguous slice of the samples;
/// failure/cancellation never returns partial success.
public enum OCRFrameSession {
  static let maximumWorkers = 8
  static let maximumSamples = SourceTextSession.maximumSamples
  static let maximumCanvasDimension = 16384
  static let maximumPixels = 16_777_216
  static let maximumSeconds = 60.0
  static let maximumTextBytes = 32768

  public static func run(_ request: OCRFrameSessionRequest) async throws -> OCRFrameSessionResult {
    try await run(request, workers: 2, factory: { NativeOCRFrameSessionWorker() })
  }

  /// Contiguous, ordered slices of `count` samples for up to `workers` children.
  static func ranges(count: Int, workers: Int) -> [Range<Int>] {
    let parts = max(1, min(workers, count))
    // Ceiling starts: the first slices take the remainder, as the former two-way split did.
    let start = { (part: Int) in (part * count + parts - 1) / parts }
    return (0..<parts).map { start($0)..<start($0 + 1) }
  }

  static func run(
    _ request: OCRFrameSessionRequest,
    workers: Int = 2,
    factory: @Sendable () -> any OCRFrameSessionWorker,
    beforeFrame: @escaping @Sendable () async throws -> Void = {}
  ) async throws -> OCRFrameSessionResult {
    try Task.checkCancellation()
    try request.validate()
    let sink = OCRFrameSessionSink(request, beforeFrame: beforeFrame)
    guard (1...maximumWorkers).contains(workers) else {
      throw ProAppsError.invalid("Invalid OCR worker count")
    }
    try await withThrowingTaskGroup(of: Void.self) { group in
      for indices in ranges(count: request.samples.count, workers: workers) {
        let worker = factory()
        group.addTask { try await worker.process(request, indices: indices, sink: sink) }
      }
      while try await group.next() != nil {}
    }
    return try await sink.finish()
  }
}

/// A bounded serial assembler preserves the request order, not completion order.
actor OCRFrameSessionSink {
  private let request: OCRFrameSessionRequest
  private let start: ContinuousClock.Instant
  private var frames: [Int: SourceTextSessionFrame] = [:]
  private var textBytes = 0
  private var setup = SourceTextSessionTiming()
  private let beforeFrame: @Sendable () async throws -> Void

  init(
    _ request: OCRFrameSessionRequest, start: ContinuousClock.Instant = ContinuousClock().now,
    beforeFrame: @escaping @Sendable () async throws -> Void = {}
  ) {
    self.request = request
    self.start = start
    self.beforeFrame = beforeFrame
  }

  func check() async throws {
    try await beforeFrame()
    try SourceTextSession.deadline(
      start: start, now: ContinuousClock().now, maximumSeconds: request.maximumSeconds)
  }

  func prepared(_ timing: SourceTextSessionTiming) async throws {
    try await check()
    guard timing.assetSetupSeconds.isFinite, timing.assetSetupSeconds >= 0,
      timing.ocrSetupSeconds.isFinite, timing.ocrSetupSeconds >= 0
    else { throw ProAppsError.invalid("Invalid ordered OCR setup timing") }
    setup.assetSetupSeconds += timing.assetSetupSeconds
    setup.ocrSetupSeconds += timing.ocrSetupSeconds
    setup.computeDevices = timing.computeDevices
  }

  func receive(_ frame: SourceTextSessionFrame, index: Int) async throws {
    try await check()
    guard request.samples.indices.contains(index), frames[index] == nil else {
      throw ProAppsError.invalid("Duplicate or out-of-range OCR frame")
    }
    try SourceTextSession.validate(
      frame, requested: request.samples[index], width: request.width, height: request.height)
    textBytes += frame.measurement.lines.reduce(0) { $0 + $1.text.utf8.count }
    guard textBytes <= OCRFrameSession.maximumTextBytes else { throw ProAppsError.outputLimit }
    frames[index] = frame
  }

  func finish() async throws -> OCRFrameSessionResult {
    try await check()
    guard frames.count == request.samples.count else {
      throw ProAppsError.invalid("Ordered OCR session is incomplete")
    }
    let ordered = try request.samples.indices.map { index in
      guard let frame = frames[index] else {
        throw ProAppsError.invalid("Ordered OCR frame is missing")
      }
      return frame
    }
    return OCRFrameSessionResult(
      measurements: ordered.map(\.measurement), actual: ordered.map(\.actual),
      frames: ordered, setup: setup)
  }
}
