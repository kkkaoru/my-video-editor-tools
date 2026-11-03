import AppKit
import SwiftUI
import TranscriptCore
import UniformTypeIdentifiers

/// 書き出し: ranges of the open movie, marked at the playhead, typed in milliseconds or taken
/// from a clip, put in any order by dragging, previewed, and exported as one movie with the
/// captions as the player shows them.
struct ExportPanel: View {
    @Bindable var session: PlaybackSession
    var savePanel: () -> NSSavePanel = { NSSavePanel() }
    @State var showsClips = false

    var model: ExportModel { session.export }

    var body: some View {
        @Bindable var model = session.export
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Button("開始をマーク", systemImage: "arrow.right.to.line") {
                    model.markStart(at: session.currentSeconds)
                }
                .help("現在位置を範囲の開始にします")
                Button("終了をマーク", systemImage: "arrow.left.to.line") {
                    model.markEnd(at: session.currentSeconds)
                }
                .disabled(model.pendingStart == nil)
                .help("開始から現在位置までを範囲に追加します")
                if let start = model.pendingStart {
                    Text("開始 \(PlaybackClock.label(start))")
                        .font(.callout.monospacedDigit())
                        .accessibilityLabel("マークした開始 \(PlaybackClock.label(start))")
                }
                // A popover of buttons, not a Menu: highlighting a menu item whose title is a
                // non-ASCII Swift string traps in the Swift runtime (crash of 2026-11-03).
                Button("切り抜きから追加", systemImage: "scissors", action: showClips)
                    .disabled(session.clips.clips.isEmpty)
                    .help("切り抜きの範囲をそのまま追加します")
                    .popover(isPresented: $showsClips, content: clipList)
                Spacer()
            }
            List {
                ForEach($model.ranges) { $range in
                    row($range, number: model.ranges.prefix { $0.id != range.id }.count + 1)
                }
                .onMove { model.move(from: $0, to: $1) }
            }
            .frame(minHeight: 120)
            .accessibilityLabel("書き出す範囲（ドラッグで並べ替え）")
            HStack(spacing: 8) {
                Text("\(model.ranges.count) 範囲・合計 \(PlaybackClock.label(model.duration))")
                    .font(.callout.monospacedDigit())
                Button("順に再生", systemImage: "play") {
                    session.play(model.preview(model.ranges), loop: false)
                }
                .disabled(model.ranges.isEmpty)
                if session.captions.captions?.plan.burned == true, session.captions.isShown {
                    Toggle("二重焼き込みを許可", isOn: $model.allowsBurned)
                        .help("この動画は字幕が焼き込み済みです。オンにすると字幕がもう一度書き込まれます")
                }
                Spacer()
                if model.isExporting {
                    ProgressView(value: model.progress)
                        .frame(width: 160)
                        .accessibilityLabel("書き出しの進捗")
                    Button("キャンセル", role: .cancel) { model.cancel() }
                } else {
                    Button("書き出し…", systemImage: "square.and.arrow.up") { export() }
                        .disabled(model.problem(captions: session.captions) != nil)
                        .help(model.problem(captions: session.captions) ?? "範囲を並べた順に連結して書き出します")
                }
            }
            if let status = model.status {
                HStack {
                    Text(status).font(.callout).foregroundStyle(.secondary)
                    if model.finished != nil {
                        Button("Finderで表示") { model.revealFinished() }
                    }
                }
            } else if let problem = model.problem(captions: session.captions), !model.ranges.isEmpty
            {
                Text(problem).font(.callout).foregroundStyle(.orange)
            }
        }
    }

    func showClips() { showsClips = true }

    func clipList() -> some View {
        VStack(alignment: .leading) { clipItems(session.clips.clips) }.padding()
    }

    func clipItems(_ clips: [ClipPoint]) -> some View {
        ForEach(clips) { clip in
            Button(clip.title) {
                model.add(clip)
                showsClips = false
            }
        }
    }

    func row(_ range: Binding<ExportRange>, number: Int) -> some View {
        HStack(spacing: 8) {
            Text("\(number)").font(.callout.monospacedDigit()).frame(width: 24)
            TextField("開始 ms", value: range.startMilliseconds, format: .number)
                .frame(width: 110)
                .accessibilityLabel("範囲\(number)の開始（ミリ秒）")
            Text("〜")
            TextField("終了 ms", value: range.endMilliseconds, format: .number)
                .frame(width: 110)
                .accessibilityLabel("範囲\(number)の終了（ミリ秒）")
            Text(PlaybackClock.range(range.wrappedValue.start, range.wrappedValue.end))
                .font(.callout.monospacedDigit())
                .foregroundStyle(.secondary)
            Spacer()
            Button {
                session.play(model.preview([range.wrappedValue]), loop: false)
            } label: {
                Image(systemName: "play.circle")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("範囲\(number)を再生")
            Button {
                model.remove(range.wrappedValue)
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("範囲\(number)を削除")
        }
    }

    func export() {
        guard let movie = model.movie else { return }
        let name = movie.deletingPathExtension().lastPathComponent + "-export.mp4"
        let panel = savePanel()
        panel.allowedContentTypes = [.mpeg4Movie]
        panel.nameFieldStringValue = name
        panel.canCreateDirectories = true
        guard panel.runModal() == .OK, let output = panel.url else { return }
        model.export(to: output, captions: session.captions)
    }
}
