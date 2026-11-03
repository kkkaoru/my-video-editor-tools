// Runs with bun (vitest). Tests the viewer page's resume-position module (public/resume.js).
import { afterEach, expect, it, vi } from "vitest";
import {
  readPosition,
  restorePosition,
  resumeKey,
  resumeStart,
  trackPositions,
  writeDue,
  writePosition,
} from "../public/resume.js";

class FakeVideo extends EventTarget {
  currentTime = 0;
  duration = 600;
  readyState = 1;
  seeking = false;
  dataset: Record<string, string> = {};
}

class FakeDocument extends EventTarget {
  visibilityState = "visible";
}

class FakeStorage {
  items = new Map<string, string>();
  getItem(key: string): string | null {
    return this.items.get(key) ?? null;
  }
  setItem(key: string, value: string): void {
    this.items.set(key, value);
  }
}

const blocked = (): FakeStorage => {
  throw new Error("SecurityError: storage is blocked");
};

afterEach(() => {
  vi.restoreAllMocks();
});

it("keys a position by catalog video and source", () => {
  expect(resumeKey("2026-10-01_replay-1", "edited")).toBe("resume:2026-10-01_replay-1:edited");
  expect(resumeKey("2026-10-01_replay-1", "original")).toBe("resume:2026-10-01_replay-1:original");
});

it("resumes a position past the start and short of the end", () => {
  expect(resumeStart(120.5, 600)).toBe(120.5);
  expect(resumeStart(5, 600)).toBe(5);
  expect(resumeStart(590, 600)).toBe(590);
});

it("starts over near the start, near the end, past the end and without a position", () => {
  expect(resumeStart(4.9, 600)).toBe(0);
  expect(resumeStart(590.1, 600)).toBe(0);
  expect(resumeStart(900, 600)).toBe(0);
  expect(resumeStart(Number.NaN, 600)).toBe(0);
});

it("trusts the saved position while the duration is unknown", () => {
  expect(resumeStart(120, Number.NaN)).toBe(120);
  expect(resumeStart(120, Number.POSITIVE_INFINITY)).toBe(120);
  expect(resumeStart(120, 0)).toBe(120);
});

it("writes at most once per interval", () => {
  expect(writeDue(10_000, Number.NEGATIVE_INFINITY)).toBe(true);
  expect(writeDue(14_999, 10_000)).toBe(false);
  expect(writeDue(15_000, 10_000)).toBe(true);
});

it("reads and writes positions in seconds with one decimal", () => {
  const storage = new FakeStorage();
  expect(writePosition(() => storage, "resume:a:edited", 12.345)).toBe(true);
  expect(storage.items.get("resume:a:edited")).toBe("12.3");
  expect(readPosition(() => storage, "resume:a:edited")).toBe(12.3);
  expect(readPosition(() => storage, "resume:b:edited")).toBe(Number.NaN);
});

it("plays on when storage is blocked, warning in English", () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  expect(readPosition(blocked, "resume:a:edited")).toBe(Number.NaN);
  expect(writePosition(blocked, "resume:a:edited", 12)).toBe(false);
  expect(warn.mock.calls).toStrictEqual([
    ["resume: reading resume:a:edited failed: Error: SecurityError: storage is blocked"],
    ["resume: writing resume:a:edited failed: Error: SecurityError: storage is blocked"],
  ]);
});

it("restores the saved position of a freshly loaded movie", () => {
  const storage = new FakeStorage();
  storage.setItem("resume:a:edited", "120.0");
  const video = new FakeVideo();
  video.dataset.resumeKey = "resume:a:edited";
  restorePosition(video, () => storage);
  expect(video.currentTime).toBe(120);
});

it("keeps a time somebody already chose, and a finished movie at the start", () => {
  const storage = new FakeStorage();
  storage.setItem("resume:a:edited", "120.0");
  storage.setItem("resume:b:edited", "599.0");
  const chosen = new FakeVideo();
  chosen.dataset.resumeKey = "resume:a:edited";
  chosen.currentTime = 42;
  restorePosition(chosen, () => storage);
  const seeking = new FakeVideo();
  seeking.dataset.resumeKey = "resume:a:edited";
  seeking.seeking = true;
  restorePosition(seeking, () => storage);
  const finished = new FakeVideo();
  finished.dataset.resumeKey = "resume:b:edited";
  restorePosition(finished, () => storage);
  const unkeyed = new FakeVideo();
  restorePosition(unkeyed, () => storage);
  expect([
    chosen.currentTime,
    seeking.currentTime,
    finished.currentTime,
    unkeyed.currentTime,
  ]).toStrictEqual([42, 0, 0, 0]);
});

it("throttles saves while playing and saves at once on pause and end", () => {
  const storage = new FakeStorage();
  const video = new FakeVideo();
  const clock = { now: 0 };
  trackPositions({
    video,
    page: new EventTarget(),
    document: new FakeDocument(),
    storage: () => storage,
    now: () => clock.now,
  });
  video.dataset.resumeKey = "resume:a:edited";
  video.currentTime = 10;
  video.dispatchEvent(new Event("timeupdate"));
  clock.now = 4000;
  video.currentTime = 14;
  video.dispatchEvent(new Event("timeupdate"));
  video.dispatchEvent(new Event("seeked"));
  expect(storage.items.get("resume:a:edited")).toBe("10.0");
  clock.now = 5000;
  video.currentTime = 15;
  video.dispatchEvent(new Event("seeked"));
  expect(storage.items.get("resume:a:edited")).toBe("15.0");
  clock.now = 5100;
  video.currentTime = 16;
  video.dispatchEvent(new Event("pause"));
  expect(storage.items.get("resume:a:edited")).toBe("16.0");
  video.currentTime = 600;
  video.dispatchEvent(new Event("ended"));
  expect(storage.items.get("resume:a:edited")).toBe("600.0");
});

it("saves when the page is hidden or left, not when it becomes visible", () => {
  const storage = new FakeStorage();
  const video = new FakeVideo();
  const page = new EventTarget();
  const document = new FakeDocument();
  trackPositions({ video, page, document, storage: () => storage, now: () => 0 });
  video.dataset.resumeKey = "resume:a:original";
  video.currentTime = 30;
  document.dispatchEvent(new Event("visibilitychange"));
  expect(storage.items.size).toBe(0);
  document.visibilityState = "hidden";
  document.dispatchEvent(new Event("visibilitychange"));
  expect(storage.items.get("resume:a:original")).toBe("30.0");
  video.currentTime = 31;
  page.dispatchEvent(new Event("pagehide"));
  expect(storage.items.get("resume:a:original")).toBe("31.0");
});

it("writes nothing before a movie is loaded or keyed, and returns a flush for source switches", () => {
  const storage = new FakeStorage();
  const video = new FakeVideo();
  const save = trackPositions({
    video,
    page: new EventTarget(),
    document: new FakeDocument(),
    storage: () => storage,
    now: () => 0,
  });
  video.currentTime = 30;
  save();
  video.dataset.resumeKey = "resume:a:edited";
  video.readyState = 0;
  video.dispatchEvent(new Event("pause"));
  expect(storage.items.size).toBe(0);
  video.readyState = 4;
  save();
  expect(storage.items.get("resume:a:edited")).toBe("30.0");
});

it("keeps tracking after a blocked write, throttling the retries", () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  const storage = new FakeStorage();
  const access = { blocked: true };
  const video = new FakeVideo();
  trackPositions({
    video,
    page: new EventTarget(),
    document: new FakeDocument(),
    storage: () => (access.blocked ? blocked() : storage),
    now: () => 0,
  });
  video.dataset.resumeKey = "resume:a:edited";
  video.currentTime = 20;
  video.dispatchEvent(new Event("timeupdate"));
  access.blocked = false;
  video.dispatchEvent(new Event("timeupdate"));
  expect(storage.items.size).toBe(0);
  video.dispatchEvent(new Event("pause"));
  expect(storage.items.get("resume:a:edited")).toBe("20.0");
  expect(warn.mock.calls.length).toBe(1);
});
