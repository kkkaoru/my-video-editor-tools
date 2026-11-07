import Foundation
import SeparationCore

let usageExitCode: Int32 = 2
let failureExitCode: Int32 = 1

func writeLine(_ line: String) {
  FileHandle.standardError.write(Data((line + "\n").utf8))
}

guard CommandLine.arguments.count == 2 else {
  writeLine("usage: separate-coreml REQUEST.json")
  exit(usageExitCode)
}

let requestFile = URL(fileURLWithPath: CommandLine.arguments[1])
do {
  var loaded: CoreMLWindowModel?
  defer {
    do {
      try loaded?.removeCompiled()
    } catch {
      writeLine("could not remove the compiled model: \(error)")
    }
  }
  try runRequest(
    at: requestFile,
    loadModel: { source in
      let model = try CoreMLWindowModel(source: source, compilingIn: requestFile.deletingLastPathComponent())
      loaded = model
      return model
    },
    report: writeLine
  )
} catch {
  writeLine("\(error)")
  exit(failureExitCode)
}
