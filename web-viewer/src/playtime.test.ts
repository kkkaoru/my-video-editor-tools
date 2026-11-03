// Runs with bun (vitest). Tests the viewer page's time formatter and clock (public/playtime.js).
import { expect, it } from "vitest";
import { clockText, formatTime, trackClock } from "../public/playtime.js";

class FakeVideo extends EventTarget {
  currentTime = 0;
  duration = Number.NaN;
  paused = true;
}

class FakeDocument extends EventTarget {
  visibilityState = "visible";
}

interface FakeFrames {
  pending: Map<number, () => void>;
  cancelled: number[];
  frame: (callback: () => void) => number;
  cancel: (id: number) => void;
  runAll: () => void;
}

function fakeFrames(): FakeFrames {
  const pending = new Map<number, () => void>();
  const cancelled: number[] = [];
  const counter = { next: 1 };
  return {
    pending,
    cancelled,
    frame: (callback) => {
      const id = counter.next++;
      pending.set(id, callback);
      return id;
    },
    cancel: (id) => {
      cancelled.push(id);
      pending.delete(id);
    },
    runAll: () => {
      const callbacks = [...pending.values()];
      pending.clear();
      callbacks.forEach((callback) => {
        callback();
      });
    },
  };
}

it("formats hours, minutes, seconds and milliseconds", () => {
  expect(formatTime(3723.456)).toBe("1:02:03.456");
  expect(formatTime(0)).toBe("0:00:00.000");
  expect(formatTime(59.9994)).toBe("0:00:59.999");
  expect(formatTime(59.9996)).toBe("0:01:00.000");
  expect(formatTime(1.001)).toBe("0:00:01.001");
  expect(formatTime(36_000)).toBe("10:00:00.000");
});

it("reads negative and unknown times as zero", () => {
  expect(formatTime(-3)).toBe("0:00:00.000");
  expect(formatTime(Number.NaN)).toBe("0:00:00.000");
  expect(formatTime(Number.POSITIVE_INFINITY)).toBe("0:00:00.000");
});

it("shows the position over the duration, or a placeholder while it is unknown", () => {
  expect(clockText(62.5, 3600)).toBe("0:01:02.500 / 1:00:00.000");
  expect(clockText(1, Number.NaN)).toBe("0:00:01.000 / -:--:--.---");
  expect(clockText(1, 0)).toBe("0:00:01.000 / -:--:--.---");
});

it("redraws every frame while playing and stops on pause", () => {
  const video = new FakeVideo();
  const element = { textContent: "" };
  const frames = fakeFrames();
  trackClock({ video, element, document: new FakeDocument(), ...frames });
  expect(element.textContent).toBe("0:00:00.000 / -:--:--.---");
  video.duration = 120;
  video.dispatchEvent(new Event("loadedmetadata"));
  expect(element.textContent).toBe("0:00:00.000 / 0:02:00.000");
  video.paused = false;
  video.dispatchEvent(new Event("playing"));
  video.dispatchEvent(new Event("playing"));
  expect(frames.pending.size).toBe(1);
  video.currentTime = 0.016;
  frames.runAll();
  expect(element.textContent).toBe("0:00:00.016 / 0:02:00.000");
  video.currentTime = 0.033;
  frames.runAll();
  expect(element.textContent).toBe("0:00:00.033 / 0:02:00.000");
  expect(frames.pending.size).toBe(1);
  video.paused = true;
  video.currentTime = 0.04;
  video.dispatchEvent(new Event("pause"));
  expect(frames.pending.size).toBe(0);
  expect(frames.cancelled).toStrictEqual([3]);
  expect(element.textContent).toBe("0:00:00.040 / 0:02:00.000");
});

it("does not start for a paused movie and redraws on a seek", () => {
  const video = new FakeVideo();
  const element = { textContent: "" };
  const frames = fakeFrames();
  trackClock({ video, element, document: new FakeDocument(), ...frames });
  video.dispatchEvent(new Event("playing"));
  expect(frames.pending.size).toBe(0);
  video.currentTime = 30.25;
  video.dispatchEvent(new Event("seeked"));
  expect(element.textContent).toBe("0:00:30.250 / -:--:--.---");
  video.dispatchEvent(new Event("ended"));
  expect(frames.cancelled).toStrictEqual([]);
});

it("stops while the page is hidden and resumes when it is visible again", () => {
  const video = new FakeVideo();
  const element = { textContent: "" };
  const document = new FakeDocument();
  const frames = fakeFrames();
  trackClock({ video, element, document, ...frames });
  video.paused = false;
  video.dispatchEvent(new Event("playing"));
  document.visibilityState = "hidden";
  document.dispatchEvent(new Event("visibilitychange"));
  expect(frames.pending.size).toBe(0);
  video.dispatchEvent(new Event("playing"));
  expect(frames.pending.size).toBe(0);
  document.visibilityState = "visible";
  document.dispatchEvent(new Event("visibilitychange"));
  expect(frames.pending.size).toBe(1);
  video.dispatchEvent(new Event("emptied"));
  expect(frames.pending.size).toBe(0);
});
