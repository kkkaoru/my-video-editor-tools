import CoreML
import Foundation

/// The Core ML program on the GPU; `.cpuAndGPU` because the Neural Engine does not compile this graph.
public final class CoreMLWindowModel: WindowModel {
  private static let inputName = "wave"
  private static let outputName = "tracks"
  private static let trackCount = 2

  public let windowSamples: Int
  private let model: MLModel
  private let compiled: URL?

  /// Loads `.mlmodelc` as it is. `.mlpackage` is compiled into `folder` and removed by `removeCompiled()`,
  /// in a folder the app clears, because a kill cannot clean up after itself.
  public init(source: URL, compilingIn folder: URL) throws {
    let configuration = MLModelConfiguration()
    configuration.computeUnits = .cpuAndGPU
    var moved: URL?
    if source.pathExtension != "mlmodelc" {
      let target = folder.appendingPathComponent("separation.mlmodelc")
      let manager = FileManager.default
      if manager.fileExists(atPath: target.path) { try manager.removeItem(at: target) }
      try manager.moveItem(at: MLModel.compileModel(at: source), to: target)
      moved = target
    }
    compiled = moved
    model = try MLModel(contentsOf: moved ?? source, configuration: configuration)
    guard let shape = model.modelDescription.inputDescriptionsByName[Self.inputName]?
      .multiArrayConstraint?.shape.map(\.intValue), shape.count == 2, shape[0] == 1, shape[1] > 0
    else {
      throw SeparationError.invalidModel("expected a fixed input `wave` of [1, samples]")
    }
    windowSamples = shape[1]
  }

  public func removeCompiled() throws {
    if let compiled { try FileManager.default.removeItem(at: compiled) }
  }

  public func tracks(of window: [Float]) throws -> [[Float]] {
    // A pool per window: Core ML's autoreleased buffers would otherwise pile up over a long request.
    try autoreleasepool {
      let input = MLMultiArray(MLShapedArray<Float>(scalars: window, shape: [1, window.count]))
      let features = try MLDictionaryFeatureProvider(dictionary: [Self.inputName: input])
      guard let output = try model.prediction(from: features).featureValue(for: Self.outputName)?.multiArrayValue,
        output.shape.map(\.intValue) == [Self.trackCount, 1, window.count]
      else {
        throw SeparationError.invalidModel("expected an output `tracks` of [2, 1, samples]")
      }
      let scalars = MLShapedArray<Float>(converting: output).scalars
      return (0..<Self.trackCount).map { Array(scalars[$0 * window.count..<($0 + 1) * window.count]) }
    }
  }
}
