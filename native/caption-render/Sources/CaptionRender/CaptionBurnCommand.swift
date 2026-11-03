import Foundation

/// One `caption-burn` invocation: export (ranges of) a delivery with its caption plan burned in.
///
///     caption-burn MOVIE PLAN.captions.json OUTPUT.mp4 [--range START:END]...
///       [--no-nameplate] [--nameplate-font FONT] [--no-gradient] [--top #RRGGBB]
///       [--bottom #RRGGBB] [--codec h264|hevc] [--over-burned] [--layout FILE]
///       [--mode plan|aboveFace|bottom|top|center] [--offset-x PX] [--offset-y PX]
///       [--speaker ID]... [--hide-speaker ID]...
///
/// `--speaker` keeps only the named speakers' titles and `--hide-speaker` drops a speaker's
/// (`unassigned`: titles without a speaker); positions are resolved before, so shown titles
/// never move. A version-2 plan's speakers keep their own colours (`--no-gradient`: each its
/// top colour); `--top`/`--bottom` recolour the unassigned titles.
///
/// Captions are placed by `FILE`, else by the movie's `<stem>.caption-layout.json`, else where
/// the plan puts them; `--mode` and the offsets override the layout's, and `aboveFace` reads the
/// movie's `<stem>.faces.json`.
/// Ranges are seconds on the delivery's clock, joined in the order given (any order, overlaps
/// allowed); none is the whole delivery. A plan's
/// nameplate (unless already burned) is drawn from the icon beside the plan and the font file
/// the plan names, found by name and SHA-256 beside the plan or in the font folders the
/// pipeline reads (`FONT` overrides the search); `--no-nameplate` leaves it out.
public struct CaptionBurnCommand: Equatable, Sendable {
  public static let usage =
    "usage: caption-burn MOVIE PLAN OUTPUT.mp4 [--range START:END]... [--no-nameplate]"
    + " [--nameplate-font FONT] [--no-gradient] [--top #RRGGBB] [--bottom #RRGGBB]"
    + " [--codec h264|hevc] [--over-burned] [--layout FILE]"
    + " [--mode plan|aboveFace|bottom|top|center] [--offset-x PX] [--offset-y PX]"
    + " [--speaker ID]... [--hide-speaker ID]..."

  public let movie: URL
  public let plan: URL
  public let output: URL
  public let ranges: [CaptionExportRange]?
  public let nameplate: Bool
  public let nameplateFont: URL?
  public let gradient: Bool
  public let top: String?
  public let bottom: String?
  public let codec: CaptionVideoCodec
  public let overBurned: Bool
  public let layout: URL?
  public let mode: CaptionLayout.Mode?
  public let offsetX: Double?
  public let offsetY: Double?
  /// The speakers to keep (nil: all) and to drop.
  public let speakers: Set<String>?
  public let hiddenSpeakers: Set<String>

  /// Parses the arguments after the program name; relative paths are against `directory`.
  public init(arguments: [String], directory: URL) throws {
    var positional: [URL] = []
    var ranges: [CaptionExportRange] = []
    var nameplate = true
    var font: URL?
    var gradient = true
    var top: String?
    var bottom: String?
    var codec = CaptionVideoCodec.h264
    var overBurned = false
    var layout: URL?
    var mode: CaptionLayout.Mode?
    var offsetX: Double?
    var offsetY: Double?
    var speakers: Set<String>?
    var hidden: Set<String> = []
    var rest = ArraySlice(arguments)
    func value() throws -> String {
      guard let next = rest.popFirst() else { throw CaptionRenderError.invalid(Self.usage) }
      return next
    }
    while let argument = rest.popFirst() {
      switch argument {
      case "--range": ranges.append(try Self.range(try value()))
      case "--no-nameplate": nameplate = false
      case "--nameplate-font": font = Self.path(try value(), directory)
      case "--no-gradient": gradient = false
      case "--top": top = try value()
      case "--bottom": bottom = try value()
      case "--codec":
        guard let chosen = CaptionVideoCodec(rawValue: try value()) else {
          throw CaptionRenderError.invalid(Self.usage)
        }
        codec = chosen
      case "--over-burned": overBurned = true
      case "--layout": layout = Self.path(try value(), directory)
      case "--mode":
        guard let chosen = CaptionLayout.Mode(rawValue: try value()) else {
          throw CaptionRenderError.invalid(Self.usage)
        }
        mode = chosen
      case "--offset-x": offsetX = try Self.number(try value())
      case "--offset-y": offsetY = try Self.number(try value())
      case "--speaker": speakers = (speakers ?? []).union([try value()])
      case "--hide-speaker": hidden.insert(try value())
      case let path where !path.hasPrefix("--"):
        positional.append(Self.path(path, directory))
      default: throw CaptionRenderError.invalid(Self.usage)
      }
    }
    guard positional.count == 3 else { throw CaptionRenderError.invalid(Self.usage) }
    movie = positional[0]
    plan = positional[1]
    output = positional[2]
    self.ranges = ranges.isEmpty ? nil : ranges
    self.nameplate = nameplate
    nameplateFont = font
    self.gradient = gradient
    self.top = top
    self.bottom = bottom
    self.codec = codec
    self.overBurned = overBurned
    self.layout = layout
    self.mode = mode
    self.offsetX = offsetX
    self.offsetY = offsetY
    self.speakers = speakers
    hiddenSpeakers = hidden
  }

  static func number(_ text: String) throws -> Double {
    guard let value = Double(text), value.isFinite else {
      throw CaptionRenderError.invalid("Not a finite number: \(text)")
    }
    return value
  }

  /// The movie's sibling with `suffix` in place of its extension.
  func sibling(_ suffix: String) -> URL {
    movie.deletingLastPathComponent().appendingPathComponent(
      movie.deletingPathExtension().lastPathComponent + suffix)
  }

  /// The layout asked for: `--layout`, else the movie's own, else the plan's positions, with the
  /// command-line overrides.
  func resolvedLayout() throws -> CaptionLayout {
    let own = sibling(CaptionLayout.suffix)
    let file = layout ?? (FileManager.default.fileExists(atPath: own.path) ? own : nil)
    let base = try file.map { try CaptionLayout.decode(try Self.read($0)) } ?? .plan
    return base.overriding(mode: mode, offsetX: offsetX, offsetY: offsetY)
  }

  /// The movie's face track, when it has one.
  func faces() throws -> CaptionFaces? {
    let file = sibling(CaptionFaces.suffix)
    guard FileManager.default.fileExists(atPath: file.path) else { return nil }
    return try CaptionFaces.decode(try Self.read(file))
  }

  static func path(_ text: String, _ directory: URL) -> URL {
    (text.hasPrefix("/") ? URL(fileURLWithPath: text) : directory.appendingPathComponent(text))
      .standardizedFileURL
  }

  static func range(_ text: String) throws -> CaptionExportRange {
    let parts = text.split(separator: ":", omittingEmptySubsequences: false)
    guard parts.count == 2, let start = Double(parts[0]), let end = Double(parts[1]) else {
      throw CaptionRenderError.invalid("A range is START:END seconds, not \(text)")
    }
    return CaptionExportRange(start: start, end: end)
  }

  /// The face colours asked for, over the plan's own; nil when nothing was asked.
  public func fill(for style: MotionCaptionStyle) -> CaptionFill? {
    guard !gradient || top != nil || bottom != nil else { return nil }
    return CaptionFill(
      top: top ?? style.gradientTop, bottom: bottom ?? style.gradientBottom, gradient: gradient)
  }

  /// Reads the plan and its nameplate files, exports, and returns the report.
  public func run(progress: (@Sendable (Int, Int) -> Void)? = nil) async throws
    -> CaptionExportReport
  {
    let plan = try CaptionPlan.decode(try Self.read(self.plan))
    var bitmap: MotionCaptionBitmap?
    if nameplate, let plate = plan.nameplate, !plate.burned {
      let font =
        try nameplateFont.map(Self.read)
        ?? Self.font(
          plate, roots: [self.plan.deletingLastPathComponent()] + Self.fontRoots)
      bitmap = try CaptionNameplateRenderer.bitmap(
        plate,
        icon: try Self.read(
          self.plan.deletingLastPathComponent().appendingPathComponent(plate.icon)),
        font: font, canvasWidth: plan.width, canvasHeight: plan.height)
    }
    // Resolved with every title, then the speakers chosen: shown titles keep their places.
    let shown = try CaptionPlacement.resolve(
      plan: plan, layout: try resolvedLayout(), faces: try faces()
    ).showing(only: speakers, hiding: hiddenSpeakers)
    var icons: MotionCaptionBitmap?
    if nameplate {
      let ids = (plan.speakers ?? []).map(\.id).filter {
        (speakers?.contains($0) ?? true) && !hiddenSpeakers.contains($0)
      }
      var files: [String: Data] = [:]
      for speaker in plan.speakers ?? [] where ids.contains(speaker.id) {
        guard let icon = speaker.icon else { continue }
        files[speaker.id] = try Self.read(
          self.plan.deletingLastPathComponent().appendingPathComponent(icon))
      }
      icons = try CaptionSpeakerIcons.bitmap(plan, icons: files, showing: Set(ids))
    }
    return try await CaptionExport.export(
      movie: movie, plan: shown, ranges: ranges, fill: fill(for: plan.style),
      speakerFills: plan.speakerFills(gradient: gradient),
      nameplate: CaptionUnderlay.bitmap(
        nameplate: bitmap, icons: icons, width: plan.width, height: plan.height),
      output: output,
      codec: codec, allowBurned: overBurned, progress: progress)
  }

  /// The font folders `replay.caption_resource_inputs.font_roots` reads, in its order.
  static let fontRoots = [
    URL(fileURLWithPath: "/Library/Fonts"), URL(fileURLWithPath: "/System/Library/Fonts"),
    FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Fonts"),
  ]

  /// The first `roots/nameFont` whose SHA-256 is the plan's.
  static func font(_ plate: CaptionNameplate, roots: [URL]) throws -> Data {
    var mismatched: [String] = []
    for root in roots {
      let url = root.appendingPathComponent(plate.nameFont)
      guard FileManager.default.fileExists(atPath: url.path) else { continue }
      let data = try read(url)
      if CaptionNameplateRenderer.digest(data) == plate.nameFontSha256 { return data }
      mismatched.append(url.path)
    }
    throw CaptionRenderError.invalid(
      mismatched.isEmpty
        ? "Nameplate font \(plate.nameFont) is in none of \(roots.map(\.path)); pass"
          + " --nameplate-font FILE or --no-nameplate"
        : "Nameplate font \(mismatched.joined(separator: ", ")) is not the plan's"
          + " (SHA-256 \(plate.nameFontSha256)); pass --nameplate-font FILE or --no-nameplate")
  }

  static func read(_ url: URL) throws -> Data {
    do {
      return try Data(contentsOf: url)
    } catch {
      throw CaptionRenderError.invalid("Cannot read \(url.path): \(error.localizedDescription)")
    }
  }
}
