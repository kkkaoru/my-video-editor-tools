import AppKit
import CaptionRender
import SwiftUI

/// 字幕表示 beside the playback controls, with the caption look in a popover; nothing shows
/// for a movie without a caption plan.
struct CaptionControls: View {
    @Bindable var model: CaptionsModel
    @State var showsStyle = false

    var body: some View {
        if model.captions != nil {
            Toggle(isOn: $model.isShown) {
                Label("字幕表示", systemImage: "captions.bubble")
                    .labelStyle(.iconOnly)
                    .font(.title2)
                    .frame(width: Chrome.hit, height: Chrome.hit)
                    .contentShape(Rectangle())
            }
            .toggleStyle(.button)
            .buttonStyle(.borderless)
            .keyboardShortcut("c", modifiers: [])
            .help("C: 字幕プランの字幕を動画の上に表示（焼き込み済みの動画は既定でオフ）")
            .accessibilityLabel("字幕表示")
            .accessibilityIdentifier("captions-shown")
            Button {
                showsStyle = true
            } label: {
                Label("字幕の見た目", systemImage: "paintpalette")
                    .labelStyle(.iconOnly)
                    .font(.title2)
                    .frame(width: Chrome.hit, height: Chrome.hit)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.borderless)
            .help("アイコン表示・グラデーション・字幕の色")
            .accessibilityLabel("字幕の見た目")
            // Opens below the button, so the settings never cover the video above it.
            .popover(isPresented: $showsStyle, arrowEdge: .bottom, content: styleOptions)
        }
        if let issue = model.issue ?? model.placementIssue ?? model.captions?.nameplateIssue
            ?? model.captions?.positionIssue
        {
            Image(systemName: "exclamationmark.triangle")
                .foregroundStyle(.orange)
                .help(issue)
                .accessibilityLabel("字幕の問題: \(issue)")
        }
    }
}

extension CaptionControls {
    func styleOptions() -> some View { CaptionStyleOptions(model: model).padding() }

}

struct CaptionStyleOptions: View {
    @Bindable var model: CaptionsModel

    var body: some View {
        Form {
            Toggle("アイコン表示", isOn: $model.showsNameplate)
                .disabled(!model.hasIcons)
                .accessibilityLabel("アイコン表示")
            Toggle("グラデーション", isOn: $model.usesGradient)
                .accessibilityLabel("グラデーション")
            ColorPicker("上の色", selection: colour(\.top, \.topColour), supportsOpacity: false)
                .accessibilityLabel("字幕の上の色")
            ColorPicker("下の色", selection: colour(\.bottom, \.bottomColour), supportsOpacity: false)
                .disabled(!model.usesGradient)
                .accessibilityLabel("字幕の下の色")
            Button("リセット") { model.reset() }
                .accessibilityLabel("字幕の見た目をリセット")
                .accessibilityHint("アイコン・グラデーション・色を字幕プランの設定に戻します")
            Section("位置（動画ごとに保存）") {
                Picker(
                    "位置", selection: Binding(get: { model.layout.mode }, set: { model.place($0) })
                ) {
                    ForEach(Self.modes) { choice in
                        Text(choice.label).tag(choice.mode)
                            .selectionDisabled(choice.mode == .aboveFace && !model.hasFaces)
                    }
                }
                // Segments, never a pop-up menu: highlighting a menu item whose title is a
                // non-ASCII Swift string traps in the Swift runtime (crash of 2026-11-03).
                .pickerStyle(.segmented)
                .accessibilityLabel("字幕の位置")
                if !model.hasFaces {
                    Text("顔の位置データ（faces.json）がないため「顔の上」は使えません")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                HStack {
                    ForEach(Self.nudges) { nudge in
                        Button {
                            model.nudge(x: nudge.x, y: nudge.y)
                        } label: {
                            Image(systemName: nudge.symbol)
                        }
                        .accessibilityLabel(nudge.label)
                        .help(nudge.label)
                    }
                }
                TextField("横 px", value: offsetX, format: .number)
                    .accessibilityLabel("字幕の横のずらし（ピクセル、右が正）")
                TextField("縦 px", value: offsetY, format: .number)
                    .accessibilityLabel("字幕の縦のずらし（ピクセル、下が正）")
                Button("位置をリセット") { model.resetPosition() }
                    .accessibilityHint("字幕プランの位置に戻し、ずらしを消します")
            }
            if !model.speakers.isEmpty {
                Section("話者ごとの位置") {
                    ForEach(model.speakers, id: \.id) { speaker in speakerPosition(speaker) }
                }
            }
        }
        .frame(width: 420)
    }

    struct Choice: Identifiable {
        let mode: CaptionLayout.Mode
        let label: String
        var id: String { label }
    }

    struct Nudge: Identifiable {
        let label: String
        let symbol: String
        let x: Double
        let y: Double
        var id: String { label }
    }

    static let modes = [
        Choice(mode: .plan, label: "今の位置"), Choice(mode: .aboveFace, label: "顔の上"),
        Choice(mode: .bottom, label: "下部"), Choice(mode: .top, label: "上部"),
        Choice(mode: .center, label: "中央"),
    ]
    /// Canvas pixels per press.
    static let step = 10.0
    static let nudges = [
        Nudge(label: "字幕を左へ10px", symbol: "arrow.left", x: -step, y: 0),
        Nudge(label: "字幕を右へ10px", symbol: "arrow.right", x: step, y: 0),
        Nudge(label: "字幕を上へ10px", symbol: "arrow.up", x: 0, y: -step),
        Nudge(label: "字幕を下へ10px", symbol: "arrow.down", x: 0, y: step),
    ]

    /// A speaker's own kind of position (全体と同じ: the whole layout's) and offsets.
    func speakerPosition(_ speaker: CaptionSpeaker) -> some View {
        let options = model.speakerOptions(speaker.id)
        return VStack(alignment: .leading) {
            Picker(
                speaker.name,
                selection: Binding(
                    get: { options?.mode }, set: { model.place(speaker.id, $0) })
            ) {
                Text("全体と同じ").tag(CaptionLayout.Mode?.none)
                ForEach(Self.modes) { choice in
                    Text(choice.label).tag(Optional(choice.mode))
                        .selectionDisabled(choice.mode == .aboveFace && !model.hasFaces)
                }
            }
            .pickerStyle(.segmented)
            .accessibilityLabel("\(speaker.name)の字幕の位置")
            HStack {
                TextField(
                    "横 px",
                    value: Binding(
                        get: { model.speakerOptions(speaker.id)?.offsetX ?? 0 },
                        set: { model.setOffset(speaker.id, x: $0) }), format: .number
                )
                .accessibilityLabel("\(speaker.name)の字幕の横のずらし（ピクセル）")
                TextField(
                    "縦 px",
                    value: Binding(
                        get: { model.speakerOptions(speaker.id)?.offsetY ?? 0 },
                        set: { model.setOffset(speaker.id, y: $0) }), format: .number
                )
                .accessibilityLabel("\(speaker.name)の字幕の縦のずらし（ピクセル、下が正）")
                Button("戻す") { model.resetPosition(speaker.id) }
                    .accessibilityLabel("\(speaker.name)の位置を全体と同じに戻す")
            }
        }
    }

    /// The saved horizontal offset in a numeric field; a typed value is saved.
    var offsetX: Binding<Double> {
        Binding(get: { model.layout.offsetX ?? 0 }, set: { model.setOffset(x: $0) })
    }

    /// The saved vertical offset (+ down) in a numeric field; a typed value is saved.
    var offsetY: Binding<Double> {
        Binding(get: { model.layout.offsetY ?? 0 }, set: { model.setOffset(y: $0) })
    }

    /// The picker's colour of `shown`, saved as an `#RRGGBB` override in `stored`.
    func colour(
        _ shown: KeyPath<CaptionsModel, String>,
        _ stored: ReferenceWritableKeyPath<CaptionsModel, String?>
    ) -> Binding<Color> {
        Binding(
            get: { Color(nsColor: CaptionsModel.colour(model[keyPath: shown]) ?? .white) },
            set: { picked in
                guard let hex = CaptionsModel.hex(NSColor(picked)) else { return }
                model[keyPath: stored] = hex
            })
    }
}

/// One chip per speaker under the playback controls (plan version 2): each shows or hides that
/// speaker's captions and icon.
struct SpeakerChips: View {
    @Bindable var model: CaptionsModel

    struct Chip: Identifiable {
        /// Characters of a name shown on its chip; longer names end in "…".
        static let shownCharacters = 8

        let id: String
        let name: String
        let colour: String

        var label: String {
            name.count > Self.shownCharacters
                ? String(name.prefix(Self.shownCharacters)) + "…" : name
        }
    }

    var body: some View {
        if !chips.isEmpty {
            HStack(spacing: 8) {
                ForEach(chips) { chip in
                    Toggle(
                        isOn: Binding(
                            get: { model.isShowing(chip.id) }, set: { model.show(chip.id, $0) })
                    ) {
                        Label(chip.label, systemImage: "circle.fill")
                            .foregroundStyle(
                                Color(nsColor: CaptionsModel.colour(chip.colour) ?? .gray))
                    }
                    .toggleStyle(.button)
                    .controlSize(.large)
                    // Its whole label, never squeezed by the row; a long name shows in full here.
                    .fixedSize()
                    .help("\(chip.name)の字幕を表示/非表示")
                    .accessibilityLabel("話者 \(chip.name) の字幕を表示")
                }
                Spacer()
            }
        }
    }

    /// One chip per speaker in lane order, then 未割当 when some speech has no speaker.
    var chips: [Chip] {
        model.speakers.map { Chip(id: $0.id, name: $0.name, colour: $0.colorBottom) }
            + (model.hasUnassigned
                ? [Chip(id: CaptionPlan.unassigned, name: "未割当", colour: "#9A9A9A")] : [])
    }
}
