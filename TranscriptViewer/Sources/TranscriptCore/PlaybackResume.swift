import Foundation

/// Where a video opens again: its last recorded position, unless that is near either end.
public enum PlaybackResume {
    /// A position this close to the start or the end opens the video from the start.
    public static let edgeSeconds = 5.0

    /// One position per movie file, so an edited delivery and its original stay apart.
    public static func key(for movie: URL) -> String { movie.standardizedFileURL.path }

    /// The start for `saved` in a movie of `duration` seconds; 0 when finished or barely begun.
    public static func start(saved: Double?, duration: Double) -> Double {
        guard let saved, saved.isFinite, duration.isFinite, saved > edgeSeconds,
            saved < duration - edgeSeconds
        else { return 0 }
        return saved
    }
}
