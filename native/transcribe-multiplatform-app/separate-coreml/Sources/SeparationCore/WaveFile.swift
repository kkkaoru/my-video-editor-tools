import Foundation

/// What can go wrong with a request, a clip or the model; the text ends up in the app's error.
public enum SeparationError: Error, Equatable, CustomStringConvertible {
  case invalidRequest(String)
  case invalidWave(String)
  case invalidModel(String)

  public var description: String {
    switch self {
    case .invalidRequest(let detail): "separation request: \(detail)"
    case .invalidWave(let detail): "audio: \(detail)"
    case .invalidModel(let detail): "model: \(detail)"
    }
  }
}

/// 16 kHz mono WAV in the forms the app writes: 16-bit from its audio extraction, 32-bit float for tracks.
enum WaveFile {
  private static let sampleRate: UInt32 = 16_000
  private static let pcmFormat: UInt16 = 1
  private static let floatFormat: UInt16 = 3
  private static let riffHeaderBytes = 12
  private static let chunkHeaderBytes = 8
  private static let formatChunkBytes = 16
  private static let fullScale: Float = 32_768

  static func samples(in data: Data) throws -> [Float] {
    let bytes = [UInt8](data)
    guard bytes.count >= riffHeaderBytes, tag(bytes, at: 0) == "RIFF", tag(bytes, at: 8) == "WAVE" else {
      throw SeparationError.invalidWave("not a WAV file")
    }
    var at = riffHeaderBytes
    var isFloat: Bool?
    while at + chunkHeaderBytes <= bytes.count {
      let size = Int(littleUInt32(bytes, at: at + 4))
      let body = at + chunkHeaderBytes
      let end = min(body + size, bytes.count)
      switch tag(bytes, at: at) {
      case "fmt " where end - body >= formatChunkBytes:
        isFloat = try readFormat(bytes, at: body)
      case "data":
        if let isFloat { return decoded(bytes[body..<end], isFloat: isFloat) }
      default:
        break
      }
      // Chunks are padded to an even size.
      at = end + (size & 1)
    }
    throw SeparationError.invalidWave("no audio in the WAV file")
  }

  /// 32-bit float: restoring the level can take a quiet track past full scale, where 16 bits would clip it.
  static func floatFile(_ samples: [Float]) -> Data {
    let dataBytes = UInt32(samples.count * 4)
    var file = Data()
    file.reserveCapacity(44 + samples.count * 4)
    file.append(contentsOf: Array("RIFF".utf8))
    file.append(contentsOf: littleBytes(36 + dataBytes))
    file.append(contentsOf: Array("WAVEfmt ".utf8))
    file.append(contentsOf: littleBytes(UInt32(formatChunkBytes)))
    file.append(contentsOf: littleBytes(floatFormat))
    file.append(contentsOf: littleBytes(UInt16(1)))
    file.append(contentsOf: littleBytes(sampleRate))
    file.append(contentsOf: littleBytes(sampleRate * 4))
    file.append(contentsOf: littleBytes(UInt16(4)))
    file.append(contentsOf: littleBytes(UInt16(32)))
    file.append(contentsOf: Array("data".utf8))
    file.append(contentsOf: littleBytes(dataBytes))
    file.append(contentsOf: samples.flatMap { littleBytes($0.bitPattern) })
    return file
  }

  private static func readFormat(_ bytes: [UInt8], at body: Int) throws -> Bool {
    let format = littleUInt16(bytes, at: body)
    let bits = littleUInt16(bytes, at: body + 14)
    let isPCM = format == pcmFormat && bits == 16
    let isFloat = format == floatFormat && bits == 32
    guard isPCM || isFloat, littleUInt16(bytes, at: body + 2) == 1, littleUInt32(bytes, at: body + 4) == sampleRate
    else {
      throw SeparationError.invalidWave("not 16 kHz mono audio")
    }
    return isFloat
  }

  private static func decoded(_ body: ArraySlice<UInt8>, isFloat: Bool) -> [Float] {
    let width = isFloat ? 4 : 2
    return stride(from: body.startIndex, through: body.endIndex - width, by: width).map { at in
      if isFloat {
        return Float(bitPattern: littleUInt32(Array(body[at..<at + 4]), at: 0))
      }
      let value = Int16(bitPattern: UInt16(body[at]) | UInt16(body[at + 1]) << 8)
      return Float(value) / fullScale
    }
  }

  private static func tag(_ bytes: [UInt8], at: Int) -> String {
    String(decoding: bytes[at..<at + 4], as: UTF8.self)
  }

  private static func littleUInt16(_ bytes: [UInt8], at: Int) -> UInt16 {
    UInt16(bytes[at]) | UInt16(bytes[at + 1]) << 8
  }

  private static func littleUInt32(_ bytes: [UInt8], at: Int) -> UInt32 {
    UInt32(littleUInt16(bytes, at: at)) | UInt32(littleUInt16(bytes, at: at + 2)) << 16
  }

  private static func littleBytes<Value: FixedWidthInteger>(_ value: Value) -> [UInt8] {
    (0..<Value.bitWidth / 8).map { UInt8(truncatingIfNeeded: value >> ($0 * 8)) }
  }
}
