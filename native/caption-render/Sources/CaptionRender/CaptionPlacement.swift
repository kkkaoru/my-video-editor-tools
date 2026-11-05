import Foundation

/// A delivery's caption position (`<movie stem>.caption-layout.json`), as
/// `replay.caption_layout_file` validates it. Absent options take the documented defaults.
/// Version 2 adds `speakers`: per-speaker options over the top-level ones.
public struct CaptionLayout: Codable, Equatable, Sendable {
  public enum Mode: String, Codable, Sendable {
    case plan
    case aboveFace
    case bottom
    case top
    case center
  }

  public static let suffix = ".caption-layout.json"
  /// The layout of a movie that has none: the plan's own positions.
  public static let plan = CaptionLayout(mode: .plan)

  public let version: Int
  public let mode: Mode
  public let offsetX: Double?
  public let offsetY: Double?
  public let margin: Double?
  public let faceGap: Double?
  public let minScore: Double?
  public let maxDrift: Double?
  /// Version 2: options of a speaker's titles, key by key over the top-level ones.
  public let speakers: [String: SpeakerOptions]?

  /// A speaker's layout options; nil keeps the layout's own.
  public struct SpeakerOptions: Codable, Equatable, Sendable {
    public let mode: Mode?
    public let offsetX: Double?
    public let offsetY: Double?
    public let margin: Double?
    public let faceGap: Double?
    public let minScore: Double?
    public let maxDrift: Double?

    public init(
      mode: Mode? = nil, offsetX: Double? = nil, offsetY: Double? = nil, margin: Double? = nil,
      faceGap: Double? = nil, minScore: Double? = nil, maxDrift: Double? = nil
    ) {
      self.mode = mode
      self.offsetX = offsetX
      self.offsetY = offsetY
      self.margin = margin
      self.faceGap = faceGap
      self.minScore = minScore
      self.maxDrift = maxDrift
    }
  }

  /// Version 1 without `speakers`, version 2 with them.
  public init(
    mode: Mode, offsetX: Double? = nil, offsetY: Double? = nil, margin: Double? = nil,
    faceGap: Double? = nil, minScore: Double? = nil, maxDrift: Double? = nil,
    speakers: [String: SpeakerOptions]? = nil
  ) {
    version = speakers == nil ? 1 : 2
    self.speakers = speakers
    self.mode = mode
    self.offsetX = offsetX
    self.offsetY = offsetY
    self.margin = margin
    self.faceGap = faceGap
    self.minScore = minScore
    self.maxDrift = maxDrift
  }

  static let required: Set<String> = ["version", "mode"]
  static let options: Set<String> = [
    "offsetX", "offsetY", "margin", "faceGap", "minScore", "maxDrift",
  ]

  /// The layout in `data`, validated (unknown keys are refused, as the Python reader does).
  public static func decode(_ data: Data) throws -> CaptionLayout {
    let shape = try JSONShape.of(data)
    try shape.requireKeys(
      required, allowed: required.union(options).union(["speakers"]), in: "A caption layout")
    if case .object(let speakers) = shape["speakers"] {
      for entry in speakers.values {
        try entry.requireKeys([], allowed: options.union(["mode"]), in: "A speaker's layout")
      }
    }
    let layout: CaptionLayout
    do {
      layout = try JSONDecoder().decode(CaptionLayout.self, from: data)
    } catch {
      throw CaptionRenderError.invalid("Unreadable caption layout: \(error)")
    }
    try layout.validate()
    return layout
  }

  /// This layout with the given options replaced (nil keeps each one).
  public func overriding(mode: Mode? = nil, offsetX: Double? = nil, offsetY: Double? = nil)
    -> CaptionLayout
  {
    CaptionLayout(
      mode: mode ?? self.mode, offsetX: offsetX ?? self.offsetX, offsetY: offsetY ?? self.offsetY,
      margin: margin, faceGap: faceGap, minScore: minScore, maxDrift: maxDrift,
      speakers: speakers)
  }

  public func validate() throws {
    let entries = (speakers ?? [:]).values.map {
      ($0.offsetX, $0.offsetY, $0.margin, $0.faceGap, $0.minScore, $0.maxDrift)
    }
    let sets = [(offsetX, offsetY, margin, faceGap, minScore, maxDrift)] + entries
    let valid = sets.allSatisfy { set in
      let values = [set.0, set.1, set.2, set.3, set.4, set.5].compactMap { $0 }
      return values.allSatisfy(\.isFinite)
        && [set.2, set.3, set.5].compactMap({ $0 }).allSatisfy({ $0 >= 0 })
        && (0...1).contains(set.4 ?? 0.5)
    }
    guard (1...2).contains(version), version == 2 || speakers == nil, valid else {
      throw CaptionRenderError.invalid("Invalid caption layout")
    }
  }
}

/// A delivery's face track (`<movie stem>.faces.json`, `replay.caption_faces`). Version 1: the
/// highest-scoring face of each observed delivery frame and how many faces it showed;
/// version 2: every face of the frame (`boxes`).
public struct CaptionFaces: Codable, Equatable, Sendable {
  public struct Box: Codable, Equatable, Sendable {
    public let x: Double
    public let y: Double
    public let w: Double
    public let h: Double
    public let score: Double
  }

  public struct Sample: Codable, Equatable, Sendable {
    public let frame: Int
    /// Version 1.
    public let faceCount: Int?
    /// Version 1; nil: observed, no face.
    public let box: Box?
    /// Version 2: every face at or above the YOLO floor (empty: observed, no face).
    public let boxes: [Box]?
  }

  public static let suffix = ".faces.json"

  public let version: Int
  public let clock: String
  public let rate: Int
  public let width: Int
  public let height: Int
  /// Strictly increasing delivery frames.
  public let samples: [Sample]

  /// The face track in `data`, validated (exact key sets, as `replay.caption_faces`).
  public static func decode(_ data: Data) throws -> CaptionFaces {
    let shape = try JSONShape.of(data)
    try shape.requireKeys(
      ["version", "clock", "rate", "width", "height", "source", "samples"], in: "A face track")
    try shape["source"].requireKeys(
      ["model", "modelSha256", "file", "sha256"], in: "A face track source")
    let version2 = shape["samples"].items.contains { $0.has("boxes") }
    for sample in shape["samples"].items {
      try sample.requireKeys(
        version2 ? ["frame", "boxes"] : ["frame", "faceCount", "box"], in: "A face sample")
      for box in [sample["box"]] + sample["boxes"].items {
        try box.requireKeys(["x", "y", "w", "h", "score"], in: "A face box")
      }
    }
    let faces: CaptionFaces
    do {
      faces = try JSONDecoder().decode(CaptionFaces.self, from: data)
    } catch {
      throw CaptionRenderError.invalid("Unreadable face track: \(error)")
    }
    try faces.validate()
    return faces
  }

  public func validate() throws {
    let shaped = samples.allSatisfy { sample in
      guard version == 1 else { return sample.boxes != nil && sample.faceCount == nil }
      guard let count = sample.faceCount, sample.boxes == nil else { return false }
      return count >= 0 && (sample.box == nil) == (count == 0)
    }
    let boxes = samples.flatMap { ($0.boxes ?? []) + [$0.box].compactMap { $0 } }
    guard (1...2).contains(version), clock == CaptionPlan.clock, rate > 0, width > 0, height > 0,
      zip(samples, samples.dropFirst()).allSatisfy({ $0.frame < $1.frame }), shaped,
      boxes.allSatisfy({
        [$0.x, $0.y, $0.w, $0.h, $0.score].allSatisfy(\.isFinite) && $0.w > 0 && $0.h > 0
      })
    else { throw CaptionRenderError.invalid("Invalid face track") }
  }
}

/// Resolves a plan's caption positions under a layout and face track, by the rule in
/// `docs/caption-plan.md` ("Caption position") that `replay.caption_layout_file.resolve`, the
/// web viewer and the Mac viewer share. It is arithmetic on the calibrated style-20 box (no
/// glyphs are measured), so a whole plan resolves in microseconds per title.
public enum CaptionPlacement {
  /// Where a title's vertical position came from.
  public enum Basis: String, Codable, Sendable {
    case plan
    case mode
    case face
    case held
  }

  public struct Placement: Equatable, Sendable {
    public let x: Double
    public let y: Double
    public let basis: Basis
  }

  static let padding = 24.0
  /// What `MotionCaptionRenderer` draws past the rim on each side.
  static let rimAntialias = 2.0
  static let emAscent = 1.16
  static let emAdvance = 1.448
  /// One line's native measurement at the style-20 calibration: 150 px at 82.9558 pt.
  static let emLineHeight = 150 / 82.95580110497238
  static let defaultMargin = 120.0
  static let defaultFaceGap = 60.0
  static let defaultMinScore = 0.5
  static let defaultMaxDrift = 150.0

  /// The plan with every title moved to its resolved position. `faces` is needed only by
  /// `aboveFace`.
  public static func resolve(plan: CaptionPlan, layout: CaptionLayout, faces: CaptionFaces?)
    throws -> CaptionPlan
  {
    let resolved = try placements(plan: plan, layout: layout, faces: faces)
    return try plan.replacing(
      titles: zip(plan.titles, resolved).map { $0.moved(x: $1.x, y: $1.y) })
  }

  /// One title's effective options: the layout's, with its speaker's laid over them.
  struct Options {
    let mode: CaptionLayout.Mode
    let offsetX: Double
    let offsetY: Double
    let margin: Double
    let faceGap: Double
    let minScore: Double
    let maxDrift: Double

    init(_ layout: CaptionLayout, speaker: String?) {
      let own = speaker.flatMap { layout.speakers?[$0] }
      mode = own?.mode ?? layout.mode
      offsetX = own?.offsetX ?? layout.offsetX ?? 0
      offsetY = own?.offsetY ?? layout.offsetY ?? 0
      margin = own?.margin ?? layout.margin ?? defaultMargin
      faceGap = own?.faceGap ?? layout.faceGap ?? defaultFaceGap
      minScore = own?.minScore ?? layout.minScore ?? defaultMinScore
      maxDrift = own?.maxDrift ?? layout.maxDrift ?? defaultMaxDrift
    }
  }

  /// The gap between stacked unassigned titles (`speaker_subtitles.STACK_GAP`).
  static let stackGap = 12.0

  /// Each title's resolved Motion position and its basis.
  public static func placements(plan: CaptionPlan, layout: CaptionLayout, faces: CaptionFaces?)
    throws -> [Placement]
  {
    try plan.validate()
    try layout.validate()
    let ids = Set((plan.speakers ?? []).map(\.id))
    guard Set((layout.speakers ?? [:]).keys).isSubset(of: ids) else {
      throw CaptionRenderError.invalid("The caption layout names a speaker the plan lacks")
    }
    let height = Double(plan.height)
    let width = Double(plan.width)
    var observed: [String: [(frame: Int, top: Double)]] = [:]
    var stack: [(end: Int, top: Double, bottom: Double)] = []
    var result: [Placement] = []
    for title in plan.titles {
      let speaker = plan.version == 2 ? title.speaker : nil
      let options = Options(layout, speaker: speaker)
      let size = (title.style ?? plan.style).fontSize
      let lines = Double(title.text.count { $0 == "\n" } + 1)
      let top = height / 2 - title.y - padding - emAscent * size
      let box = emLineHeight * size + (lines - 1) * emAdvance * size
      var basis = Basis.mode
      var placed: Double
      switch options.mode {
      case .plan: (basis, placed) = (.plan, top)
      case .top: placed = options.margin
      case .bottom: placed = height - options.margin - box
      case .center: placed = (height - box) / 2
      case .aboveFace:
        let key = speaker ?? ""
        let tops =
          try observed[key]
          ?? faceTops(plan: plan, speaker: speaker, minScore: options.minScore, faces: faces)
        observed[key] = tops
        if let face = faceTop(tops, title: title, maxDrift: options.maxDrift) {
          basis = face.held ? .held : .face
          placed = max(
            options.margin, min(height - options.margin, face.top - options.faceGap) - box)
        } else {
          (basis, placed) = (.plan, top)
        }
      }
      placed = max(0, min(placed + options.offsetY, height - box))
      if plan.version == 2, speaker == nil {
        placed = stacked(placed, box: box, title: title, stack: &stack)
      }
      let x = insideSafeZone(
        max(-width / 2, min(title.x + options.offsetX, width / 2)), title: title, plan: plan)
      result.append(
        Placement(x: x, y: height / 2 - (placed + padding + emAscent * size), basis: basis))
    }
    return result
  }

  /// A centred title's position with its estimated block between the plan's margins; a block wider
  /// than they leave sits in their middle. The estimate needs no installed font, so every reader
  /// of the plan agrees (`docs/caption-plan.md` "Safe zone").
  static func insideSafeZone(_ x: Double, title: MotionCaptionTitle, plan: CaptionPlan) -> Double {
    guard let left = plan.safeLeft, let right = plan.safeRight, title.alignment == nil else {
      return x
    }
    let width = Double(plan.width)
    let (low, high) = (Double(left) - width / 2, width / 2 - Double(right))
    let half = estimatedBlockWidth(title, style: title.style ?? plan.style) / 2
    return high - low <= 2 * half ? (low + high) / 2 : max(low + half, min(x, high - half))
  }

  /// The widest line at 0.6 em per ASCII character, 0.5 em per half-width form and 1 em per other
  /// character, with the rim on both sides.
  static func estimatedBlockWidth(_ title: MotionCaptionTitle, style: MotionCaptionStyle) -> Double {
    let widest = title.text.split(separator: "\n", omittingEmptySubsequences: false)
      .map { line in
        line.unicodeScalars.reduce(0.0) { total, scalar in
          total + (scalar.isASCII ? 0.6 : (0xFF61...0xFFDC).contains(scalar.value) ? 0.5 : 1.0)
        }
      }
      .max() ?? 0
    return widest * style.fontSize + 2 * (style.rim + rimAntialias)
  }

  /// An unassigned title's top in the lowest free slot among the overlapping unassigned titles
  /// already placed (`speaker_subtitles._LaneStack`, in top-down canvas pixels).
  static func stacked(
    _ placed: Double, box: Double, title: MotionCaptionTitle,
    stack: inout [(end: Int, top: Double, bottom: Double)]
  ) -> Double {
    stack.removeAll { $0.end <= title.startFrame }
    let candidates = Set([placed + box] + stack.map { $0.top - stackGap }).sorted(by: >)
    let bottom =
      candidates.first { candidate in
        stack.allSatisfy {
          candidate <= $0.top - stackGap || candidate - box >= $0.bottom + stackGap
        }
      } ?? candidates[candidates.count - 1]
    stack.append((title.endFrame, bottom - box, bottom))
    return bottom - box
  }

  /// (frame, top) observations of `speaker`'s face (nil: an unassigned title or a version-1
  /// plan), in frame order: version-1 tracks and unassigned titles take frames showing exactly
  /// one face; a speaker takes frames with exactly one qualifying face in its lane band.
  static func faceTops(
    plan: CaptionPlan, speaker: String?, minScore: Double, faces: CaptionFaces?
  ) throws -> [(frame: Int, top: Double)] {
    guard let faces else {
      throw CaptionRenderError.invalid("Caption mode aboveFace needs the delivery's face track")
    }
    try faces.validate()
    guard (faces.width, faces.height) == (plan.width, plan.height) else {
      throw CaptionRenderError.invalid("The face track is not on the plan's canvas")
    }
    let lane = speaker.map { band(of: $0, plan: plan) }
    return faces.samples.compactMap { sample in
      let found: CaptionFaces.Box?
      if let boxes = sample.boxes {
        let qualifying = boxes.filter { $0.score >= minScore }
        if let lane {
          let inside = qualifying.filter { lane.contains($0.x + $0.w / 2) }
          found = inside.count == 1 ? inside[0] : nil
        } else {
          found = boxes.count == 1 ? qualifying.first : nil
        }
      } else {
        found = sample.faceCount == 1 ? sample.box.flatMap { $0.score >= minScore ? $0 : nil } : nil
      }
      return found.map { (sample.frame, $0.y) }
    }
  }

  /// A speaker's lane band in canvas pixels: from the midpoint with its left neighbour
  /// (inclusive) to the one with its right neighbour (exclusive), open at the ends.
  static func band(of speaker: String, plan: CaptionPlan) -> Range<Double> {
    let centres = (plan.speakers ?? []).map { (id: $0.id, x: Double(plan.width) / 2 + $0.laneX) }
      .sorted { $0.x < $1.x }
    // Validated plans name only their own speakers; an unknown one has no band.
    guard let index = centres.firstIndex(where: { $0.id == speaker }) else { return 0..<0 }
    let low = index == 0 ? -Double.infinity : (centres[index - 1].x + centres[index].x) / 2
    let high =
      index == centres.count - 1 ? Double.infinity : (centres[index].x + centres[index + 1].x) / 2
    return low..<high
  }

  /// The highest face top seen in [startFrame, endFrame] (both inclusive); else the higher of
  /// the nearest ones before and after, held when they differ by at most `maxDrift`.
  static func faceTop(
    _ tops: [(frame: Int, top: Double)], title: MotionCaptionTitle, maxDrift: Double
  ) -> (top: Double, held: Bool)? {
    let low = firstIndex(tops) { $0.frame >= title.startFrame }
    let high = firstIndex(tops) { $0.frame > title.endFrame }
    if low < high { return (tops[low..<high].reduce(.infinity) { min($0, $1.top) }, false) }
    guard low > 0, high < tops.count,
      abs(tops[low - 1].top - tops[high].top) <= maxDrift
    else { return nil }
    return (min(tops[low - 1].top, tops[high].top), true)
  }

  /// The first index whose element satisfies `predicate`, which holds from some index on.
  static func firstIndex(
    _ tops: [(frame: Int, top: Double)], where predicate: ((frame: Int, top: Double)) -> Bool
  ) -> Int {
    var low = 0
    var high = tops.count
    while low < high {
      let middle = (low + high) / 2
      if predicate(tops[middle]) { high = middle } else { low = middle + 1 }
    }
    return low
  }
}
