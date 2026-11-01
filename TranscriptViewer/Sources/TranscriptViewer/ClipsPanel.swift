import SwiftUI
import TranscriptCore

/// The selected video's clip points: each plays its ranges in order and shows its length,
/// band, ranges and generated summary. Reload asks the Worker to compute them again.
struct ClipsPanel: View {
    let model: ClipsModel
    /// Clip, first range, loop.
    let play: (ClipPoint, Int, Bool) -> Void
    /// Seeks the player without playing a clip (a click on a row).
    var seek: (Double) -> Void = { _ in }
    @State private var selectedID: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                Text("切り抜き").font(.headline).accessibilityAddTraits(.isHeader)
                Text(note).foregroundStyle(.secondary)
                Spacer(minLength: 0)
                Button("切り抜きを再計算", action: model.reload)
                    .disabled(!model.canReload)
                    .frame(minHeight: Chrome.hit)
                    .help("このMacの切り抜きモデルで切り抜き箇所と要約を計算し直します（1時間ほどかかります）")
            }
            LazyVStack(alignment: .leading, spacing: 10) {
                ForEach(model.clips) { clip in
                    ClipRow(
                        clip: clip, cursor: model.cursor, play: play,
                        selected: selectedID == clip.id
                    ) {
                        selectedID = clip.id
                        if let first = clip.ranges.first { seek(first.start) }
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    var note: String {
        if let cursor = model.cursor, let range = cursor.range {
            let parts =
                cursor.clip.ranges.count > 1
                ? "・区間\(cursor.index + 1)/\(cursor.clip.ranges.count)" : ""
            return
                "\(cursor.loop ? "ループ再生中" : "再生中"): \(cursor.clip.title)（\(ClipPoint.seconds(cursor.clip.duration))\(parts)、\(ClipPoint.seconds(range.duration))の区間）"
        }
        return Self.note(for: model.state)
    }

    static func note(for state: ClipsModel.State) -> String {
        switch state {
        case .idle: ""
        case .loading: "読み込み中…"
        case .ready(let clips): clips.isEmpty ? "切り抜きに向く区間が見つかりませんでした" : "\(clips.count)件"
        case .pending: "計算中です（Macで1時間ほどかかります）…"
        case .mismatch: "Web版の動画と内容が異なるため表示できません"
        case .missing: "この動画の切り抜きはまだありません（Macで作成されると表示されます）"
        case .noExcitement: "盛り上がりデータがまだないため作成できません（計算して同期した後、Macで作成されます）"
        case .unavailable: "Web版の接続設定がありません"
        case .failed(let message): "読み込めませんでした: \(message)"
        }
    }
}

/// One clip: its title, a second line with where it plays, how long and its band, its summary,
/// then the play buttons and its ranges. A click on the row selects it and seeks to its start.
struct ClipRow: View {
    let clip: ClipPoint
    let cursor: ClipCursor?
    let play: (ClipPoint, Int, Bool) -> Void
    var selected = false
    var select: () -> Void = {}
    @State private var hovering = false

    var playing: Bool { cursor?.clip.id == clip.id }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(clip.title)
                .font(.title3.weight(.semibold))
                .fixedSize(horizontal: false, vertical: true)
            Text(detailLabel)
                .font(.callout.monospacedDigit())
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            Text(clip.summary).fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                Button {
                    play(clip, 0, true)
                } label: {
                    Label("再生", systemImage: "repeat")
                        .frame(minWidth: Chrome.hit, minHeight: Chrome.hit)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel("\(clip.title)をループ再生（\(ClipPoint.seconds(clip.duration))）")
                .help("切り抜きを繰り返し再生します")
                Button {
                    play(clip, 0, false)
                } label: {
                    Label("1回再生", systemImage: "play.fill")
                        .frame(minWidth: Chrome.hit, minHeight: Chrome.hit)
                        .contentShape(Rectangle())
                }
                .accessibilityLabel("\(clip.title)を1回だけ再生")
                ScrollView(.horizontal) {
                    HStack(spacing: 6) {
                        ForEach(Array(clip.ranges.enumerated()), id: \.offset) { index, range in
                            Button(rangeLabel(index, range)) { play(clip, index, true) }
                                .frame(minHeight: Chrome.hit)
                        }
                    }
                }
                .accessibilityLabel("\(clip.title)の区間")
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 10).fill(fill))
        .overlay(
            RoundedRectangle(cornerRadius: 10)
                .strokeBorder(Color.accentColor, lineWidth: selected ? 2 : 0)
        )
        .contentShape(RoundedRectangle(cornerRadius: 10))
        .onHover { hovering = $0 }
        .onTapGesture(perform: select)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    var fill: Color {
        if playing { return Color.accentColor.opacity(0.18) }
        return Color.secondary.opacity(hovering || selected ? 0.16 : 0.08)
    }

    /// Where in the video the clip lies (its earliest start to its latest end: a climax-first clip
    /// plays its later range first), then its length.
    var detailLabel: String {
        guard let start = clip.ranges.map(\.start).min(), let end = clip.ranges.map(\.end).max()
        else { return lengthLabel }
        return "\(ExcitementTimeline.clock(start))–\(ExcitementTimeline.clock(end))・\(lengthLabel)"
    }

    var lengthLabel: String {
        let parts = clip.ranges.count > 1 ? "・\(clip.ranges.count)区間" : ""
        var pattern = ""
        if let label = clip.patternLabel { pattern = "・\(label)" }
        return "\(ClipPoint.seconds(clip.duration))・\(clip.kindLabel)\(pattern)\(parts)"
    }

    func rangeLabel(_ index: Int, _ range: ClipRange) -> String {
        "\(index + 1). \(ExcitementTimeline.clock(range.start))〜（\(ClipPoint.seconds(range.duration))）"
    }
}
