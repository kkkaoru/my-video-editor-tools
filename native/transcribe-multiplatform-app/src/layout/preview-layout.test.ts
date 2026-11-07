// Runs with bun: the preview geometry matches what the caption files will contain.
import { expect, it } from "vitest";
import type { Anchor, Settings } from "../backend/types.ts";
import {
  metricsFor,
  type PreviewLayout,
  type PreviewRow,
  previewLayout,
  previewRows,
  wrapText,
} from "./preview-layout.ts";

const settingsWith = (anchor: Anchor, offsetX: number, offsetY: number): Settings => ({
  palette: [
    {
      fill: "#FFFFFF",
      fill_width: 0,
      outline: "#000000",
      outline_width: 0.1,
      rim: "#000000",
      rim_width: 0.034,
      shadow: null,
      shadow_x: 0.03,
      shadow_y: -0.03,
      shadow_blur: 5,
    },
  ],
  font_family: "Sans",
  font_ratio: 0.05,
  width_ratio: 0.8,
  safe_left_percent: 0,
  safe_right_percent: 0,
  placement: {
    anchor,
    offset_x_percent: offsetX,
    offset_y_percent: offsetY,
  },
  language: "auto",
  face_model: "c",
  model_dirs: [],
  active_profile: "",
  line_height: 1.448,
  row_spacing: 1.45,
  rows_share_left: true,
  separation_backend: "onnx",
  speaker_count: 0,
  subtitle_format: "fcpxml",
  ui_language: "",
  ui_theme: "system",
});

it("derives the metrics from the short edge and the width", () => {
  const metrics: ReturnType<typeof metricsFor> = metricsFor(
    { width: 1920, height: 1080 },
    settingsWith("video_top", 0, 0),
  );
  expect(metrics.fontPx).toBe(54);
  expect(metrics.maxLinePx).toBe(1509);
});

it("keeps the metrics at their minimums on a tiny frame", () => {
  const metrics: ReturnType<typeof metricsFor> = metricsFor(
    { width: 4, height: 4 },
    settingsWith("video_top", 0, 0),
  );
  expect(metrics.fontPx).toBe(1);
  expect(metrics.maxLinePx).toBe(1);
});

it("never lets a line be shorter than one character", () => {
  expect(
    metricsFor({ width: 100, height: 100 }, { ...settingsWith("video_top", 0, 0), width_ratio: 0.01 })
      .maxLinePx,
  ).toBe(1);
});

it("shares a Japanese line evenly between two lines", () => {
  expect(
    wrapText("話者ごとに色が変わる袋文字の字幕です", {
      fontPx: 1,
      scale: 1,
      maxLinePx: 17,
    }),
  ).toStrictEqual(["話者ごとに色が変わ", "る袋文字の字幕です"]);
});

it("keeps the Japanese sample in two even lines on a vertical frame", () => {
  expect(
    previewLayout(
      "話者ごとに色が変わる袋文字の字幕です",
      { width: 1080, height: 1920 },
      { ...settingsWith("video_top", 0, 0), font_ratio: 0.03 },
    ).lines,
  ).toStrictEqual(["話者ごとに色が変わ", "る袋文字の字幕です"]);
});

it("runs on to more lines when two cannot hold it", () => {
  expect(
    wrapText("話者ごとに色が変わる袋文字の字幕です", {
      fontPx: 1,
      scale: 1,
      maxLinePx: 7,
    }),
  ).toStrictEqual(["話者ごとに色が", "変わる袋文", "字の字幕です"]);
});

it("keeps a line that fits whole", () => {
  expect(wrapText("abc", { fontPx: 1, scale: 1, maxLinePx: 10 })).toStrictEqual(["abc"]);
});

it("never cuts a Latin word, and drops the space at the break", () => {
  expect(
    wrapText("Untertitel mit Kontur wechseln die Farbe je Sprecher", {
      fontPx: 10,
      scale: 1,
      maxLinePx: 200,
    }),
  ).toStrictEqual(["Untertitel mit Kontur", "wechseln die Farbe je Sprecher"]);
});

it("splits Latin text at the evenest space", () => {
  expect(wrapText("one two three four five", { fontPx: 10, scale: 1, maxLinePx: 90 })).toStrictEqual([
    "one two three",
    "four five",
  ]);
});

it("measures Latin glyphs as narrower than Japanese ones", () => {
  expect(wrapText("abcdefghij", { fontPx: 10, scale: 1, maxLinePx: 60 })).toStrictEqual(["abcdefghij"]);
  expect(wrapText("かきくけこさしすせそ", { fontPx: 10, scale: 1, maxLinePx: 60 })).toStrictEqual([
    "かきくけこ",
    "さしすせそ",
  ]);
});

it("measures half-width katakana at half an em", () => {
  expect(wrapText("ｱｲｳｴｵｶｷｸ", { fontPx: 10, scale: 1, maxLinePx: 40 })).toStrictEqual(["ｱｲｳｴｵｶｷｸ"]);
});

it("cuts a word only when it is too long for a line by itself", () => {
  expect(wrapText("abcdefghijkl", { fontPx: 10, scale: 1, maxLinePx: 36 })).toStrictEqual([
    "abcdef",
    "ghijkl",
  ]);
});

it("keeps a katakana word whole", () => {
  expect(wrapText("これはプレビューです", { fontPx: 10, scale: 1, maxLinePx: 70 })).toStrictEqual([
    "これは",
    "プレビューです",
  ]);
});

it("does not start a line with closing punctuation or a small kana", () => {
  expect(wrapText("ああああ、いいいい", { fontPx: 10, scale: 1, maxLinePx: 60 })).toStrictEqual([
    "ああああ、",
    "いいいい",
  ]);
  expect(wrapText("かかかっきき", { fontPx: 10, scale: 1, maxLinePx: 40 })).toStrictEqual([
    "かか",
    "かっきき",
  ]);
});

it("does not end a line with an opening bracket", () => {
  expect(wrapText("ああああ「いいいい", { fontPx: 10, scale: 1, maxLinePx: 60 })).toStrictEqual([
    "ああああ",
    "「いいいい",
  ]);
});

it("leaves text with no legal break on one line", () => {
  expect(wrapText("、、、、、、", { fontPx: 10, scale: 1, maxLinePx: 30 })).toStrictEqual(["、、、、、、"]);
});

it("gives nothing for only spaces", () => {
  expect(wrapText("   ", { fontPx: 10, scale: 1, maxLinePx: 30 })).toStrictEqual([]);
});

it("wraps empty text to no lines", () => {
  expect(wrapText("", { fontPx: 10, scale: 1, maxLinePx: 30 })).toStrictEqual([]);
});

it("starts every row at one left edge and spaces them by the setting", () => {
  const frame = { width: 1920, height: 1080 };
  const settings: Settings = settingsWith("video_bottom", 0, 0);
  const layout: PreviewLayout = previewLayout("abcdefghij", frame, settings);
  const rows: PreviewRow[] = previewRows(layout, frame, settings);
  expect(rows.map(({ line }: PreviewRow): string => line)).toStrictEqual(["abcdefghij", "abcdefg", "abcde"]);
  expect(rows[0]?.x).toBe(960);
  expect(rows[1]?.x).toBeLessThan(960);
  expect(rows[2]?.x).toBeLessThan(rows[1]?.x ?? 0);
  expect((rows[0]?.y ?? 0) - (rows[1]?.y ?? 0)).toBeCloseTo(1.45 * layout.renderedFontPx, 6);
});

it("centres the rows when they need not share an edge", () => {
  const frame = { width: 1920, height: 1080 };
  const settings: Settings = { ...settingsWith("video_bottom", 0, 0), rows_share_left: false };
  const layout: PreviewLayout = previewLayout("abcdefghij", frame, settings);
  expect(previewRows(layout, frame, settings).map(({ x }: PreviewRow): number => x)).toStrictEqual([
    960, 960, 960,
  ]);
});

it("grows the rows downwards from the top of the video", () => {
  const frame = { width: 1920, height: 1080 };
  const settings: Settings = settingsWith("video_top", 0, 0);
  const layout: PreviewLayout = previewLayout("abcdefghij", frame, settings);
  const rows: PreviewRow[] = previewRows(layout, frame, settings);
  expect(rows[1]?.y).toBeGreaterThan(rows[0]?.y ?? 0);
});

it("places the block at the bottom of the video", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("video_bottom", 0, 0),
  );
  expect(layout.lines).toStrictEqual(["abcd"]);
  expect(layout.x).toBe(960);
  expect(layout.y).toBeCloseTo(912.45, 1);
  expect(layout.face).toBe(null);
});

it("places the block at the top of the video", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("video_top", 0, 0),
  );
  expect(layout.y).toBeCloseTo(167.55, 1);
  expect(layout.face).toBe(null);
});

it("places the block above the face", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("face_above", 0, 0),
  );
  expect(layout.y).toBeCloseTo(162.45, 1);
  expect(layout.face?.centerX).toBe(960);
});

it("places the block below the face", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1920, height: 1080 },
    settingsWith("face_below", 0, 0),
  );
  expect(layout.y).toBeCloseTo(863.55, 1);
  expect(layout.face?.centerX).toBe(960);
});

it("applies the percentage offsets", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1000, height: 1000 },
    settingsWith("video_top", 10, 20),
  );
  expect(layout.x).toBe(600);
  expect(layout.y).toBeCloseTo(351.88, 1);
});

it("keeps the block inside the frame when the offset pushes it out", () => {
  const layout: PreviewLayout = previewLayout(
    "abcd",
    { width: 1000, height: 1000 },
    settingsWith("video_bottom", -50, 50),
  );
  expect(layout.x).toBeCloseTo(67.51, 1);
  expect(layout.y).toBeCloseTo(959.23, 1);
});

it("centres a block wider than the frame", () => {
  const layout: PreviewLayout = previewLayout(
    "かきくけこさしす",
    { width: 100, height: 1080 },
    { ...settingsWith("video_top", 40, 0), font_ratio: 1, width_ratio: 0.1 },
  );
  expect(layout.lines).toStrictEqual(["か", "き", "く", "け", "こ", "さ", "し", "す"]);
  expect(layout.x).toBe(50);
  expect(layout.y).toBe(540);
});

const safeSettings = (left: number, right: number): Settings => ({
  ...settingsWith("video_bottom", 0, 0),
  safe_left_percent: left,
  safe_right_percent: right,
});

it("gives the width the safe zones leave when that is less than the width setting", () => {
  expect(metricsFor({ width: 1000, height: 1000 }, safeSettings(30, 20)).maxLinePx).toBe(473);
});

it("keeps the width setting when the safe zones leave more than that", () => {
  expect(metricsFor({ width: 1000, height: 1000 }, safeSettings(5, 5)).maxLinePx).toBe(773);
});

it("wraps sooner between wide safe zones", () => {
  expect(
    previewLayout("あいうえおかきくけこ", { width: 1000, height: 1000 }, safeSettings(30, 30)).lines,
  ).toStrictEqual(["あいうえお", "かきくけこ"]);
});

it("reports the safe zones in pixels", () => {
  const layout: PreviewLayout = previewLayout("あ", { width: 1000, height: 1000 }, safeSettings(30, 10));
  expect([layout.safeLeftPx, layout.safeRightPx]).toStrictEqual([300, 100]);
});

it("moves a centred block out of a wide left safe zone", () => {
  const layout: PreviewLayout = previewLayout("あ", { width: 1000, height: 1000 }, safeSettings(60, 0));
  expect(layout.x).toBeCloseTo(635.95, 1);
});

it("stops an offset at the right safe zone", () => {
  const settings: Settings = safeSettings(0, 20);
  const layout: PreviewLayout = previewLayout(
    "あ",
    { width: 1000, height: 1000 },
    { ...settings, placement: { ...settings.placement, offset_x_percent: 50 } },
  );
  expect(layout.x).toBeCloseTo(764.05, 1);
});

it("centres a block wider than the space between the safe zones in that space", () => {
  const layout: PreviewLayout = previewLayout("あ", { width: 100, height: 1000 }, safeSettings(40, 40));
  expect(layout.x).toBeCloseTo(50, 5);
});

it("draws glyphs by the frame's height, as the editor does", () => {
  const layout: PreviewLayout = previewLayout(
    "あ",
    { width: 1080, height: 1920 },
    settingsWith("video_top", 0, 0),
  );
  expect(layout.fontPx).toBe(54);
  expect(layout.renderedFontPx).toBeCloseTo(93.504, 3);
  expect(layout.lineHeightPx).toBeCloseTo(135.39, 2);
});

it("scales a landscape frame by its height too", () => {
  const layout: PreviewLayout = previewLayout(
    "あ",
    { width: 1920, height: 1080 },
    settingsWith("video_top", 0, 0),
  );
  expect(layout.renderedFontPx).toBeCloseTo(52.596, 3);
});

it("draws the strokes in frame pixels, whatever the frame's size", () => {
  expect(
    previewLayout("あ", { width: 1280, height: 720 }, settingsWith("video_top", 0, 0)).strokeUnitPx,
  ).toBe(100);
  expect(
    previewLayout("あ", { width: 3840, height: 2160 }, settingsWith("video_top", 0, 0)).strokeUnitPx,
  ).toBe(100);
});

it("takes every drawn layer of the first speaker off the line width", () => {
  const settings: Settings = {
    ...settingsWith("video_top", 0, 0),
    palette: [
      {
        fill: "#FFFFFF",
        fill_width: 0.01,
        outline: "#000000",
        outline_width: 0.1,
        rim: "#000000",
        rim_width: 0.02,
        shadow: null,
        shadow_x: 0.03,
        shadow_y: -0.03,
        shadow_blur: 5,
      },
    ],
  };
  expect(metricsFor({ width: 1000, height: 1000 }, settings).maxLinePx).toBe(774);
});

it("does not count a transparent outer edge", () => {
  const settings: Settings = {
    ...settingsWith("video_top", 0, 0),
    palette: [
      {
        fill: "#FFFFFF",
        fill_width: 0,
        outline: "#000000",
        outline_width: 0.1,
        rim: null,
        rim_width: 0.2,
        shadow: null,
        shadow_x: 0.03,
        shadow_y: -0.03,
        shadow_blur: 5,
      },
    ],
  };
  expect(metricsFor({ width: 1000, height: 1000 }, settings).maxLinePx).toBe(780);
});

it("takes the widest edge of all speakers off the line width", () => {
  const speaker: Settings["palette"][number] = {
    fill: "#FFFFFF",
    fill_width: 0,
    outline: "#000000",
    outline_width: 0.25,
    rim: null,
    rim_width: 0,
    shadow: null,
    shadow_x: 0.03,
    shadow_y: -0.03,
    shadow_blur: 5,
  };
  const base: Settings = settingsWith("video_top", 0, 0);
  expect(
    metricsFor({ width: 1000, height: 1000 }, { ...base, palette: [...base.palette, speaker] }).maxLinePx,
  ).toBe(750);
});

it("keeps at least one pixel of edge on a tiny text", () => {
  const base: Settings = settingsWith("video_top", 0, 0);
  expect(
    metricsFor({ width: 1000, height: 1000 }, { ...base, font_ratio: 0.002, palette: [] }).maxLinePx,
  ).toBe(700);
});

it("keeps a sokuon with the kana after it", () => {
  expect(wrapText("あっかかか", { fontPx: 10, scale: 1, maxLinePx: 30 })).toStrictEqual(["あっか", "かか"]);
});
