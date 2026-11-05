// Runs with bun (vitest). Tests the viewer page's live caption module (public/captions.js) on the
// shared synthetic plan (tests/fixtures/caption-plan-v1.json) and tiny synthetic bitmaps.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { expect, it, vi } from "vitest";
import {
  anchor,
  applyFill,
  compose,
  estimatedBlockWidth,
  fitRect,
  fontOf,
  frameAt,
  gradientAt,
  graphemes,
  grow,
  insideSafeZone,
  layoutTitle,
  linePrefix,
  lineSpans,
  parsePlan,
  readSettings,
  resolveTitles,
  runningMax,
  SETTING_KEYS,
  shadowed,
  speakerIcons,
  titlesAt,
  UNASSIGNED,
  upcoming,
  visibleAt,
  writeSetting,
} from "../public/captions.js";

const fixture = () =>
  JSON.parse(
    readFileSync(resolve(import.meta.dirname, "../../tests/fixtures/caption-plan-v1.json"), "utf8"),
  );

class MemoryStorage {
  values = new Map<string, string>();
  getItem(key: string) {
    return this.values.get(key) ?? null;
  }
  setItem(key: string, value: string) {
    this.values.set(key, value);
  }
  removeItem(key: string) {
    this.values.delete(key);
  }
}

it("parses the shared fixture plan", () => {
  const plan = parsePlan(fixture());
  expect(plan.titles.map((title: { text: string }) => title.text)).toStrictEqual([
    "ふぅ",
    "なんかミスっ\nてないかなぁ",
    "来てくれてありがとう\nございます",
  ]);
  expect(plan.nameplate.name).toBe("テスト");
});

it("refuses plans that break the contract", () => {
  const broken = (change: (plan: ReturnType<typeof fixture>) => void) => {
    const plan = fixture();
    change(plan);
    return () => parsePlan(plan);
  };
  expect(broken((p) => (p.version = 3))).toThrow("unsupported version");
  expect(broken((p) => (p.burned = "yes"))).toThrow("burned");
  expect(broken((p) => (p.rate = 0))).toThrow("rate");
  expect(broken((p) => (p.style.extra = 1))).toThrow("14 fields");
  expect(broken((p) => (p.style.keyline = 99))).toThrow("invalid style");
  expect(broken((p) => (p.titles = {}))).toThrow("titles must be a list");
  expect(broken((p) => p.titles[1].graphemeFrames.pop())).toThrow("invalid title");
  expect(broken((p) => (p.titles[1].graphemeFrames[0] = 99))).toThrow("invalid title");
  expect(broken((p) => (p.titles[1].alignment = "right"))).toThrow("invalid title");
  expect(broken((p) => (p.titles[1].style = { fontName: "x" }))).toThrow("14 fields");
  expect(broken((p) => (p.titles[1].startFrame = 30))).toThrow("overlap");
  expect(() => parsePlan({ ...fixture(), style: null })).toThrow("style is missing");
});

it("counts graphemes as Swift does, line breaks included", () => {
  expect(graphemes("ｶﾞ👍🏽\n é")).toStrictEqual(["ｶﾞ", "👍🏽", "\n", " ", "é"]);
  const spans = lineSpans("なんかミスっ\nてないかなぁ");
  expect(spans.map((span: { first: number }) => span.first)).toStrictEqual([0, 7]);
  expect(linePrefix(spans[0], 3)).toBe("なんか");
  expect(linePrefix(spans[1], 6)).toBe("");
  expect(linePrefix(spans[1], 9)).toBe("てな");
});

it("reveals graphemes from their onset frames over the title's span", () => {
  const plan = parsePlan(fixture());
  const title = plan.titles[1];
  expect([39, 40, 49, 50, 59, 85, 119, 120].map((f) => visibleAt(title, f))).toStrictEqual([
    0, 0, 0, 3, 4, 13, 13, 0,
  ]);
  expect(
    [0, 14, 33, 34, 40, 119, 120, 156, 157].map((f) => titlesAt(plan.titles, f)),
  ).toStrictEqual([[], [0], [0], [], [1], [1], [2], [2], []]);
  // 14/30 s × 30 is 13.999… in floating point: still frame 14.
  expect(frameAt(14 / 30, 30)).toBe(14);
  expect(frameAt(14.99 / 30, 30)).toBe(14);
});

it("prepares the next caption state ahead", () => {
  const { titles } = parsePlan(fixture());
  expect(upcoming(titles, 14)).toStrictEqual([
    { index: 0, visible: 2 },
    { index: 1, visible: 3 },
  ]);
  expect(upcoming(titles, 20)).toStrictEqual([{ index: 1, visible: 3 }]);
  expect(upcoming(titles, 85)).toStrictEqual([{ index: 2, visible: 5 }]);
  expect(upcoming(titles, 140)).toStrictEqual([]);
  expect(upcoming(titles, 200)).toStrictEqual([]);
});

it("maps Motion positions and letterboxes the canvas", () => {
  expect(anchor({ x: -460, y: -404 }, 1920, 1080)).toStrictEqual({ x: 500, baseline: 944 });
  expect(fitRect(400, 400, 1080, 1920)).toStrictEqual({ x: 87.5, y: 0, scale: 400 / 1920 });
  expect(fitRect(1920, 400, 1920, 1080)).toStrictEqual({
    x: (1920 - 1920 * (400 / 1080)) / 2,
    y: 0,
    scale: 400 / 1080,
  });
});

it("lays a title out in a fixed box sized for its outline and shadow", () => {
  const plan = parsePlan(fixture());
  const style = plan.style;
  const font = fontOf(style);
  expect(font.css).toBe(`700 ${style.fontSize}px CaptionNotoSansJP`);
  expect(font.ascent + font.descent).toBeCloseTo(120.12, 2);
  const measure = (text: string) => ({
    width: graphemes(text).length * 80,
    actualBoundingBoxLeft: 0,
    actualBoundingBoxRight: graphemes(text).length * 80,
    actualBoundingBoxAscent: 70,
    actualBoundingBoxDescent: 10,
  });
  const args = { style, canvasWidth: 1920, canvasHeight: 1080, measure, ...font };
  const centred = layoutTitle({ ...args, title: { ...plan.titles[1], alignment: undefined } });
  expect(centred.lines.map((line: { left: number }) => line.left)).toStrictEqual([260, 260]);
  const left = layoutTitle({ ...args, title: plan.titles[1] });
  expect(
    left.lines.map((line: { left: number; baseline: number }) => [line.left, line.baseline]),
  ).toStrictEqual([
    [500, 862],
    [500, 862 + font.ascent + font.descent],
  ]);
  expect(left.originX).toBe(Math.floor(500 - (3 * style.shadowSigma + 2)));
  expect(left.originY).toBe(Math.floor(862 - 70 - (style.rim + 2)));
  expect(left.boxTop).toBeCloseTo(862 - font.ascent);
  expect(() => layoutTitle({ ...args, title: { ...plan.titles[0], text: "\n" } })).toThrow(
    "no glyphs",
  );
  expect(() => fontOf({ ...style, fontName: "Other" })).toThrow("unsupported caption font");
});

it("dilates by an axis-aligned square, truncating at the edges", () => {
  const dot = new Float32Array(25);
  dot[12] = 1;
  const square = Array.from(grow(dot, 5, 5, 1));
  expect(square).toStrictEqual([
    0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0,
  ]);
  const corner = new Float32Array(25);
  corner[0] = 0.5;
  expect(Array.from(grow(corner, 5, 5, 1)).slice(0, 7)).toStrictEqual([
    0.5, 0.5, 0, 0, 0, 0.5, 0.5,
  ]);
  // A fractional radius blends the two nearest whole windows.
  const half = Array.from(grow(dot, 5, 5, 0.5));
  expect(half[12]).toBe(1);
  expect(half[11]).toBe(0.5);
  expect(half[6]).toBe(0.25);
  expect(grow(dot, 5, 5, 0)).toBe(dot);
  expect(
    Array.from(runningMax(Float32Array.from([0, 3, 0, 0, 2, 0, 0]), 7, 1, 3, true)),
  ).toStrictEqual([3, 3, 3, 2, 2, 2, 0]);
});

it("blurs and moves the shadow down, scaled by its opacity", () => {
  const style = { shadowSigma: 0.6, shadowOffsetY: 1.5, shadowOpacity: 0.5 };
  const dot = new Float32Array(49);
  dot[3 * 7 + 3] = 1;
  const shadow = shadowed(dot, 7, 7, style);
  const column = [0, 1, 2, 3, 4, 5, 6].map((row) => shadow[row * 7 + 3] ?? 0);
  // Mass sums to the opacity, centred 1.5 rows below the dot.
  const total = Array.from(shadow).reduce((sum, value) => sum + value, 0);
  expect(total).toBeCloseTo(0.5, 2);
  const centre =
    column.reduce((sum, value, row) => sum + value * row, 0) / column.reduce((a, b) => a + b, 0);
  expect(centre).toBeCloseTo(4.5, 1);
});

it("blends the face gradient between sRGB stops and the gradient-off fill uses the top colour", () => {
  const style = parsePlan(fixture()).style;
  const top = gradientAt(style, 0.5, 0, 100);
  const bottom = gradientAt(style, 99.5, 0, 100);
  // #97D8F7 and #2FB0EE in linear light.
  expect(top[0]).toBeCloseTo(((0x97 / 255 + 0.055) / 1.055) ** 2.4, 6);
  expect(bottom[2]).toBeCloseTo(((0xee / 255 + 0.055) / 1.055) ** 2.4, 6);
  // Each line's box repeats the gradient.
  expect(gradientAt(style, 199.5, 0, 100)).toStrictEqual(bottom);
  const single = applyFill(style, { top: "#FF0000", bottom: "#0000FF", gradient: false });
  expect([single.gradientTop, single.gradientBottom]).toStrictEqual(["#FF0000", "#FF0000"]);
  expect(gradientAt(single, 99.5, 0, 100)).toStrictEqual([1, 0, 0]);
  const both = applyFill(style, { top: "#FF0000", bottom: "#0000FF", gradient: true });
  expect(both.gradientBottom).toBe("#0000FF");
  expect(applyFill(style, null)).toBe(style);
  expect(() => applyFill(style, { top: "red", bottom: "#0000FF", gradient: true })).toThrow();
});

it("composites the layers back to front into the burned movie's code values", () => {
  const style = { ...parsePlan(fixture()).style, shadowOpacity: 0 };
  // A 4×4 opaque glyph at (18…21, 18…21).
  const coverage = Float32Array.from({ length: 1600 }, (_, i) =>
    Math.abs((i % 40) - 19.5) < 2 && Math.abs(Math.floor(i / 40) - 19.5) < 2 ? 1 : 0,
  );
  const rgba = compose({
    coverage,
    width: 40,
    height: 40,
    originY: 0,
    style,
    boxTop: 0,
    boxHeight: 40,
  });
  const pixel = (x: number, y: number) =>
    Array.from(rgba.slice((y * 40 + x) * 4, (y * 40 + x) * 4 + 4));
  expect(pixel(0, 0)).toStrictEqual([0, 0, 0, 0]);
  // Outside the white band only the black rim remains; inside it, white; at the face, the fill.
  expect(pixel(18 - 11, 20)).toStrictEqual([0, 0, 0, 255]);
  expect(pixel(18 - 5, 20)).toStrictEqual([255, 255, 255, 255]);
  const face = pixel(20, 20);
  expect(face[3]).toBe(255);
  const linearTop = gradientAt(style, 20.5, 0, 40);
  expect(face.slice(0, 3)).toStrictEqual(
    linearTop.map((value: number) => Math.floor(value ** (1 / style.outputGamma) * 255 + 0.5)),
  );
});

it("defaults captions and the icon to off when burned, and keeps viewer overrides", () => {
  const storage = new MemoryStorage();
  const plan = parsePlan(fixture());
  const id = "v";
  expect(readSettings(() => storage, id, plan)).toStrictEqual({
    captions: true,
    icon: true,
    gradient: true,
    top: "#97D8F7",
    bottom: "#2FB0EE",
    fill: null,
    layout: { version: 1, mode: "plan" },
    hidden: new Set(),
  });
  const burned = { ...plan, burned: true, nameplate: { ...plan.nameplate, burned: true } };
  const off = readSettings(() => storage, id, burned);
  expect([off.captions, off.icon]).toStrictEqual([false, false]);
  expect(readSettings(() => storage, id, { ...plan, nameplate: null }).icon).toBe(false);
  writeSetting(() => storage, SETTING_KEYS.captions(id), "1");
  writeSetting(() => storage, SETTING_KEYS.gradient, "0");
  writeSetting(() => storage, SETTING_KEYS.top, "#ff0000");
  const custom = readSettings(() => storage, id, burned);
  expect(custom.captions).toBe(true);
  expect(readSettings(() => storage, "other", burned).captions).toBe(false);
  expect(custom.fill).toStrictEqual({ top: "#FF0000", bottom: "#2FB0EE", gradient: false });
  writeSetting(() => storage, SETTING_KEYS.top, null);
  expect(storage.values.has(SETTING_KEYS.top)).toBe(false);
});

it("works without storage", () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const blocked = () => {
    throw new Error("blocked");
  };
  const plan = parsePlan(fixture());
  expect(readSettings(blocked, "v", plan).captions).toBe(true);
  expect(writeSetting(blocked, "k", "1")).toBe(false);
  expect(warn).toHaveBeenCalled();
  warn.mockRestore();
});

const fixtureJson = (name: string) =>
  JSON.parse(readFileSync(resolve(import.meta.dirname, `../../tests/fixtures/${name}`), "utf8"));

// The per-title face tops build_catalog publishes (caption_face_tops), derived here from the
// shared face track with the same rule, so the resolver's half is checked against the fixture.
interface FaceSample {
  frame: number;
  faceCount: number;
  box: { y: number; score: number } | null;
}
const faceTopsFor = (layout: { mode: string; minScore?: number; maxDrift?: number }) => {
  const plan = parsePlan(fixture());
  const trusted = fixtureJson("caption-faces-v1.json")
    .samples.filter(
      (s: FaceSample) => s.faceCount === 1 && s.box && s.box.score >= (layout.minScore ?? 0.5),
    )
    .map((s: FaceSample) => [s.frame, s.box?.y ?? 0]);
  return plan.titles.map((title: { startFrame: number; endFrame: number }) => {
    const inside = trusted.filter(
      (f: number[]) => (f[0] ?? 0) >= title.startFrame && (f[0] ?? 0) <= title.endFrame,
    );
    if (inside.length > 0) return [Math.min(...inside.map((f: number[]) => f[1] ?? 0)), "face"];
    const before = trusted.filter((f: number[]) => (f[0] ?? 0) < title.startFrame).at(-1);
    const after = trusted.find((f: number[]) => (f[0] ?? 0) > title.endFrame);
    if (!before || !after || Math.abs(before[1] - after[1]) > (layout.maxDrift ?? 150)) return null;
    return [Math.min(before[1], after[1]), "held"];
  });
};

it("resolves caption positions as the shared expectations do", () => {
  const expected = fixtureJson("caption-layout-expected-v1.json");
  const plan = parsePlan(fixture());
  const results = expected.cases.map((item: { name: string; layout: { mode: string } }) => [
    item.name,
    resolveTitles(plan, item.layout, faceTopsFor(item.layout)),
  ]);
  const misses = results.flatMap(
    ([name, titles]: [string, { x: number; y: number; basis: string }[]]) =>
      titles.flatMap((title, index) => {
        const want = expected.cases.find((item: { name: string }) => item.name === name).titles[
          index
        ];
        const close = Math.abs(title.x - want.x) <= 1e-6 && Math.abs(title.y - want.y) <= 1e-6;
        return close && title.basis === want.basis ? [] : [`${name}#${index}`];
      }),
  );
  expect(results.length).toBe(10);
  expect(misses).toStrictEqual([]);
});

it("lets a viewer override the delivery's caption position", () => {
  const storage = new MemoryStorage();
  const plan = { ...parsePlan(fixture()), layout: { version: 1, mode: "bottom", margin: 160 } };
  expect(readSettings(() => storage, "v", plan).layout).toStrictEqual({
    version: 1,
    mode: "bottom",
    margin: 160,
  });
  writeSetting(
    () => storage,
    SETTING_KEYS.layout("v"),
    '{"mode":"aboveFace","offsetY":-30,"margin":1,"offsetX":"x"}',
  );
  expect(readSettings(() => storage, "v", plan).layout).toStrictEqual({
    version: 1,
    mode: "aboveFace",
    margin: 160,
    offsetY: -30,
  });
  writeSetting(() => storage, SETTING_KEYS.layout("v"), "{broken");
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  expect(readSettings(() => storage, "v", parsePlan(fixture())).layout).toStrictEqual({
    version: 1,
    mode: "plan",
  });
  warn.mockRestore();
});

const planV2 = () => fixtureJson("caption-plan-v2.json");

// build_catalog's caption_face_tops for each version 2 case (tests/test_web_viewer_build_catalog.py
// asserts it computes exactly these), so this checks its tops through the resolver to the fixture.
const BUILT_FACE_TOPS_V2: Record<string, ([number, string] | null)[]> = {
  absent: [
    [480.0, "face"],
    [400.0, "held"],
    [300.0, "face"],
    [300.0, "face"],
    [300.0, "face"],
    null,
    [600.0, "face"],
  ],
  "fixture-layout": [
    [480.0, "face"],
    [400.0, "held"],
    [300.0, "face"],
    [300.0, "face"],
    [300.0, "face"],
    null,
    [600.0, "face"],
  ],
  "bottom-all": [
    [480.0, "face"],
    [400.0, "held"],
    [300.0, "face"],
    [300.0, "face"],
    [300.0, "face"],
    null,
    [600.0, "face"],
  ],
  "above-face-all": [
    [480.0, "face"],
    [400.0, "held"],
    [300.0, "face"],
    [300.0, "face"],
    [300.0, "face"],
    null,
    [600.0, "face"],
  ],
  "above-face-v1-faces": [
    [700.0, "face"],
    [700.0, "held"],
    [700.0, "held"],
    [760.0, "face"],
    [760.0, "face"],
    null,
    null,
  ],
  "per-speaker-offsets": [
    [480.0, "face"],
    [400.0, "held"],
    [300.0, "face"],
    [300.0, "face"],
    [300.0, "face"],
    null,
    [600.0, "face"],
  ],
};
interface LayoutCase {
  mode: string;
}

it("resolves multi-speaker positions as the shared version 2 expectations do", () => {
  const expected = fixtureJson("caption-layout-expected-v2.json");
  const plan = parsePlan(planV2());
  const misses = expected.cases.flatMap(
    (item: {
      name: string;
      layout: LayoutCase;
      faces: string;
      titles: { x: number; y: number; basis: string }[];
    }) => {
      const got = resolveTitles(plan, item.layout, BUILT_FACE_TOPS_V2[item.name] ?? null);
      return item.titles.flatMap((want, index) => {
        const title = got[index];
        const close = Math.abs(title.x - want.x) <= 1e-6 && Math.abs(title.y - want.y) <= 1e-6;
        return close && title.basis === want.basis ? [] : [`${item.name}#${index}`];
      });
    },
  );
  expect(expected.cases.length).toBe(6);
  expect(misses).toStrictEqual([]);
});

it("shows overlapping speakers together and checks the version 2 rules", () => {
  const plan = parsePlan(planV2());
  expect(titlesAt(plan.titles, 55)).toStrictEqual([0, 1, 2]);
  expect(titlesAt(plan.titles, 100)).toStrictEqual([2, 3, 4]);
  expect(UNASSIGNED).toBe("unassigned");
  const broken = (change: (plan: ReturnType<typeof planV2>) => void) => {
    const value = planV2();
    change(value);
    return () => parsePlan(value);
  };
  expect(broken((p) => p.speakers.pop() && p.speakers.pop())).toThrow("at least two speakers");
  expect(broken((p) => (p.speakers[1].id = "left"))).toThrow("invalid speaker");
  expect(broken((p) => (p.speakers[0].id = "unassigned"))).toThrow("invalid speaker");
  expect(broken((p) => (p.speakers[0].extra = 1))).toThrow("invalid speaker");
  expect(broken((p) => (p.speakers[1].icon = "a.png"))).toThrow("invalid speaker");
  expect(broken((p) => (p.titles[0].speaker = "nobody"))).toThrow("unknown speaker");
  expect(broken((p) => (p.titles[6].startFrame = 40))).toThrow("titles go back");
  expect(
    broken(
      (p) => (p.titles[6] = { ...p.titles[6], startFrame: 59, graphemeFrames: [59, 163, 166] }),
    ),
  ).toThrow("titles go back");
  expect(
    broken((p) => {
      p.titles[1].speaker = "left";
    }),
  ).toThrow("left's titles overlap");
});

it("keeps the viewer's position over each speaker's and remembers hidden speakers", () => {
  const storage = new MemoryStorage();
  const plan = { ...parsePlan(planV2()), layout: fixtureJson("caption-layout-v2.json") };
  expect(readSettings(() => storage, "v", plan).layout.speakers.right).toStrictEqual({
    mode: "aboveFace",
    minScore: 0.3,
    offsetY: -30,
  });
  writeSetting(() => storage, SETTING_KEYS.layout("v"), '{"mode":"top","offsetY":5}');
  writeSetting(() => storage, SETTING_KEYS.hidden("v"), '["left","unassigned",3]');
  const settings = readSettings(() => storage, "v", plan);
  expect([
    settings.layout.mode,
    settings.layout.speakers.right,
    [...settings.hidden],
  ]).toStrictEqual(["top", { minScore: 0.3 }, ["left", "unassigned"]]);
  writeSetting(() => storage, SETTING_KEYS.hidden("v"), "{oops");
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  expect([...readSettings(() => storage, "v", plan).hidden]).toStrictEqual([]);
  warn.mockRestore();
});

it("places speaker icons at the bottom edge of their lanes, as the shared expectation does", () => {
  const plan = parsePlan(planV2());
  expect(speakerIcons(plan)).toStrictEqual(fixtureJson("caption-layout-expected-v2.json").icons);
  // Clamped inside the canvas, and none for a plan without speakers.
  const far = {
    ...plan,
    speakers: [
      { ...plan.speakers[0], laneX: -2000 },
      { ...plan.speakers[2], laneX: 2000 },
    ],
  };
  expect(speakerIcons(far).map((icon: { x: number }) => icon.x)).toStrictEqual([0, 1834]);
  expect(speakerIcons(parsePlan(fixture()))).toStrictEqual([]);
  // Never burned: shown by default even on a burned plan; the viewer's switch still wins.
  const storage = new MemoryStorage();
  expect(readSettings(() => storage, "v", { ...plan, burned: true }).icon).toBe(true);
  writeSetting(() => storage, SETTING_KEYS.icon("v"), "0");
  expect(readSettings(() => storage, "v", plan).icon).toBe(false);
});

it("keeps the caption canvas transparent and hideable over the movie", () => {
  // The page's generic canvas rule (timeline bars) paints a panel background and display:block,
  // which covered the playing movie (sound with no picture) when the caption canvas inherited it.
  const page = readFileSync(resolve(import.meta.dirname, "../public/index.html"), "utf8");
  expect(page).toMatch(/#captionCanvas \{[^}]*background:none;/);
  expect(page).toContain("#captionCanvas[hidden] { display:none; }");
});

it("makes only the captions unselectable, not the movie player", () => {
  const page = readFileSync(resolve(import.meta.dirname, "../public/index.html"), "utf8");
  expect(page).toMatch(/#captionCanvas \{[^}]*-webkit-user-select:none; user-select:none;/);
  expect(page).not.toMatch(/#videoSlot \{[^}]*user-select/);
});

const SAFE_STYLE = { fontSize: 100, rim: 8 };
const SAFE_PLAN = { width: 1000, style: SAFE_STYLE, safeLeft: 50, safeRight: 150 };

it("estimates a block from its widest line and the rim on both sides", () => {
  expect(estimatedBlockWidth({ text: "あい\nab" }, SAFE_STYLE)).toBe(220);
  expect(estimatedBlockWidth({ text: "ｱa" }, SAFE_STYLE)).toBe(130);
});

it("keeps a block between unequal margins", () => {
  expect(insideSafeZone(100, { text: "あ".repeat(6) }, SAFE_PLAN)).toBe(40);
  expect(insideSafeZone(-300, { text: "あ".repeat(2) }, SAFE_PLAN)).toBe(-300);
  expect(insideSafeZone(-500, { text: "あ".repeat(2) }, SAFE_PLAN)).toBe(-340);
  expect(insideSafeZone(500, { text: "あ".repeat(2) }, SAFE_PLAN)).toBe(240);
});

it("centres a block wider than the margins leave between them", () => {
  expect(insideSafeZone(100, { text: "あ".repeat(20) }, SAFE_PLAN)).toBe(-50);
});

it("leaves a plan without margins and a left-aligned title alone", () => {
  expect(insideSafeZone(490, { text: "あ" }, { width: 1000, style: SAFE_STYLE })).toBe(490);
  expect(insideSafeZone(490, { text: "あ", alignment: "left" }, SAFE_PLAN)).toBe(490);
});

it("refuses margins that come alone or leave no width", () => {
  const plan = JSON.parse(
    readFileSync(resolve(import.meta.dirname, "../../tests/fixtures/caption-plan-v1.json"), "utf8"),
  );
  expect(() => parsePlan({ ...plan, safeLeft: 5 })).toThrow();
  expect(() => parsePlan({ ...plan, safeLeft: plan.width, safeRight: 0 })).toThrow();
  expect(() => parsePlan({ ...plan, safeLeft: -1, safeRight: 2 })).toThrow();
  expect(parsePlan({ ...plan, safeLeft: 54, safeRight: 54 }).safeLeft).toBe(54);
});
