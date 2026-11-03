import Foundation

/// The one way playback times are shown: `H:MM:SS.mmm`, or `MM:SS.mmm` under an hour.
public enum PlaybackClock {
    public static func label(_ seconds: Double) -> String {
        let milliseconds = seconds.isFinite ? max(0, Int((seconds * 1_000).rounded())) : 0
        let hours = milliseconds / 3_600_000
        let minutes = milliseconds / 60_000 % 60
        let rest = String(format: "%02d.%03d", milliseconds / 1_000 % 60, milliseconds % 1_000)
        return hours > 0
            ? String(format: "%d:%02d:", hours, minutes) + rest
            : String(format: "%02d:", minutes) + rest
    }

    /// `start – end` of a range.
    public static func range(_ start: Double, _ end: Double) -> String {
        "\(label(start)) – \(label(end))"
    }
}
