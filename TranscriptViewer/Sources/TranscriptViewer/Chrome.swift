import CoreGraphics

enum Chrome {
    static let hit: CGFloat = 44
    static let playerHeight: CGFloat = 320
    static let utteranceBlockHeight: CGFloat = 96
    static let nearbyStripHeight: CGFloat = 148
    /// With the clips tab open the timeline keeps its current line only and the list gets the rest,
    /// at least this much.
    static let clipsTimelineHeight: CGFloat = 150
    /// The timeline's heading and current line: the clips list never squeezes them away.
    static let clipsTimelineMinHeight: CGFloat = 110
    static let clipsMinHeight: CGFloat = 240

    /// The timeline's height bounds: fixed between these with the clips tab open, free otherwise.
    static func timelineHeights(clipsOpen: Bool) -> (min: CGFloat?, max: CGFloat?) {
        clipsOpen ? (clipsTimelineMinHeight, clipsTimelineHeight) : (nil, nil)
    }
}
