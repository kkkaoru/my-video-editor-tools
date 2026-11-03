import SwiftUI
import Testing
import ViewInspector

@testable import TranscriptCore
@testable import TranscriptViewer

struct PlaybackClockTests {
    @Test(arguments: [
        (0.0, "00:00.000"), (1.25, "00:01.250"), (62.3, "01:02.300"), (599.9994, "09:59.999"),
        (599.9996, "10:00.000"), (3599.999, "59:59.999"), (3600, "1:00:00.000"),
        (3723.456, "1:02:03.456"), (36_000.5, "10:00:00.500"), (-3, "00:00.000"),
        (.nan, "00:00.000"), (.infinity, "00:00.000"),
    ])
    func labelsShowMilliseconds(seconds: Double, expected: String) {
        #expect(PlaybackClock.label(seconds) == expected)
    }

    @Test func rangesJoinBothEnds() {
        #expect(PlaybackClock.range(1, 2.5) == "00:01.000 – 00:02.500")
    }
}

@MainActor
struct PlaybackTimeReadoutTests {
    @Test func readoutShowsThePlayheadAndLengthToTheMillisecond() throws {
        let fixture = try SummaryFixture()
        defer { #expect(throws: Never.self) { try fixture.remove() } }
        let session = PlaybackSession()
        defer {
            session.player.pause()
            session.stopObserving()
        }
        session.videos = [fixture.video]
        session.selectedVideoID = fixture.video.id
        session.openSelection()
        session.updateCue(at: 0.25)
        #expect(session.displaySeconds == 0.25)
        let readout = try PlaybackTimeReadout(session: session).inspect().text()
        #expect(try readout.string() == "00:00.250 / 00:01.000")
        #expect(try readout.accessibilityLabel().string() == "再生位置 00:00.250 / 00:01.000")
        let root = try TranscriptRootView(session: session).inspect()
        #expect(throws: Never.self) {
            try root.find(viewWithAccessibilityIdentifier: "playback-time")
        }
        #expect(throws: Never.self) { try root.find(text: "00:00.000 – 00:01.000") }
    }
}

@MainActor
struct ClipTimeLabelTests {
    @Test func clipRowsShowMillisecondRangesAndKeepTheirDefaultActionsInert() {
        let panel = ClipsPanel(model: ClipsModel(api: nil)) { _, _, _ in }
        panel.seek(1)
        let row = ClipRow(clip: twoRangeClip, cursor: nil) { _, _, _ in }
        row.select()
        #expect(row.rangeLabel(1, twoRangeClip.ranges[1]) == "2. 05:00.000〜（20.0秒）")
    }
}
