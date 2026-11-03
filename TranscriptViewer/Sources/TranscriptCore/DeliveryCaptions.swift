import CaptionRender
import CoreGraphics
import Foundation

public enum DeliveryCaptionsError: Error, Equatable, LocalizedError {
    case unreadable(String)
    case otherMovie(String)

    public var errorDescription: String? {
        switch self {
        case .unreadable(let detail): "字幕プランを読み込めません: \(detail)"
        case .otherMovie(let movie): "字幕プランが別の動画（\(movie)）のものです"
        }
    }
}

/// A delivery's caption plan (`<movie stem>.captions.json`, docs/caption-plan.md) with its
/// nameplate drawn once, and its caption position (`.caption-layout.json`, `.faces.json`),
/// for drawing the captions live over the movie and exporting them.
public final class DeliveryCaptions: Sendable {
    /// The pipeline's `caption_resource_inputs.font_roots()`, searched in this order.
    public static let fontRoots = [
        URL(fileURLWithPath: "/Library/Fonts"), URL(fileURLWithPath: "/System/Library/Fonts"),
        URL.homeDirectory.appending(path: "Library/Fonts"),
    ]
    static let maximumBytes = 64 << 20

    public let plan: CaptionPlan
    /// The full-canvas nameplate (straight alpha) to draw under the captions; nil when the plan
    /// has none or it is burned.
    public let nameplate: MotionCaptionBitmap?
    /// Why a nameplate or speaker icon the plan asks for could not be drawn (the captions
    /// still are).
    public let nameplateIssue: String?
    /// The speakers' icon files (version 2), by speaker id, read beside the plan.
    public let speakerIcons: [String: Data]
    /// The saved caption position; the plan's own when there is none.
    public let layout: CaptionLayout
    /// The face track `aboveFace` needs; nil when there is none.
    public let faces: CaptionFaces?
    /// Why a saved position or face track was not used (the plan's positions are).
    public let positionIssue: String?

    init(
        plan: CaptionPlan, nameplate: MotionCaptionBitmap?,
        nameplateIssue: String?, position: Position, icons: (files: [String: Data], issue: String?)
    ) {
        self.plan = plan
        self.nameplate = nameplate
        self.nameplateIssue = nameplateIssue ?? icons.issue
        speakerIcons = icons.files
        layout = position.layout
        faces = position.faces
        positionIssue = position.issue
    }

    struct Position {
        var layout = CaptionLayout.plan
        var faces: CaptionFaces?
        var issue: String?
    }

    /// `movie`'s sibling `<stem><suffix>`.
    public static func sibling(of movie: URL, _ suffix: String) -> URL {
        movie.deletingLastPathComponent().appending(
            path: movie.deletingPathExtension().lastPathComponent + suffix)
    }

    public static func planURL(for movie: URL) -> URL { sibling(of: movie, ".captions.json") }

    /// A sibling's bytes; nil when absent. A linked or oversized file throws.
    static func read(_ url: URL) throws -> Data? {
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        let values = try url.resourceValues(forKeys: [.isSymbolicLinkKey, .fileSizeKey])
        guard values.isSymbolicLink == false, let size = values.fileSize, size <= maximumBytes
        else { throw DeliveryCaptionsError.unreadable(url.lastPathComponent) }
        return try Data(contentsOf: url)
    }

    /// The movie's saved position and face track; an unusable one is reported and left out.
    static func position(movie: URL, plan: CaptionPlan) -> Position {
        var position = Position()
        do {
            if let data = try read(sibling(of: movie, CaptionLayout.suffix)) {
                position.layout = try CaptionLayout.decode(data)
            }
        } catch {
            position.issue = "字幕位置を読めません: \(error)"
        }
        do {
            if let data = try read(sibling(of: movie, CaptionFaces.suffix)) {
                let faces = try CaptionFaces.decode(data)
                guard faces.width == plan.width, faces.height == plan.height else {
                    throw DeliveryCaptionsError.unreadable("顔の位置が別の画面サイズのものです")
                }
                position.faces = faces
            }
        } catch {
            position.issue = "顔の位置を読めません: \(error)"
        }
        return position
    }

    /// The plan beside `movie`; nil when there is none. A plan that is linked, oversized,
    /// invalid or written for another movie throws.
    public static func load(movie: URL, fontRoots: [URL] = fontRoots) throws -> DeliveryCaptions? {
        let url = planURL(for: movie)
        let plan: CaptionPlan
        do {
            guard let data = try read(url) else { return nil }
            plan = try CaptionPlan.decode(data)
        } catch let error as DeliveryCaptionsError {
            throw error
        } catch {
            throw DeliveryCaptionsError.unreadable("\(url.lastPathComponent): \(error)")
        }
        guard plan.movie == movie.lastPathComponent else {
            throw DeliveryCaptionsError.otherMovie(plan.movie)
        }
        let position = position(movie: movie, plan: plan)
        let icons = speakerIcons(plan, beside: url)
        guard let plate = plan.nameplate, !plate.burned else {
            return DeliveryCaptions(
                plan: plan, nameplate: nil, nameplateIssue: nil, position: position, icons: icons)
        }
        do {
            let bitmap = try CaptionNameplateRenderer.bitmap(
                plate,
                icon: Data(contentsOf: url.deletingLastPathComponent().appending(path: plate.icon)),
                font: Data(contentsOf: font(named: plate.nameFont, in: fontRoots)),
                canvasWidth: plan.width, canvasHeight: plan.height)
            return DeliveryCaptions(
                plan: plan, nameplate: bitmap, nameplateIssue: nil,
                position: position, icons: icons)
        } catch {
            return DeliveryCaptions(
                plan: plan, nameplate: nil, nameplateIssue: "アイコンを描けません: \(error)",
                position: position, icons: icons)
        }
    }

    /// The icon files the plan's speakers name; the speakers whose file is unusable are left
    /// out and reported.
    static func speakerIcons(_ plan: CaptionPlan, beside url: URL) -> (
        files: [String: Data], issue: String?
    ) {
        var files: [String: Data] = [:]
        var missing: [String] = []
        for speaker in plan.speakers ?? [] {
            guard let icon = speaker.icon else { continue }
            do {
                guard let data = try read(url.deletingLastPathComponent().appending(path: icon))
                else { throw DeliveryCaptionsError.unreadable(icon) }
                files[speaker.id] = data
            } catch {
                missing.append(speaker.name)
            }
        }
        return (files, missing.isEmpty ? nil : "話者アイコンを読めません: \(missing.joined(separator: "、"))")
    }

    /// The first root holding a file of that name, as the pipeline resolves the badge font.
    static func font(named name: String, in roots: [URL]) throws -> URL {
        guard
            let url = roots.map({ $0.appending(path: name) }).first(where: {
                FileManager.default.fileExists(atPath: $0.path)
            })
        else { throw DeliveryCaptionsError.unreadable("フォント \(name) がありません") }
        return url
    }
}

/// Where the plan's canvas-pixel captions lie over the displayed video.
public enum CaptionGeometry {
    /// Absorbs floating error in a time that lands exactly on a frame boundary.
    static let frameEpsilon = 1e-6

    /// The delivery frame showing at `seconds` (the delivery movie starts at zero).
    public static func frame(atSeconds seconds: Double, rate: Int) -> Int {
        guard seconds.isFinite, seconds > 0 else { return 0 }
        return Int((seconds * Double(rate) + frameEpsilon).rounded(.down))
    }

    /// `rect` (canvas pixels, origin top left) inside `video`, the movie's displayed rect in a
    /// view whose origin is at the bottom left unless `flipped`.
    public static func frame(
        of rect: CGRect, canvas: CGSize, in video: CGRect, flipped: Bool
    ) -> CGRect {
        let scaleX = video.width / canvas.width
        let scaleY = video.height / canvas.height
        let y = flipped ? video.minY + rect.minY * scaleY : video.maxY - rect.maxY * scaleY
        return CGRect(
            x: video.minX + rect.minX * scaleX, y: y, width: rect.width * scaleX,
            height: rect.height * scaleY)
    }
}

extension VideoTranscript {
    /// Where this movie's caption plan would be; it may not exist.
    public var captionPlanURL: URL { DeliveryCaptions.planURL(for: movieURL) }
}

/// Writes a movie's caption position (`<stem>.caption-layout.json`) as
/// `replay.caption_layout_file.write` does: validated, one-space indented JSON in its key
/// order, replaced atomically by a private (0600) file.
public enum CaptionLayoutFile {
    public static func url(for movie: URL) -> URL {
        DeliveryCaptions.sibling(of: movie, CaptionLayout.suffix)
    }

    /// Replaces the movie's layout with `layout`, which carries every option it keeps (the
    /// viewer edits the layout it read, so options it does not show stay).
    public static func write(_ layout: CaptionLayout, movie: URL) throws {
        let target = url(for: movie)
        let data = Data(try text(layout).utf8)
        _ = try CaptionLayout.decode(data)
        let staging = target.deletingLastPathComponent().appending(
            path: ".\(target.lastPathComponent).\(UUID().uuidString)")
        guard
            FileManager.default.createFile(
                atPath: staging.path, contents: nil, attributes: [.posixPermissions: 0o600])
        else { throw DeliveryCaptionsError.unreadable(staging.lastPathComponent) }
        do {
            let handle = try FileHandle(forWritingTo: staging)
            defer { handle.closeFile() }
            try handle.write(contentsOf: data)
            try handle.synchronize()
            guard rename(staging.path, target.path) == 0 else {
                throw CocoaError(.fileWriteUnknown, userInfo: [NSFilePathErrorKey: target.path])
            }
        } catch {
            try FileManager.default.removeItem(at: staging)
            throw error
        }
    }

    /// `json.dumps(layout, indent=1)` in `replay.caption_layout_file`'s key order (speakers
    /// by name).
    static func text(_ layout: CaptionLayout) throws -> String {
        var lines =
            [" \"version\": \(layout.version)", " \"mode\": \"\(layout.mode.rawValue)\""]
            + members(
                [
                    ("offsetX", layout.offsetX), ("offsetY", layout.offsetY),
                    ("margin", layout.margin), ("faceGap", layout.faceGap),
                    ("minScore", layout.minScore), ("maxDrift", layout.maxDrift),
                ], indent: " ")
        if let speakers = layout.speakers {
            let entries = try speakers.sorted { $0.key < $1.key }.map { name, options in
                let fields =
                    (options.mode.map { ["   \"mode\": \"\($0.rawValue)\""] } ?? [])
                    + members(
                        [
                            ("offsetX", options.offsetX), ("offsetY", options.offsetY),
                            ("margin", options.margin), ("faceGap", options.faceGap),
                            ("minScore", options.minScore), ("maxDrift", options.maxDrift),
                        ], indent: "   ")
                return "  \(try quoted(name)): " + object(fields, closing: "  ")
            }
            lines.append(" \"speakers\": " + object(entries, closing: " "))
        }
        return object(lines, closing: "") + "\n"
    }

    static func members(_ values: [(String, Double?)], indent: String) -> [String] {
        values.compactMap { key, value in value.map { "\(indent)\"\(key)\": \($0)" } }
    }

    /// A JSON object of `lines`, `{}` when empty, as `json.dumps` indents it.
    static func object(_ lines: [String], closing: String) -> String {
        lines.isEmpty ? "{}" : "{\n" + lines.joined(separator: ",\n") + "\n\(closing)}"
    }

    static func quoted(_ name: String) throws -> String {
        String(decoding: try JSONEncoder().encode(name), as: UTF8.self)
    }
}
