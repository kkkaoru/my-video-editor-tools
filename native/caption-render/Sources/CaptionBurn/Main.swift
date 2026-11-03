import CaptionRender
import Foundation

/// Burns a delivery's caption plan into (ranges of) its movie and prints the JSON report.
@main
struct CaptionBurnMain {
  static func main() async {
    do {
      let command = try CaptionBurnCommand(
        arguments: Array(CommandLine.arguments.dropFirst()),
        directory: URL(fileURLWithPath: FileManager.default.currentDirectoryPath))
      let report = try await command.run()
      let encoder = JSONEncoder()
      encoder.outputFormatting = [.sortedKeys]
      try FileHandle.standardOutput.write(contentsOf: encoder.encode(report) + Data("\n".utf8))
    } catch {
      FileHandle.standardError.write(Data("\(error)\n".utf8))
      exit(1)
    }
  }
}
