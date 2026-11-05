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
  active_profile: "",
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
  expect(metrics.maxLinePx).toBeCloseTo(1521.53, 1);
});

it("keeps the metrics at their minimums on a tiny frame", () => {
  const metrics: ReturnType<typeof metricsFor> = metricsFor(
    { width: 4, height: 4 },
    settingsWith("video_top", 0, 0),
  );
  expect(metrics.fontPx).toBe(1);
  expect(metrics.outlinePx).toBe(1);
  expect(metrics.maxLinePx).toBeCloseTo(1.132);
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

it("shares a Japanese line evenly between two lines", () => {
  expect(wrapText("話者ごとに色が変わる袋文字の字幕です", { fontPx: 1, maxLinePx: 17 })).toStrictEqual([
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

it("runs on to more lines when two cannot hold it", () => {
  expect(wrapText("話者ごとに色が変わる袋文字の字幕です", { fontPx: 1, maxLinePx: 7 })).toStrictEqual([
    "話者ごとに色が",
    "変わる袋文",
    "字の字幕です",
  ]);
});

it("keeps a line that fits whole", () => {
  expect(wrapText("abc", { fontPx: 1, maxLinePx: 10 })).toStrictEqual(["abc"]);
});

it("never cuts a Latin word, and drops the space at the break", () => {
  expect(
    wrapText("Untertitel mit Kontur wechseln die Farbe je Sprecher", { fontPx: 10, maxLinePx: 200 }),
  ).toStrictEqual(["Untertitel mit Kontur", "wechseln die Farbe je Sprecher"]);
});

it("splits Latin text at the evenest space", () => {
  expect(wrapText("one two three four five", { fontPx: 10, maxLinePx: 90 })).toStrictEqual([
    "one two three",
    "four five",
  ]);
});

it("measures Latin glyphs as narrower than Japanese ones", () => {
  expect(wrapText("abcdefghij", { fontPx: 10, maxLinePx: 60 })).toStrictEqual(["abcdefghij"]);
  expect(wrapText("かきくけこさしすせそ", { fontPx: 10, maxLinePx: 60 })).toStrictEqual([
    "かきくけこ",
    "さしすせそ",
  ]);
});

it("measures half-width katakana at half an em", () => {
  expect(wrapText("ｱｲｳｴｵｶｷｸ", { fontPx: 10, maxLinePx: 40 })).toStrictEqual(["ｱｲｳｴｵｶｷｸ"]);
});

it("cuts a word only when it is too long for a line by itself", () => {
  expect(wrapText("abcdefghijkl", { fontPx: 10, maxLinePx: 36 })).toStrictEqual(["abcdef", "ghijkl"]);
});

it("keeps a katakana word whole", () => {
  expect(wrapText("これはプレビューです", { fontPx: 10, maxLinePx: 70 })).toStrictEqual([
    "これは",
    "プレビューです",
  ]);
});

it("does not start a line with closing punctuation or a small kana", () => {
  expect(wrapText("ああああ、いいいい", { fontPx: 10, maxLinePx: 60 })).toStrictEqual([
    "ああああ、",
    "いいいい",
  ]);
  expect(wrapText("かかかっきき", { fontPx: 10, maxLinePx: 40 })).toStrictEqual(["かか", "かっきき"]);
});

it("does not end a line with an opening bracket", () => {
  expect(wrapText("ああああ「いいいい", { fontPx: 10, maxLinePx: 60 })).toStrictEqual([
    "ああああ",
    "「いいいい",
  ]);
});

it("leaves text with no legal break on one line", () => {
  expect(wrapText("、、、、、、", { fontPx: 10, maxLinePx: 30 })).toStrictEqual(["、、、、、、"]);
});

it("gives nothing for only spaces", () => {
  expect(wrapText("   ", { fontPx: 10, maxLinePx: 30 })).toStrictEqual([]);
});

it("wraps empty text to no lines", () => {
  expect(wrapText("", { fontPx: 10, maxLinePx: 30 })).toStrictEqual([]);
});

it("places the block at the bottom of the video", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("video_bottom", 0, 0),
  );
  expect(layout.lines).toStrictEqual(["abcd"]);
  expect(layout.x).toBe(960);
  expect(layout.y).toBeCloseTo(911.18, 1);
  expect(layout.face).toBe(null);
});

it("places the block at the top of the video", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("video_top", 0, 0),
  );
  expect(layout.y).toBeCloseTo(168.82, 1);
  expect(layout.face).toBe(null);
});

it("places the block above the face", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("face_above", 0, 0),
  );
  expect(layout.y).toBeCloseTo(161.18, 1);
  expect(layout.face?.centerX).toBe(960);
});

it("places the block below the face", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("face_below", 0, 0),
  );
  expect(layout.y).toBeCloseTo(864.82, 1);
  expect(layout.face?.centerX).toBe(960);
});

it("applies the percentage offsets", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1000, height: 1000 },
    settingsWith("video_top", 10, 20),
  );
  expect(layout.x).toBe(600);
  expect(layout.y).toBeCloseTo(356.32, 1);
});

it("keeps the block inside the frame when the offset pushes it out", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1000, height: 1000 },
    settingsWith("video_bottom", -50, 50),
  );
  expect(layout.x).toBeCloseTo(66.7);
  expect(layout.y).toBeCloseTo(954.8, 1);
});

it("centres a block wider than the frame", () => {
  const layout: PreviewLayout = previewLayout(
    "かきくけこさしす",
    { width: 100, height: 100 },
    { ...settingsWith("video_top", 40, 0), font_ratio: 1, width_ratio: 0.1 },
  );
  expect(layout.lines).toStrictEqual(["か", "き", "く", "け", "こ", "さ", "し", "す"]);
  expect(layout.x).toBe(50);
  expect(layout.y).toBe(50);
});
