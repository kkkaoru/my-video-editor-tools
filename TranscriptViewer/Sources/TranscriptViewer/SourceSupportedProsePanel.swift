import SwiftUI
import TranscriptCore

/// Speech-backed paragraphs retain the exact selected source and its actionable clock.
struct SourceSupportedProsePanel: View {
    let summary: SourceSupportedProse
    let canSeek: Bool
    let seek: (Double) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(summary.label).font(.headline)
            Text("実際の発話を根拠に文章をまとめています。認識精度と話題の代表性は未確認です。長い発声は読みやすい表記にしています。")
                .font(.caption)
            Text("動画全体").font(.subheadline)
            Text(summary.overview).textSelection(.enabled)
            Text(
                summary.clockDomain == .editedOutput
                    ? "時刻は編集後の発話区間です。" : "時刻は原動画の発話区間です。編集後の時刻には変換していません。"
            )
            .font(.caption).foregroundStyle(.secondary)
            ForEach(summary.sections) { section in
                Divider()
                Text(
                    PlaybackClock.range(section.startSeconds, section.endSeconds)
                )
                .font(.headline.monospacedDigit())
                Text(section.text).textSelection(.enabled)
                ForEach(section.evidence.indices, id: \.self) { index in
                    let clock = section.evidence[index]
                    VStack(alignment: .leading, spacing: 12) {
                        if canSeek {
                            Button("\(PlaybackClock.label(clock.startSeconds))から") {
                                seek(clock.startSeconds)
                            }
                        } else {
                            Text("原動画 \(PlaybackClock.label(clock.startSeconds))から")
                        }
                        Text("根拠の発話：「\(clock.span.text)」").font(.caption).textSelection(.enabled)
                    }
                    .id(SummaryEvidenceTarget(summary: summary, clock: clock))
                }
            }
            Divider()
        }
    }
}
