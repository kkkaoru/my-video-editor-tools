import CoreML
import Foundation
import Testing

@testable import SeparationCore

/// First track is the window, second its negative; `swapsSecondCall` swaps them on the second window.
private final class FakeModel: WindowModel {
  let windowSamples: Int
  var seenWindows: [[Float]] = []
  var swapsSecondCall = false
  var result: [[Float]]?

  init(windowSamples: Int) { self.windowSamples = windowSamples }

  func tracks(of window: [Float]) throws -> [[Float]] {
    seenWindows.append(window)
    if let result { return result }
    let first = window
    let second = window.map { -$0 }
    return swapsSecondCall && seenWindows.count == 2 ? [second, first] : [first, second]
  }
}

private func withTemporaryFolder(_ body: (URL) throws -> Void) throws {
  let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
  try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
  try body(folder)
  try FileManager.default.removeItem(at: folder)
}

@Suite struct WaveFileTests {
  @Test func floatFileRoundTrips() throws {
    #expect(try WaveFile.samples(in: WaveFile.floatFile([0.25, -1.5])) == [0.25, -1.5])
  }

  @Test func floatFileHasTheExpectedHeader() {
    let file = WaveFile.floatFile([0])
    #expect(file.count == 48)
    #expect(Array(file[0..<4]) == Array("RIFF".utf8))
    #expect(Array(file[20..<22]) == [3, 0])
    #expect(Array(file[24..<28]) == [0x80, 0x3E, 0, 0])
  }

  @Test func readsPCMAndSkipsPaddedChunks() throws {
    var file = Data("RIFF\0\0\0\0WAVE".utf8)
    file.append(contentsOf: Array("LIST".utf8) + [1, 0, 0, 0, 9, 0])
    file.append(contentsOf: Array("fmt ".utf8) + [16, 0, 0, 0, 1, 0, 1, 0, 0x80, 0x3E, 0, 0])
    file.append(contentsOf: [0, 0, 0, 0, 2, 0, 16, 0])
    file.append(contentsOf: Array("data".utf8) + [4, 0, 0, 0, 0, 0x40, 0, 0x80])
    #expect(try WaveFile.samples(in: file) == [0.5, -1])
  }

  @Test func refusesWhatIsNotAWave() {
    #expect(throws: SeparationError.invalidWave("not a WAV file")) {
      try WaveFile.samples(in: Data("nope".utf8))
    }
  }

  @Test func refusesAnotherRate() {
    var file = WaveFile.floatFile([0])
    file[24] = 0x81
    #expect(throws: SeparationError.invalidWave("not 16 kHz mono audio")) {
      try WaveFile.samples(in: file)
    }
  }

  @Test func refusesAudioBeforeAFormat() {
    var file = Data("RIFF\0\0\0\0WAVE".utf8)
    file.append(contentsOf: Array("data".utf8) + [2, 0, 0, 0, 0, 0])
    #expect(throws: SeparationError.invalidWave("no audio in the WAV file")) {
      try WaveFile.samples(in: file)
    }
  }
}

@Suite struct ClipSeparatorTests {
  @Test func shortClipIsPaddedAndCutBack() throws {
    let model = FakeModel(windowSamples: 20_000)
    let tracks = try ClipSeparator(model: model).separate([0.5, -0.5])
    #expect(model.seenWindows[0].count == 20_000)
    #expect(Array(model.seenWindows[0][0..<3]) == [0.5, -0.5, 0])
    #expect(tracks[0].count == 2)
    #expect(tracks[1].count == 2)
  }

  @Test func restoresTheLevelOfTheInput() throws {
    let model = FakeModel(windowSamples: 20_000)
    model.result = [Array(repeating: 2, count: 20_000), Array(repeating: 4, count: 20_000)]
    let tracks = try ClipSeparator(model: model).separate([0.5, 0.5])
    #expect(tracks[0] == [0.5, 0.5])
    #expect(tracks[1] == [0.5, 0.5])
  }

  @Test func longClipIsWindowedWithAnOverlap() throws {
    let model = FakeModel(windowSamples: 24_000)
    let wave = (0..<30_000).map { Float($0 % 7) - 3 }
    let tracks = try ClipSeparator(model: model).separate(wave)
    #expect(model.seenWindows.count == 2)
    #expect(model.seenWindows[1][0] == wave[8_000])
    #expect(tracks[0].count == 30_000)
    #expect(tracks[1].count == 30_000)
  }

  @Test func keepsTrackOrderWhenAWindowSwaps() throws {
    let model = FakeModel(windowSamples: 24_000)
    model.swapsSecondCall = true
    let wave = (0..<30_000).map { Float($0 % 7) - 3 }
    let tracks = try ClipSeparator(model: model).separate(wave)
    #expect(tracks[0][29_999] == wave[29_999])
    #expect(tracks[1][29_999] == -wave[29_999])
  }

  @Test func silenceSkipsTheModel() throws {
    let model = FakeModel(windowSamples: 20_000)
    let tracks = try ClipSeparator(model: model).separate([0, 0, 0])
    #expect(model.seenWindows.isEmpty)
    #expect(tracks == [[0, 0, 0], [0, 0, 0]])
  }

  @Test func refusesAnEmptyClip() {
    #expect(throws: SeparationError.invalidWave("the clip has no samples")) {
      try ClipSeparator(model: FakeModel(windowSamples: 20_000)).separate([])
    }
  }

  @Test func refusesAWindowShorterThanTheOverlap() {
    #expect(throws: SeparationError.invalidModel("a window of 100 samples is too short")) {
      try ClipSeparator(model: FakeModel(windowSamples: 100)).separate([1])
    }
  }

  @Test func refusesTheWrongTracks() {
    let model = FakeModel(windowSamples: 20_000)
    model.result = [[1]]
    #expect(throws: SeparationError.invalidModel("expected two tracks of 20000 samples")) {
      try ClipSeparator(model: model).separate([1])
    }
  }

  @Test func refusesNotANumber() {
    let model = FakeModel(windowSamples: 20_000)
    model.result = [Array(repeating: .nan, count: 20_000), Array(repeating: 0, count: 20_000)]
    #expect(throws: SeparationError.invalidModel("the tracks are not finite numbers")) {
      try ClipSeparator(model: model).separate([1])
    }
  }
}

@Suite struct RunRequestTests {
  @Test func writesTracksAndReportsInOrder() throws {
    try withTemporaryFolder { folder in
      let clip = folder.appendingPathComponent("clip.wav")
      try WaveFile.floatFile([0.5, -0.5]).write(to: clip)
      let first = folder.appendingPathComponent("a.wav")
      let second = folder.appendingPathComponent("b.wav")
      let request = folder.appendingPathComponent("request.json")
      let json = #"{"library":"ignored","model":"m","runs_on":"cpu","clips":[{"input":"\#(clip.path)","outputs":["\#(first.path)","\#(second.path)"]}]}"#
      try Data(json.utf8).write(to: request)
      var lines: [String] = []
      try runRequest(
        at: request, loadModel: { _ in FakeModel(windowSamples: 20_000) }, report: { lines.append($0) })
      #expect(lines == ["separated 1/1"])
      #expect(try WaveFile.samples(in: Data(contentsOf: first)) == [0.5, -0.5])
      #expect(try WaveFile.samples(in: Data(contentsOf: second)) == [-0.5, 0.5])
    }
  }

  @Test func refusesMalformedJSON() throws {
    try withTemporaryFolder { folder in
      let request = folder.appendingPathComponent("request.json")
      try Data("{}".utf8).write(to: request)
      #expect(throws: SeparationError.self) {
        try runRequest(at: request, loadModel: { _ in FakeModel(windowSamples: 20_000) }, report: { _ in })
      }
    }
  }

  @Test func refusesAClipWithoutTwoOutputsBeforeLoading() throws {
    try withTemporaryFolder { folder in
      let request = folder.appendingPathComponent("request.json")
      try Data(#"{"model":"m","clips":[{"input":"x","outputs":["y"]}]}"#.utf8).write(to: request)
      var loads = 0
      #expect(throws: SeparationError.invalidRequest("x needs two outputs")) {
        try runRequest(
          at: request,
          loadModel: { _ in
            loads += 1
            return FakeModel(windowSamples: 20_000)
          },
          report: { _ in })
      }
      #expect(loads == 0)
    }
  }

  @Test func errorsDescribeThemselves() {
    #expect(SeparationError.invalidRequest("a").description == "separation request: a")
    #expect(SeparationError.invalidWave("b").description == "audio: b")
    #expect(SeparationError.invalidModel("c").description == "model: c")
  }
}

/// Tiny real models from `make_fixtures.py`: both tracks are the input.
private enum TinyModel {
  static let twoTracks = "CAQSKwoSCgR3YXZlGgoqCAoCASAQoIAEUhUKBnRyYWNrcxoLKgkKAwIBIBCggASiH0wKHgoGZXhwYW5kEgR3YXZlGghleHBhbmRlZKpGAwoBAAomCgV0d2ljZRIIZXhwYW5kZWQSCGV4cGFuZGVkGgZ0cmFja3OiPQAoATAB"
  static let wrongInput = "CAQSKQoRCgR3YXZlGgkqBwoBIBCggARSFAoGdHJhY2tzGgoqCAoCAiAQoIAEoh9MCh4KBmV4cGFuZBIEd2F2ZRoIZXhwYW5kZWSqRgMKAQAKJgoFdHdpY2USCGV4cGFuZGVkEghleHBhbmRlZBoGdHJhY2tzoj0AKAEwAQ=="
  static let wrongOutput = "CAQSKgoSCgR3YXZlGgoqCAoCASAQoIAEUhQKBnRyYWNrcxoKKggKAgIgEKCABKIfJAoeCgV0d2ljZRIEd2F2ZRIEd2F2ZRoGdHJhY2tzoj0AKAEwAQ=="
  static let samples = 32

  static func file(_ base64: String, in folder: URL) throws -> URL {
    let file = folder.appendingPathComponent("tiny.mlmodel")
    try #require(Data(base64Encoded: base64)).write(to: file)
    return file
  }
}

@Suite struct CoreMLWindowModelTests {
  @Test func compilesRunsAndRemovesItsCopy() throws {
    try withTemporaryFolder { folder in
      let source = try TinyModel.file(TinyModel.twoTracks, in: folder)
      let model = try CoreMLWindowModel(source: source, compilingIn: folder)
      let compiled = folder.appendingPathComponent("separation.mlmodelc")
      #expect(FileManager.default.fileExists(atPath: compiled.path))
      #expect(model.windowSamples == TinyModel.samples)
      let window = (0..<TinyModel.samples).map(Float.init)
      #expect(try model.tracks(of: window) == [window, window])
      try model.removeCompiled()
      #expect(!FileManager.default.fileExists(atPath: compiled.path))
    }
  }

  @Test func replacesACopyAKilledRunLeft() throws {
    try withTemporaryFolder { folder in
      let source = try TinyModel.file(TinyModel.twoTracks, in: folder)
      let compiled = folder.appendingPathComponent("separation.mlmodelc")
      try FileManager.default.createDirectory(at: compiled, withIntermediateDirectories: true)
      let model = try CoreMLWindowModel(source: source, compilingIn: folder)
      #expect(model.windowSamples == TinyModel.samples)
    }
  }

  @Test func loadsACompiledModelAsItIsAndLeavesItThere() throws {
    try withTemporaryFolder { folder in
      let compiled = try MLModel.compileModel(at: TinyModel.file(TinyModel.twoTracks, in: folder))
      let model = try CoreMLWindowModel(source: compiled, compilingIn: folder)
      try model.removeCompiled()
      #expect(FileManager.default.fileExists(atPath: compiled.path))
      try FileManager.default.removeItem(at: compiled)
    }
  }

  @Test func refusesAnInputThatIsNotOneWindow() throws {
    try withTemporaryFolder { folder in
      let source = try TinyModel.file(TinyModel.wrongInput, in: folder)
      #expect(throws: SeparationError.self) {
        try CoreMLWindowModel(source: source, compilingIn: folder)
      }
    }
  }

  @Test func refusesAnOutputThatIsNotTwoTracks() throws {
    try withTemporaryFolder { folder in
      let source = try TinyModel.file(TinyModel.wrongOutput, in: folder)
      let model = try CoreMLWindowModel(source: source, compilingIn: folder)
      #expect(throws: SeparationError.self) {
        try model.tracks(of: Array(repeating: 0, count: TinyModel.samples))
      }
    }
  }
}
