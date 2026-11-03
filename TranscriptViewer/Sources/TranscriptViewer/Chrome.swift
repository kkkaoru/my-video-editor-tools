import CoreGraphics

enum Chrome {
    static let hit: CGFloat = 44
    static let playerHeight: CGFloat = 320
    /// The user drags the player between these heights; a stored height outside is clamped.
    static let playerHeightRange: ClosedRange<Double> = 180...900
    static let playerHeightStep = 40.0
    static let utteranceBlockHeight: CGFloat = 96
    static let nearbyStripHeight: CGFloat = 148
    /// With the clips tab open the timeline keeps its current line only and the list gets the rest,
    /// at least this much.
    static let clipsTimelineHeight: CGFloat = 150
    /// The timeline's heading and current line: the clips list never squeezes them away.
    static let clipsTimelineMinHeight: CGFloat = 110
    static let clipsMinHeight: CGFloat = 240

    static func clampedPlayerHeight(_ height: Double) -> Double {
        height.isFinite
            ? min(max(height, playerHeightRange.lowerBound), playerHeightRange.upperBound)
            : Double(playerHeight)
    }

    /// The timeline's height bounds: fixed between these with the clips tab open, free otherwise.
    static func timelineHeights(clipsOpen: Bool) -> (min: CGFloat?, max: CGFloat?) {
        clipsOpen ? (clipsTimelineMinHeight, clipsTimelineHeight) : (nil, nil)
    }
}
