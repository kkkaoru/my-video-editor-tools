import AVFoundation
import CoreVideo
import Darwin
import Foundation

/// A range of the delivery to export, `[start, end)` seconds on the delivery's clock. Ranges
/// are joined in the order given; they may be in any order and may overlap.
public struct CaptionExportRange: Codable, Equatable, Sendable {
  public let start: Double
  public let end: Double

  public init(start: Double, end: Double) {
    self.start = start
    self.end = end
  }
}

/// A range as exported: snapped to the delivery's frames `[startFrame, endFrame)` and placed
/// at `outputFrame` in the exported movie.
public struct CaptionExportSegment: Codable, Equatable, Sendable {
  public let start: Double
  public let end: Double
  public let startFrame: Int
  public let endFrame: Int
  public let outputFrame: Int
}

/// What `CaptionExport.export` wrote.
public struct CaptionExportReport: Codable, Equatable, Sendable {
  public let output: String
  public let frames: Int
  public let ranges: [CaptionExportSegment]
  /// Seconds of exported video.
  public let duration: Double
}

/// The exported movie's video codec.
public enum CaptionVideoCodec: String, Codable, Sendable {
  case h264
  case hevc
}

/// Burns a delivery's caption plan into (ranges of) the delivery movie, or exports the ranges
/// without captions when there is no plan: the ranges are joined in order, each frame gets the captions of its own delivery frame, composited with the burned
/// path's straight-alpha "over" (`CaptionFrame.burn`), and the original audio is re-encoded as
/// AAC. The movie is written beside `output` under a hidden staging name, checked (frame count,
/// tracks) and only then renamed onto `output`, which must not exist (never overwritten).
public enum CaptionExport {
  /// Bits per pixel per frame of the video's average bit rate (about 9 Mbit/s at 1080p30).
  static let bitsPerPixel = 0.15
  static let audioRate = 48_000
  static let audioChannels = 2
  static let audioBitRate = 256_000
  static let writerPollInterval = Duration.milliseconds(2)

  /// - Parameters:
  ///   - plan: the captions to burn; nil exports the ranges without captions or nameplate, on
  ///     the movie's own canvas and (whole-number) frame rate.
  ///   - layout: places the captions (nil keeps the plan's positions); `faces` is needed by
  ///     `aboveFace`.
  ///   - fill: replaces every caption's face colours (nil keeps the plan's).
  ///   - speakerFills: a speaker's own face colours, over `fill` for that speaker's titles.
  ///   - nameplate: drawn under the captions on every frame (`CaptionNameplateRenderer.bitmap`).
  ///   - allowBurned: export a plan whose movie already has these captions burned in.
  ///   - progress: called with (frames written, frames in total) after each frame.
  public static func export(
    movie: URL, plan: CaptionPlan?, ranges: [CaptionExportRange]?, layout: CaptionLayout? = nil,
    faces: CaptionFaces? = nil, fill: CaptionFill? = nil,
    speakerFills: [String: CaptionFill] = [:],
    nameplate: MotionCaptionBitmap? = nil, output: URL, codec: CaptionVideoCodec = .h264,
    allowBurned: Bool = false, progress: (@Sendable (Int, Int) -> Void)? = nil
  ) async throws -> CaptionExportReport {
    var captions: Captions?
    if let plan {
      try plan.validate()
      guard allowBurned || !plan.burned else {
        throw CaptionRenderError.invalid("The plan's movie already has its captions burned in")
      }
      if let nameplate {
        guard (nameplate.width, nameplate.height) == (plan.width, plan.height) else {
          throw CaptionRenderError.invalid("The nameplate is not on the plan's canvas")
        }
      }
      captions = Captions(
        overlay: try CaptionOverlay(
          plan: plan, layout: layout, faces: faces, fill: fill, speakerFills: speakerFills),
        titles: plan.titles, nameplate: nameplate)
    }
    let staging = try staging(for: output)
    let asset = AVURLAsset(url: movie)
    let (source, canvas) = try await tracks(asset, expected: plan.map(Canvas.init))
    let segments = try segments(ranges, canvas: canvas)
    do {
      let frames = try await write(
        source, segments: segments, canvas: canvas, captions: &captions, to: staging,
        codec: codec, progress: progress)
      try await verify(staging, frames: frames, audio: source.audio != nil)
      guard renamex_np(staging.path, output.path, UInt32(RENAME_EXCL)) == 0 else {
        throw CaptionRenderError.invalid("Refusing to overwrite \(output.path)")
      }
      return CaptionExportReport(
        output: output.path, frames: frames, ranges: segments,
        duration: Double(frames) / Double(canvas.rate))
    } catch {
      try discard(staging)
      throw error
    }
  }

  /// The ranges snapped to delivery frames, joined in the order given (any order, overlaps
  /// allowed); nil is the whole delivery.
  static func segments(_ ranges: [CaptionExportRange]?, canvas plan: Canvas) throws
    -> [CaptionExportSegment]
  {
    let wanted =
      ranges ?? [CaptionExportRange(start: 0, end: Double(plan.frames) / Double(plan.rate))]
    guard !wanted.isEmpty else { throw CaptionRenderError.invalid("No range to export") }
    var result: [CaptionExportSegment] = []
    var output = 0
    for range in wanted {
      guard range.start.isFinite, range.end.isFinite else {
        throw CaptionRenderError.invalid("Export ranges must be finite seconds")
      }
      let start = Int((range.start * Double(plan.rate)).rounded())
      let end = Int((range.end * Double(plan.rate)).rounded())
      guard start >= 0, start < end, end <= plan.frames else {
        throw CaptionRenderError.invalid(
          "Export range [\(range.start), \(range.end)) is empty or outside the movie")
      }
      result.append(
        CaptionExportSegment(
          start: range.start, end: range.end, startFrame: start, endFrame: end, outputFrame: output)
      )
      output += end - start
    }
    return result
  }

  /// Removes the staging file and the writer's own temporary siblings of it (`NAME.sb-*`, left
  /// behind by a cancelled writer). The fresh hidden name is ours alone; a finished output is
  /// never touched.
  static func discard(_ staging: URL) throws {
    let directory = staging.deletingLastPathComponent()
    for name in try FileManager.default.contentsOfDirectory(atPath: directory.path)
    where name.hasPrefix(staging.lastPathComponent) {
      do {
        try FileManager.default.removeItem(at: directory.appendingPathComponent(name))
      } catch CocoaError.fileNoSuchFile {
        continue  // the writer removed it meanwhile
      }
    }
  }

  /// The delivery frame shown at exported frame `frame`.
  static func deliveryFrame(_ frame: Int, in segments: [CaptionExportSegment]) -> Int? {
    guard
      let segment = segments.last(where: { $0.outputFrame <= frame }),
      frame - segment.outputFrame < segment.endFrame - segment.startFrame
    else { return nil }
    return segment.startFrame + frame - segment.outputFrame
  }

  /// A fresh hidden name beside `output`, so the final rename stays on one volume.
  static func staging(for output: URL) throws -> URL {
    guard output.isFileURL, output.path.hasPrefix("/"), output.pathExtension == "mp4" else {
      throw CaptionRenderError.invalid("The output must be an absolute .mp4 path")
    }
    guard !FileManager.default.fileExists(atPath: output.path) else {
      throw CaptionRenderError.invalid("Refusing to overwrite \(output.path)")
    }
    let directory = output.deletingLastPathComponent()
    var isDirectory: ObjCBool = false
    guard FileManager.default.fileExists(atPath: directory.path, isDirectory: &isDirectory),
      isDirectory.boolValue
    else { throw CaptionRenderError.invalid("No directory for \(output.path)") }
    return directory.appendingPathComponent(
      ".\(output.deletingPathExtension().lastPathComponent).partial-\(UUID().uuidString).mp4")
  }

  struct Source {
    let video: AVAssetTrack
    let audio: AVAssetTrack?
  }

  /// A movie's frame size, whole-number rate and length in frames.
  struct Canvas: Equatable {
    let width: Int
    let height: Int
    let rate: Int
    let frames: Int

    init(width: Int, height: Int, rate: Int, frames: Int) {
      self.width = width
      self.height = height
      self.rate = rate
      self.frames = frames
    }

    init(_ plan: CaptionPlan) {
      self.init(width: plan.width, height: plan.height, rate: plan.rate, frames: plan.frames)
    }
  }

  /// The captions drawn over each frame.
  struct Captions {
    var overlay: CaptionOverlay
    let titles: [MotionCaptionTitle]
    let nameplate: MotionCaptionBitmap?
  }

  /// The movie's tracks and canvas, refused unless it is `expected` (the plan's) when given.
  static func tracks(_ asset: AVURLAsset, expected: Canvas?) async throws -> (Source, Canvas) {
    let video: AVAssetTrack
    let audio: AVAssetTrack?
    let size: CGSize
    let rate: Float
    let range: CMTimeRange
    do {
      guard let first = try await asset.loadTracks(withMediaType: .video).first else {
        throw CaptionRenderError.invalid("\(asset.url.path) has no video")
      }
      video = first
      audio = try await asset.loadTracks(withMediaType: .audio).first
      (size, rate, range) = try await video.load(.naturalSize, .nominalFrameRate, .timeRange)
    } catch let error where error is CaptionRenderError || error is CancellationError {
      throw error
    } catch {
      throw CaptionRenderError.invalid("Unreadable movie \(asset.url.path): \(error)")
    }
    // shortcut: whole-number constant frame rates only (deliveries are 30 fps); a fractional
    // or variable rate needs time-based ranges and frame mapping.
    let whole = Int(Double(rate).rounded())
    guard whole > 0, abs(Double(rate) - Double(whole)) < 0.01 else {
      throw CaptionRenderError.invalid(
        "\(asset.url.path) is at \(rate) fps; only whole-number frame rates are exported")
    }
    let measured = Canvas(
      width: Int(size.width), height: Int(size.height), rate: whole,
      frames: Int((range.duration.seconds * Double(whole)).rounded()))
    if let expected, measured != expected {
      throw CaptionRenderError.invalid(
        "\(asset.url.path) is \(measured.width)x\(measured.height) at \(rate) fps for"
          + " \(measured.frames) frames, not the plan's \(expected.width)x\(expected.height)"
          + " at \(expected.rate) fps for \(expected.frames)")
    }
    return (Source(video: video, audio: audio), measured)
  }

  static func time(_ frame: Int, _ rate: Int) -> CMTime {
    CMTime(value: CMTimeValue(frame), timescale: CMTimeScale(rate))
  }

  /// Writes the joined, captioned ranges to `staging` and returns the frames written.
  static func write(
    _ source: Source, segments: [CaptionExportSegment], canvas plan: Canvas,
    captions: inout Captions?, to staging: URL,
    codec: CaptionVideoCodec, progress: (@Sendable (Int, Int) -> Void)?
  ) async throws -> Int {
    let composition = AVMutableComposition()
    guard
      let video = composition.addMutableTrack(
        withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)
    else { throw CaptionRenderError.unavailable("Cannot compose the movie's video") }
    let audio =
      source.audio == nil
      ? nil
      : composition.addMutableTrack(
        withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)
    for segment in segments {
      let range = CMTimeRange(
        start: time(segment.startFrame, plan.rate),
        end: time(segment.endFrame, plan.rate))
      let at = time(segment.outputFrame, plan.rate)
      try video.insertTimeRange(range, of: source.video, at: at)
      if let audio, let track = source.audio {
        try audio.insertTimeRange(range, of: track, at: at)
      }
    }
    let total = segments.reduce(0) { $0 + $1.endFrame - $1.startFrame }
    let reader = try AVAssetReader(asset: composition)
    let frames = AVAssetReaderVideoCompositionOutput(
      videoTracks: [video],
      videoSettings: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
    frames.videoComposition = videoComposition(composition, plan: plan, frames: total)
    reader.add(frames)
    var sound: AVAssetReaderAudioMixOutput?
    if let audio {
      let output = AVAssetReaderAudioMixOutput(
        audioTracks: [audio],
        audioSettings: [
          AVFormatIDKey: kAudioFormatLinearPCM, AVSampleRateKey: audioRate,
          AVNumberOfChannelsKey: audioChannels, AVLinearPCMBitDepthKey: 32,
          AVLinearPCMIsFloatKey: true, AVLinearPCMIsBigEndianKey: false,
          AVLinearPCMIsNonInterleaved: false,
        ])
      reader.add(output)
      sound = output
    }
    let writer = try AVAssetWriter(outputURL: staging, fileType: .mp4)
    writer.shouldOptimizeForNetworkUse = true
    let picture = AVAssetWriterInput(
      mediaType: .video, outputSettings: videoSettings(plan: plan, codec: codec))
    picture.expectsMediaDataInRealTime = false
    let adaptor = AVAssetWriterInputPixelBufferAdaptor(
      assetWriterInput: picture, sourcePixelBufferAttributes: nil)
    guard writer.canAdd(picture) else {
      throw CaptionRenderError.unavailable("The encoder rejected \(codec.rawValue)")
    }
    writer.add(picture)
    let voice: AVAssetWriterInput? =
      sound == nil
      ? nil
      : AVAssetWriterInput(
        mediaType: .audio,
        outputSettings: [
          AVFormatIDKey: kAudioFormatMPEG4AAC, AVSampleRateKey: audioRate,
          AVNumberOfChannelsKey: audioChannels, AVEncoderBitRateKey: audioBitRate,
        ])
    if let voice {
      voice.expectsMediaDataInRealTime = false
      guard writer.canAdd(voice) else {
        throw CaptionRenderError.unavailable("The encoder rejected AAC")
      }
      writer.add(voice)
    }
    guard reader.startReading() else {
      throw failure("Cannot read the movie", reader.error)
    }
    guard writer.startWriting() else {
      reader.cancelReading()
      throw failure("Cannot start the encoder", writer.error)
    }
    writer.startSession(atSourceTime: .zero)
    var written = 0
    var videoDone = false
    var audioDone = voice == nil
    var shown: [Int: Int]? = nil
    var drawn: [UInt8] = []
    do {
      while !videoDone || !audioDone {
        try Task.checkCancellation()
        var moved = false
        if !videoDone, picture.isReadyForMoreMediaData {
          moved = true
          if let sample = frames.copyNextSampleBuffer() {
            let pts = CMSampleBufferGetPresentationTimeStamp(sample)
            let frame = Int((pts.seconds * Double(plan.rate)).rounded())
            guard let buffer = CMSampleBufferGetImageBuffer(sample),
              let delivery = deliveryFrame(frame, in: segments)
            else { throw CaptionRenderError.unavailable("The movie gave an unexpected frame") }
            if var current = captions {
              let state = visible(current.titles, at: delivery)
              if state != shown {
                let bitmaps = try current.overlay.layers(at: delivery).map(\.bitmap)
                let layers = (current.nameplate.map { [$0] } ?? []) + bitmaps
                drawn =
                  layers.isEmpty
                  ? [] : CaptionFrame.composite(layers, width: plan.width, height: plan.height)
                shown = state
                captions = current
              }
            }
            if !drawn.isEmpty { try CaptionFrame.burn(drawn, into: buffer) }
            guard adaptor.append(buffer, withPresentationTime: pts) else {
              throw failure("The encoder rejected a frame", writer.error)
            }
            written += 1
            progress?(written, total)
          } else {
            try readerFinished(reader)
            picture.markAsFinished()
            videoDone = true
          }
        }
        if !audioDone, let voice, let sound, voice.isReadyForMoreMediaData {
          moved = true
          if let sample = sound.copyNextSampleBuffer() {
            guard voice.append(sample) else {
              throw failure("The encoder rejected audio", writer.error)
            }
          } else {
            try readerFinished(reader)
            voice.markAsFinished()
            audioDone = true
          }
        }
        if !moved { try await Task.sleep(for: writerPollInterval) }
      }
    } catch {
      reader.cancelReading()
      writer.cancelWriting()
      throw error
    }
    await writer.finishWriting()
    guard writer.status == .completed else {
      throw failure("The encoder did not finish", writer.error)
    }
    return written
  }

  static func failure(_ what: String, _ cause: (any Error)?) -> CaptionRenderError {
    .unavailable(cause.map { "\(what): \($0)" } ?? what)
  }

  static func readerFinished(_ reader: AVAssetReader) throws {
    guard reader.status != .failed else {
      throw failure("Reading the movie failed", reader.error)
    }
  }

  /// Visible graphemes per showing title, the key of the composited caption canvas.
  static func visible(_ titles: [MotionCaptionTitle], at frame: Int) -> [Int: Int] {
    var result: [Int: Int] = [:]
    for (index, title) in titles.enumerated() {
      let count = title.visible(at: frame)
      if count > 0 { result[index] = count }
    }
    return result
  }

  /// One pass-through layer on the plan's canvas, with BT.709 colour properties so the
  /// compositor leaves the decoded code values as they are.
  static func videoComposition(_ composition: AVComposition, plan: Canvas, frames: Int)
    -> AVVideoComposition
  {
    let instruction = AVMutableVideoCompositionInstruction()
    instruction.timeRange = CMTimeRange(start: .zero, duration: time(frames, plan.rate))
    instruction.layerInstructions = composition.tracks(withMediaType: .video).map {
      AVMutableVideoCompositionLayerInstruction(assetTrack: $0)
    }
    let result = AVMutableVideoComposition()
    result.renderSize = CGSize(width: plan.width, height: plan.height)
    result.frameDuration = time(1, plan.rate)
    result.instructions = [instruction]
    result.colorPrimaries = AVVideoColorPrimaries_ITU_R_709_2
    result.colorTransferFunction = AVVideoTransferFunction_ITU_R_709_2
    result.colorYCbCrMatrix = AVVideoYCbCrMatrix_ITU_R_709_2
    return result
  }

  /// The writer input's settings; `[String: Any]` is AVAssetWriterInput's own parameter type.
  static func videoSettings(plan: Canvas, codec: CaptionVideoCodec) -> [String: Any] {
    [
      AVVideoCodecKey: codec == .h264 ? AVVideoCodecType.h264 : AVVideoCodecType.hevc,
      AVVideoWidthKey: plan.width, AVVideoHeightKey: plan.height,
      AVVideoCompressionPropertiesKey: [
        AVVideoAverageBitRateKey: Int(
          Double(plan.width * plan.height * plan.rate) * bitsPerPixel)
      ],
      AVVideoColorPropertiesKey: [
        AVVideoColorPrimariesKey: AVVideoColorPrimaries_ITU_R_709_2,
        AVVideoTransferFunctionKey: AVVideoTransferFunction_ITU_R_709_2,
        AVVideoYCbCrMatrixKey: AVVideoYCbCrMatrix_ITU_R_709_2,
      ],
    ]
  }

  /// The staged movie holds exactly `frames` video samples (and audio when the source had it).
  static func verify(_ staged: URL, frames: Int, audio: Bool) async throws {
    let asset = AVURLAsset(url: staged)
    let videos = try await asset.loadTracks(withMediaType: .video)
    let sounds = try await asset.loadTracks(withMediaType: .audio)
    guard let video = videos.first, videos.count == 1, sounds.count == (audio ? 1 : 0) else {
      throw CaptionRenderError.unavailable("The exported movie lacks its tracks")
    }
    let reader = try AVAssetReader(asset: asset)
    let output = AVAssetReaderTrackOutput(track: video, outputSettings: nil)
    reader.add(output)
    guard reader.startReading() else {
      throw CaptionRenderError.unavailable("Cannot read back the exported movie")
    }
    var count = 0
    while let sample = output.copyNextSampleBuffer() {
      count += CMSampleBufferGetNumSamples(sample)
    }
    try readerFinished(reader)
    guard count == frames else {
      throw CaptionRenderError.unavailable(
        "The exported movie holds \(count) frames, not \(frames)")
    }
  }
}
