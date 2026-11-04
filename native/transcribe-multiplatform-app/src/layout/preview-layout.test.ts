// Runs with bun: the preview geometry matches what the caption files will contain.
import { expect, it } from "vitest";
import type { Anchor, Settings } from "../backend/types.ts";
import { metricsFor, type PreviewLayout, previewLayout, wrapText } from "./preview-layout.ts";

const settingsWith = (anchor: Anchor, offsetX: number, offsetY: number): Settings => ({
  palette: [],
  unassigned: { fill: "#AAAAAA", outline: "#000000" },
  font_family: "Sans",
  font_ratio: 0.05,
  outline_ratio: 0.1,
  shadow: { enabled: false, strength: 0.5 },
  width_ratio: 0.8,
  placement: { anchor, offset_x_percent: offsetX, offset_y_percent: offsetY },
  language: "auto",
  diarize_model: "b",
  face_model: "c",
  model_dirs: [],
  rim_ratio: 0.034,
  line_height: 1.448,
  speaker_count: 0,
});

it("derives the metrics from the short edge and the width", () => {
  const metrics: ReturnType<typeof metricsFor> = metricsFor(
    { width: 1920, height: 1080 },
    settingsWith("video_top", 0, 0),
  );
  expect(metrics.fontPx).toBe(54);
  expect(metrics.outlinePx).toBeCloseTo(5.4);
  expect(metrics.maxLinePx).toBeCloseTo(1525.2);
});

it("keeps the metrics at their minimums on a tiny frame", () => {
  expect(metricsFor({ width: 4, height: 4 }, settingsWith("video_top", 0, 0))).toStrictEqual({
    fontPx: 1,
    outlinePx: 1,
    maxLinePx: 1.2000000000000002,
  });
});

it("never lets a line be shorter than one character", () => {
  expect(
    metricsFor({ width: 100, height: 100 }, { ...settingsWith("video_top", 0, 0), width_ratio: 0.01 }),
  ).toStrictEqual({
    fontPx: 5,
    outlinePx: 1,
    maxLinePx: 5,
  });
});

it("wraps text at the character count", () => {
  expect(wrapText("abcde", 2)).toStrictEqual(["ab", "cd", "e"]);
});

it("shares an orphaned last line evenly between the last two lines", () => {
  expect(wrapText("話者ごとに色が変わる袋文字の字幕です", 17)).toStrictEqual([
    "話者ごとに色が変わ",
    "る袋文字の字幕です",
  ]);
});

it("keeps the Japanese sample in two even lines on a vertical frame", () => {
  expect(
    previewLayout(
      "話者ごとに色が変わる袋文字の字幕です",
      { width: 1080, height: 1920 },
      settingsWith("video_top", 0, 0),
    ).lines,
  ).toStrictEqual(["話者ごとに色が変わ", "る袋文字の字幕です"]);
});

it("leaves earlier lines alone when rebalancing, like captions.rs", () => {
  expect(wrapText("話者ごとに色が変わる袋文字の字幕です", 7)).toStrictEqual([
    "話者ごとに色が",
    "変わる袋文字",
    "の字幕です",
  ]);
});

it("rebalances a last line of exactly four characters", () => {
  expect(wrapText("abcdefghijklmn", 10)).toStrictEqual(["abcdefg", "hijklmn"]);
});

it("keeps a last line of five characters", () => {
  expect(wrapText("abcdefghijklmno", 10)).toStrictEqual(["abcdefghij", "klmno"]);
});

it("never rebalances a single short line", () => {
  expect(wrapText("abc", 10)).toStrictEqual(["abc"]);
});

it("wraps empty text to no lines", () => {
  expect(wrapText("", 2)).toStrictEqual([]);
});

it("places the block at the bottom of the video", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("video_bottom", 0, 0),
  );
  expect(layout.lines).toStrictEqual(["abcd"]);
  expect(layout.x).toBe(960);
  expect(layout.y).toBeCloseTo(979.668);
  expect(layout.face).toBe(null);
});

it("places the block at the top of the video", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("video_top", 0, 0),
  );
  expect(layout.y).toBeCloseTo(100.332);
  expect(layout.face).toBe(null);
});

it("places the block above the face", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("face_above", 0, 0),
  );
  expect(layout.y).toBeCloseTo(198.828);
  expect(layout.face?.centerX).toBe(960);
});

it("places the block below the face", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("face_below", 0, 0),
  );
  expect(layout.y).toBeCloseTo(827.172);
  expect(layout.face?.centerX).toBe(960);
});

it("applies the percentage offsets", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1000, height: 1000 },
    settingsWith("video_top", 10, 20),
  );
  expect(layout.x).toBe(600);
  expect(layout.y).toBeCloseTo(292.9);
});

it("keeps the block inside the frame when the offset pushes it out", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1000, height: 1000 },
    settingsWith("video_bottom", -50, 50),
  );
  expect(layout.x).toBe(105);
  expect(layout.y).toBeCloseTo(957.1);
});

it("centres a block wider than the frame", () => {
  const layout: PreviewLayout = previewLayout(
    "abcdefgh",
    { width: 100, height: 100 },
    { ...settingsWith("video_top", 40, 0), font_ratio: 1, width_ratio: 0.1 },
  );
  expect(layout.lines).toStrictEqual(["a", "b", "c", "d", "e", "f", "g", "h"]);
  expect(layout.x).toBe(50);
  expect(layout.y).toBe(50);
});
