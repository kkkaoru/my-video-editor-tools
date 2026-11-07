import Foundation

/// One 16 kHz mono clip and where its two tracks go; the order of the tracks means nothing.
struct SeparationClip: Decodable {
  let input: String
  let outputs: [String]
}

/// The app's `separation-request.json`. Fields only ONNX Runtime uses are ignored.
struct SeparationRequest: Decodable {
  let model: String
  let clips: [SeparationClip]
}

/// Runs a request file: clips in order, as a clip may be a track an earlier one wrote.
///
/// Reports `separated done/total` after each clip. Every output is written atomically, so killing the
/// process leaves complete tracks or none: that is how the app cancels.
public func runRequest(
  at file: URL,
  loadModel: (URL) throws -> any WindowModel,
  report: (String) -> Void
) throws {
  let request: SeparationRequest
  do {
    request = try JSONDecoder().decode(SeparationRequest.self, from: Data(contentsOf: file))
  } catch let error as DecodingError {
    throw SeparationError.invalidRequest("\(error)")
  }
  guard let invalid = request.clips.first(where: { $0.outputs.count != 2 }) else {
    let separator = try ClipSeparator(model: loadModel(URL(fileURLWithPath: request.model)))
    for (index, clip) in request.clips.enumerated() {
      let wave = try WaveFile.samples(in: Data(contentsOf: URL(fileURLWithPath: clip.input)))
      let tracks = try separator.separate(wave)
      for (output, track) in zip(clip.outputs, tracks) {
        try WaveFile.floatFile(track).write(to: URL(fileURLWithPath: output), options: .atomic)
      }
      report("separated \(index + 1)/\(request.clips.count)")
    }
    return
  }
  throw SeparationError.invalidRequest("\(invalid.input) needs two outputs")
}
