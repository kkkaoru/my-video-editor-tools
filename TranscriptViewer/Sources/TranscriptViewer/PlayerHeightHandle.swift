import SwiftUI

/// The divider under the player: dragging it (or VoiceOver's adjust) resizes the player. The
/// caller stores the height, so it survives relaunches.
struct PlayerHeightHandle: View {
    @Binding var height: Double
    @State private var dragStart: Double?

    var body: some View {
        Capsule()
            .fill(.secondary)
            .frame(width: 56, height: 5)
            .frame(maxWidth: .infinity, minHeight: 16)
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 1).onChanged(dragChanged).onEnded(dragEnded))
            .help("ドラッグで動画の高さを変更")
            .accessibilityLabel("動画の高さ")
            .accessibilityValue("\(Int(height))ポイント")
            .accessibilityAdjustableAction(adjust)
            .accessibilityIdentifier("player-height-handle")
            .accessibilityElement()
    }

    func dragChanged(_ value: DragGesture.Value) {
        let start = dragStart ?? height
        dragStart = start
        height = Chrome.clampedPlayerHeight(start + value.translation.height)
    }

    func dragEnded(_: DragGesture.Value) { dragStart = nil }

    func adjust(_ direction: AccessibilityAdjustmentDirection) {
        let step = direction == .increment ? Chrome.playerHeightStep : -Chrome.playerHeightStep
        height = Chrome.clampedPlayerHeight(height + step)
    }
}
