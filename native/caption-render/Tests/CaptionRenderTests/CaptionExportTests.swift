import AVFoundation
import CoreVideo
import Foundation
import Synchronization
import Testing

@testable import CaptionRender

/// Synthetic movies and plans under the checkout's ignored `edits/work` (never the OS temp).
enum Synthetic {
  static let root = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appendingPathComponent("edits/work/caption-render/test-output")
  static let grey: UInt8 = 128

  static func directory() throws -> URL {
    let url = root.appendingPathComponent(UUID().uuidString)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
  }

  static func remove(_ url: URL) {
    do {
      try FileManager.default.removeItem(at: url)
    } catch {
      Issue.record(error)
    }
  }

  static func plan(
    width: Int = 320, burned: Bool = false, nameplate: String = "null"
  ) -> Data {
    Data(
      """
      {"version": 1, "clock": "delivery", "movie": "movie.mp4", "burned": \(burned),
       "width": \(width), "height": 240, "rate": 30, "frames": 30,
       "style": {"fontName": "HiraginoSans-W6", "fontSize": 60, "faceGrow": 0.3,
         "keyline": 1.4, "whiteBorder": 6, "rim": 8, "shadowOpacity": 0.7, "shadowOffsetY": 6,
         "shadowSigma": 2, "gradientTop": "#97D8F7", "gradientBottom": "#2FB0EE",
         "topLocation": 0.27, "bottomLocation": 0.39, "outputGamma": 1.961},
       "titles": [{"text": "口", "startFrame": 5, "endFrame": 25, "x": 0, "y": -20,
         "graphemeFrames": [5]}],
       "nameplate": \(nameplate)}
      """.utf8)
  }

  /// A one-second 320x240 30 fps mid-grey H.264 movie, with a silent AAC track when `audio`.
  static func movie(
    in directory: URL, audio: Bool = true, frameDuration: CMTime = CMTime(value: 1, timescale: 30)
  ) async throws -> URL {
    let video = directory.appendingPathComponent(audio ? "video-only.mp4" : "movie.mp4")
    let writer = try AVAssetWriter(outputURL: video, fileType: .mp4)
    let input = AVAssetWriterInput(
      mediaType: .video,
      outputSettings: [
        AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: 320, AVVideoHeightKey: 240,
      ])
    let adaptor = AVAssetWriterInputPixelBufferAdaptor(
      assetWriterInput: input,
      sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: 320, kCVPixelBufferHeightKey as String: 240,
      ])
    writer.add(input)
    #expect(writer.startWriting())
    writer.startSession(atSourceTime: .zero)
    for frame in 0..<30 {
      while !input.isReadyForMoreMediaData { try await Task.sleep(for: .milliseconds(2)) }
      let pool = try #require(adaptor.pixelBufferPool)
      var allocated: CVPixelBuffer?
      CVPixelBufferPoolCreatePixelBuffer(nil, pool, &allocated)
      let buffer = try #require(allocated)
      try fill(buffer, grey)
      #expect(
        adaptor.append(
          buffer, withPresentationTime: CMTimeMultiply(frameDuration, multiplier: Int32(frame))))
    }
    input.markAsFinished()
    await writer.finishWriting()
    #expect(writer.status == .completed)
    guard audio else { return video }
    let sound = directory.appendingPathComponent("silence.m4a")
    try silence(sound)
    // Remux both streams untouched (an export session refuses this pair with -12780).
    let movie = directory.appendingPathComponent("movie.mp4")
    let muxer = try AVAssetWriter(outputURL: movie, fileType: .mp4)
    var pairs: [(AVAssetReaderTrackOutput, AVAssetWriterInput, AVAssetReader)] = []
    for (url, type) in [(video, AVMediaType.video), (sound, AVMediaType.audio)] {
      let asset = AVURLAsset(url: url)
      let track = try #require(try await asset.loadTracks(withMediaType: type).first)
      let hint = try #require(try await track.load(.formatDescriptions).first)
      let reader = try AVAssetReader(asset: asset)
      // Keep the movie one second long, as its video is.
      reader.timeRange = CMTimeRange(start: .zero, duration: CMTime(value: 1, timescale: 1))
      let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
      reader.add(output)
      let input = AVAssetWriterInput(mediaType: type, outputSettings: nil, sourceFormatHint: hint)
      muxer.add(input)
      #expect(reader.startReading())
      pairs.append((output, input, reader))
    }
    #expect(muxer.startWriting())
    muxer.startSession(atSourceTime: .zero)
    var open = Set(pairs.indices)
    while !open.isEmpty {
      var moved = false
      for index in open where pairs[index].1.isReadyForMoreMediaData {
        moved = true
        if let sample = pairs[index].0.copyNextSampleBuffer() {
          #expect(pairs[index].1.append(sample))
        } else {
          pairs[index].1.markAsFinished()
          open.remove(index)
        }
      }
      if !moved { try await Task.sleep(for: .milliseconds(2)) }
    }
    await muxer.finishWriting()
    #expect(muxer.status == .completed)
    return movie
  }

  /// One second of stereo AAC silence; the file is closed when this returns.
  static func silence(_ url: URL) throws {
    let format = try #require(AVAudioFormat(standardFormatWithSampleRate: 48_000, channels: 2))
    let file = try AVAudioFile(
      forWriting: url,
      settings: [
        AVFormatIDKey: kAudioFormatMPEG4AAC, AVSampleRateKey: 48_000, AVNumberOfChannelsKey: 2,
      ])
    let buffer = try #require(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 48_000))
    buffer.frameLength = 48_000
    try file.write(from: buffer)
  }

  /// Sets every pixel of a 32BGRA buffer to an opaque grey.
  static func fill(_ buffer: CVPixelBuffer, _ value: UInt8) throws {
    CVPixelBufferLockBaseAddress(buffer, [])
    defer { CVPixelBufferUnlockBaseAddress(buffer, []) }
    let base = try #require(CVPixelBufferGetBaseAddress(buffer))
    let count = CVPixelBufferGetBytesPerRow(buffer) * CVPixelBufferGetHeight(buffer)
    let pixels = base.bindMemory(to: UInt8.self, capacity: count)
    for index in 0..<count { pixels[index] = index % 4 == 3 ? 255 : value }
  }

  /// BGRA bytes of every decoded frame of `movie`.
  static func frames(_ movie: URL) async throws -> [[UInt8]] {
    let asset = AVURLAsset(url: movie)
    let track = try #require(try await asset.loadTracks(withMediaType: .video).first)
    let reader = try AVAssetReader(asset: asset)
    let output = AVAssetReaderTrackOutput(
      track: track,
      outputSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
    reader.add(output)
    #expect(reader.startReading())
    var result: [[UInt8]] = []
    while let sample = output.copyNextSampleBuffer() {
      let buffer = try #require(CMSampleBufferGetImageBuffer(sample))
      CVPixelBufferLockBaseAddress(buffer, .readOnly)
      defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
      let rowBytes = CVPixelBufferGetBytesPerRow(buffer)
      let height = CVPixelBufferGetHeight(buffer)
      let width = CVPixelBufferGetWidth(buffer)
      let base = try #require(CVPixelBufferGetBaseAddress(buffer))
      let pixels = base.bindMemory(to: UInt8.self, capacity: rowBytes * height)
      var frame: [UInt8] = []
      for y in 0..<height {
        for x in 0..<(width * 4) { frame.append(pixels[y * rowBytes + x]) }
      }
      result.append(frame)
    }
    return result
  }

  /// Pixels clearly off the grey background.
  static func marked(_ frame: [UInt8]) -> Int {
    stride(from: 0, to: frame.count, by: 4).count { abs(Int(frame[$0 + 1]) - Int(grey)) > 40 }
  }
}

struct CaptionExportTests {
  @Test func burnsTheWholeMovieWithItsAudio() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory)
    let plan = try CaptionPlan.decode(Synthetic.plan())
    let output = directory.appendingPathComponent("burned.mp4")
    let report = try await CaptionExport.export(
      movie: movie, plan: plan, ranges: nil, output: output)
    #expect(report.frames == 30 && report.duration == 1 && report.output == output.path)
    #expect(
      report.ranges == [
        CaptionExportSegment(start: 0, end: 1, startFrame: 0, endFrame: 30, outputFrame: 0)
      ])
    let asset = AVURLAsset(url: output)
    #expect(try await asset.loadTracks(withMediaType: .audio).count == 1)
    let frames = try await Synthetic.frames(output)
    #expect(frames.count == 30)
    #expect(Synthetic.marked(frames[2]) == 0)
    #expect(Synthetic.marked(frames[10]) > 500)
    #expect(Synthetic.marked(frames[27]) == 0)
    let left = try FileManager.default.contentsOfDirectory(atPath: directory.path)
    #expect(!left.contains { $0.contains(".partial-") })
    await #expect(throws: CaptionRenderError.invalid("Refusing to overwrite \(output.path)")) {
      try await CaptionExport.export(movie: movie, plan: plan, ranges: nil, output: output)
    }
  }

  @Test func joinsRangesInTheGivenOrderWithEachFramesOwnCaptions() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory, audio: false)
    let plan = try CaptionPlan.decode(Synthetic.plan())
    let output = directory.appendingPathComponent("ranges.mp4")
    let progress = Counter()
    let report = try await CaptionExport.export(
      movie: movie, plan: plan,
      // Later first, then earlier, then overlapping the first.
      ranges: [
        CaptionExportRange(start: 0.5, end: 0.7), CaptionExportRange(start: 0, end: 0.1),
        CaptionExportRange(start: 0.6, end: 0.7),
      ],
      fill: CaptionFill(top: "#FF0000", bottom: "#00FF00", gradient: false), output: output,
      codec: .hevc, progress: { done, total in progress.add(done, total) })
    #expect(report.frames == 12 && report.ranges.map(\.outputFrame) == [0, 6, 9])
    let last = try #require(progress.last)
    #expect(last == (12, 12))
    #expect(try await AVURLAsset(url: output).loadTracks(withMediaType: .audio).isEmpty)
    let frames = try await Synthetic.frames(output)
    #expect(
      frames.map(Synthetic.marked).map { $0 > 0 } == Array(repeating: true, count: 6)
        + [false, false, false, true, true, true])
  }

  @Test func drawsTheNameplateUnderTheCaptions() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory, audio: false)
    let plan = try CaptionPlan.decode(Synthetic.plan())
    let plate = MotionCaptionBitmap(
      originX: 0, originY: 0, width: 320, height: 240,
      rgba: (0..<(320 * 240)).flatMap {
        $0 < 320 * 20 ? [255, 255, 255, 255] : [0, 0, 0, 0] as [UInt8]
      })
    let output = directory.appendingPathComponent("plate.mp4")
    _ = try await CaptionExport.export(
      movie: movie, plan: plan, ranges: [CaptionExportRange(start: 0, end: 0.1)], nameplate: plate,
      output: output)
    let first = try #require(try await Synthetic.frames(output).first)
    #expect(first[(5 * 320 + 100) * 4 + 1] > 240)
    let small = MotionCaptionBitmap(
      originX: 0, originY: 0, width: 1, height: 1, rgba: [0, 0, 0, 0])
    await #expect(throws: CaptionRenderError.invalid("The nameplate is not on the plan's canvas")) {
      try await CaptionExport.export(
        movie: movie, plan: plan, ranges: nil, nameplate: small,
        output: directory.appendingPathComponent("never.mp4"))
    }
  }

  @Test func refusesMoviesAndPlansThatDoNotMatch() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory, audio: false)
    let output = directory.appendingPathComponent("never.mp4")
    await #expect(
      throws: CaptionRenderError.invalid("The plan's movie already has its captions burned in")
    ) {
      try await CaptionExport.export(
        movie: movie, plan: try CaptionPlan.decode(Synthetic.plan(burned: true)), ranges: nil,
        output: output)
    }
    await #expect(throws: CaptionRenderError.self) {
      try await CaptionExport.export(
        movie: movie, plan: try CaptionPlan.decode(Synthetic.plan(width: 640)), ranges: nil,
        output: output)
    }
    await #expect(throws: CaptionRenderError.self) {
      try await CaptionExport.export(
        movie: directory.appendingPathComponent("missing.mp4"),
        plan: try CaptionPlan.decode(Synthetic.plan()), ranges: nil, output: output)
    }
    await #expect(throws: CaptionRenderError.self) {
      let text = directory.appendingPathComponent("plan.json")
      try Synthetic.plan().write(to: text)
      return try await CaptionExport.export(
        movie: text, plan: try CaptionPlan.decode(Synthetic.plan()), ranges: nil, output: output)
    }
    #expect(!FileManager.default.fileExists(atPath: output.path))
  }

  @Test func neverOverwritesAnOutputThatAppearsMeanwhile() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory, audio: false)
    let output = directory.appendingPathComponent("raced.mp4")
    let path = output.path
    await #expect(throws: CaptionRenderError.invalid("Refusing to overwrite \(path)")) {
      try await CaptionExport.export(
        movie: movie, plan: try CaptionPlan.decode(Synthetic.plan()),
        ranges: [CaptionExportRange(start: 0, end: 0.1)], output: output,
        progress: { done, _ in
          if done == 1 { FileManager.default.createFile(atPath: path, contents: Data("x".utf8)) }
        })
    }
    #expect(try Data(contentsOf: output) == Data("x".utf8))
    let left = try FileManager.default.contentsOfDirectory(atPath: directory.path)
    #expect(Set(left) == ["movie.mp4", "raced.mp4"])
  }

  @Test func refusesAMovieWithoutVideo() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let sound = directory.appendingPathComponent("sound.m4a")
    try Synthetic.silence(sound)
    await #expect(throws: CaptionRenderError.invalid("\(sound.path) has no video")) {
      try await CaptionExport.export(
        movie: sound, plan: try CaptionPlan.decode(Synthetic.plan()), ranges: nil,
        output: directory.appendingPathComponent("never.mp4"))
    }
  }

  @Test func namesTheCauseOfAFailure() {
    #expect(CaptionExport.failure("Broke", nil) == .unavailable("Broke"))
    #expect(
      CaptionExport.failure("Broke", CaptionRenderError.invalid("x"))
        == .unavailable("Broke: Invalid request: x"))
  }

  @Test func exportsRangesWithoutCaptionsWhenThereIsNoPlan() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory)
    let whole = try await CaptionExport.export(
      movie: movie, plan: nil, ranges: nil, output: directory.appendingPathComponent("plain.mp4"))
    #expect(whole.frames == 30 && whole.duration == 1)
    let plain = try await Synthetic.frames(directory.appendingPathComponent("plain.mp4"))
    #expect(plain.count == 30 && Synthetic.marked(plain[10]) == 0)
    #expect(
      try await AVURLAsset(url: directory.appendingPathComponent("plain.mp4"))
        .loadTracks(withMediaType: .audio).count == 1)
    let ranges = try await CaptionExport.export(
      movie: movie, plan: nil,
      ranges: [CaptionExportRange(start: 0.5, end: 0.6), CaptionExportRange(start: 0, end: 0.6)],
      output: directory.appendingPathComponent("ranges.mp4"))
    #expect(ranges.frames == 21 && ranges.ranges.map(\.startFrame) == [15, 0])
  }

  @Test func refusesAFractionalFrameRateWithoutAPlan() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(
      in: directory, audio: false, frameDuration: CMTime(value: 1001, timescale: 30000))
    await #expect(throws: CaptionRenderError.self) {
      try await CaptionExport.export(
        movie: movie, plan: nil, ranges: nil, output: directory.appendingPathComponent("x.mp4"))
    }
  }

  @Test func burnedPlansExportWhenAllowed() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory, audio: false)
    let report = try await CaptionExport.export(
      movie: movie, plan: try CaptionPlan.decode(Synthetic.plan(burned: true)),
      ranges: [CaptionExportRange(start: 0, end: 0.1)],
      output: directory.appendingPathComponent("again.mp4"), allowBurned: true)
    #expect(report.frames == 3)
  }

  @Test func cancellationLeavesNoFile() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory, audio: false)
    let plan = try CaptionPlan.decode(Synthetic.plan())
    let output = directory.appendingPathComponent("cancelled.mp4")
    let task = Task {
      try await CaptionExport.export(movie: movie, plan: plan, ranges: nil, output: output)
    }
    task.cancel()
    await #expect(throws: CancellationError.self) { try await task.value }
    let left = try FileManager.default.contentsOfDirectory(atPath: directory.path)
    #expect(Set(left) == ["movie.mp4"])
  }

  @Test func snapsRangesToFramesInOrder() throws {
    let plan = try CaptionPlan.decode(Synthetic.plan())
    let segments = try CaptionExport.segments(
      [CaptionExportRange(start: 0.1, end: 0.2), CaptionExportRange(start: 0.2, end: 1)],
      canvas: CaptionExport.Canvas(plan))
    #expect(segments.map(\.startFrame) == [3, 6] && segments.map(\.endFrame) == [6, 30])
    #expect(segments.map(\.outputFrame) == [0, 3])
    #expect(CaptionExport.deliveryFrame(0, in: segments) == 3)
    #expect(CaptionExport.deliveryFrame(3, in: segments) == 6)
    #expect(CaptionExport.deliveryFrame(26, in: segments) == 29)
    #expect(CaptionExport.deliveryFrame(27, in: segments) == nil)
    #expect(CaptionExport.deliveryFrame(-1, in: segments) == nil)
  }

  @Test func joinsRangesInAnyOrderAndOverlapping() throws {
    let plan = try CaptionPlan.decode(Synthetic.plan())
    let segments = try CaptionExport.segments(
      [
        CaptionExportRange(start: 0.5, end: 0.6), CaptionExportRange(start: 0.1, end: 0.2),
        CaptionExportRange(start: 0.5, end: 0.7),
      ], canvas: CaptionExport.Canvas(plan))
    #expect(segments.map(\.startFrame) == [15, 3, 15] && segments.map(\.endFrame) == [18, 6, 21])
    #expect(segments.map(\.outputFrame) == [0, 3, 6])
    #expect(CaptionExport.deliveryFrame(3, in: segments) == 3)
    #expect(CaptionExport.deliveryFrame(6, in: segments) == 15)
    #expect(CaptionExport.deliveryFrame(11, in: segments) == 20)
  }

  @Test(arguments: [
    [CaptionExportRange(start: 0.5, end: 0.5)],
    [CaptionExportRange(start: 0.5, end: 2)],
    [CaptionExportRange(start: -1, end: 0.5)],
    [CaptionExportRange(start: .nan, end: 0.5)],
    [],
  ])
  func refusesBadRanges(_ ranges: [CaptionExportRange]) throws {
    let plan = try CaptionPlan.decode(Synthetic.plan())
    #expect(throws: CaptionRenderError.self) {
      try CaptionExport.segments(ranges, canvas: CaptionExport.Canvas(plan))
    }
  }

  @Test func refusesOutputsItCannotStage() throws {
    #expect(throws: CaptionRenderError.invalid("The output must be an absolute .mp4 path")) {
      try CaptionExport.staging(for: URL(fileURLWithPath: "/synthetic/out.mov"))
    }
    #expect(throws: CaptionRenderError.invalid("No directory for /synthetic-missing/out.mp4")) {
      try CaptionExport.staging(for: URL(fileURLWithPath: "/synthetic-missing/out.mp4"))
    }
  }

  @Test func refusesAStagedMovieWithTheWrongFrames() async throws {
    let directory = try Synthetic.directory()
    defer { Synthetic.remove(directory) }
    let movie = try await Synthetic.movie(in: directory, audio: false)
    await #expect(
      throws: CaptionRenderError.unavailable("The exported movie holds 30 frames, not 31")
    ) {
      try await CaptionExport.verify(movie, frames: 31, audio: false)
    }
    await #expect(throws: CaptionRenderError.unavailable("The exported movie lacks its tracks")) {
      try await CaptionExport.verify(movie, frames: 30, audio: true)
    }
  }

  @Test func storesACanvasAsEveryPixelOfABGRAFrame() throws {
    var allocated: CVPixelBuffer?
    CVPixelBufferCreate(nil, 2, 1, kCVPixelFormatType_32BGRA, nil, &allocated)
    let buffer = try #require(allocated)
    try Synthetic.fill(buffer, 77)
    try CaptionFrame.store([1, 2, 3, 4, 0, 0, 0, 0], into: buffer)
    CVPixelBufferLockBaseAddress(buffer, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
    let base = try #require(CVPixelBufferGetBaseAddress(buffer))
    let pixels = base.bindMemory(to: UInt8.self, capacity: 8)
    #expect((0..<8).map { pixels[$0] } == [3, 2, 1, 4, 0, 0, 0, 0])
    #expect(throws: CaptionRenderError.self) { try CaptionFrame.store([0], into: buffer) }
  }

  @Test func burnsACanvasOverAnOpaqueFrameWithStraightAlpha() throws {
    var allocated: CVPixelBuffer?
    CVPixelBufferCreate(nil, 2, 1, kCVPixelFormatType_32BGRA, nil, &allocated)
    let buffer = try #require(allocated)
    try Synthetic.fill(buffer, 0)
    CVPixelBufferLockBaseAddress(buffer, [])
    let base = try #require(CVPixelBufferGetBaseAddress(buffer))
    let pixels = base.bindMemory(to: UInt8.self, capacity: 8)
    (pixels[0], pixels[1], pixels[2]) = (30, 20, 10)
    CVPixelBufferUnlockBaseAddress(buffer, [])
    try CaptionFrame.burn([200, 100, 0, 128, 9, 9, 9, 0], into: buffer)
    CVPixelBufferLockBaseAddress(buffer, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
    #expect((0..<8).map { pixels[$0] } == [15, 60, 105, 255, 0, 0, 0, 255])
    #expect(throws: CaptionRenderError.invalid("A caption canvas must match its 32BGRA frame")) {
      try CaptionFrame.burn([0, 0, 0, 0], into: buffer)
    }
  }
}

/// Progress calls, recorded across the exporter's calls.
final class Counter: Sendable {
  private let state = Mutex<(Int, Int)?>(nil)
  func add(_ done: Int, _ total: Int) { state.withLock { $0 = (done, total) } }
  var last: (Int, Int)? { state.withLock { $0 } }
}
