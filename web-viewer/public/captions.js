// Live style-20 captions over the delivery movie, drawn from its caption plan (docs/caption-plan.md)
// with the algorithm of native/caption-render's MotionCaptionRenderer: the plan's lines laid out
// once, any grapheme prefix drawn in that fixed layout (Type On), the face grown and the keyline,
// white band and rim grown by axis-aligned squares, a Gaussian shadow of the glyphs moved down,
// all composited in linear light and written as the burned movie's code values. Loaded by
// index.html with import(); the pure rules are unit-tested in src/captions.test.ts.

export const FONT_FAMILY = "CaptionNotoSansJP";
// The same Noto Sans JP variable font the native renderer uses (lossless WOFF2; OFL.txt beside it).
export const FONT_PATH = "/fonts/NotoSansJP-VariableFont_wght.woff2";
// The plans' fontName is the variable font's Bold named instance. Ascent and descent per em are its
// hhea values (1160, -288 at 1000 upem), which CoreText reports: TextMetrics rounds them to pixels.
const FONTS = { "NotoSansJP-Thin_Bold": { weight: 700, ascent: 1.16, descent: 0.288 } };
const HEX = /^#[0-9A-Fa-f]{6}$/;
const STYLE_KEYS = [
  "fontName", "fontSize", "faceGrow", "keyline", "whiteBorder", "rim", "shadowOpacity",
  "shadowOffsetY", "shadowSigma", "gradientTop", "gradientBottom", "topLocation", "bottomLocation",
  "outputGamma",
];
// 14/30 s × 30 is 13.999…: a time on a frame boundary belongs to that frame.
const FRAME_EPSILON = 1e-4;

const segmenter = new Intl.Segmenter(undefined, { granularity: "grapheme" });
// Extended grapheme clusters, as Swift's String.count counts them ("\n" is one).
export const graphemes = (text) => Array.from(segmenter.segment(text), (part) => part.segment);

const finite = (value) => typeof value === "number" && Number.isFinite(value);
const fail = (message) => {
  throw new Error(`caption plan: ${message}`);
};

// MotionCaptionStyle.validate().
export function checkStyle(style) {
  if (!style || typeof style !== "object") fail("style is missing");
  const keys = Object.keys(style).sort();
  if (keys.join() !== [...STYLE_KEYS].sort().join()) fail("style must have exactly its 14 fields");
  const lengths = ["fontSize", "faceGrow", "keyline", "whiteBorder", "rim", "shadowOffsetY", "shadowSigma", "outputGamma"];
  const ok =
    typeof style.fontName === "string" &&
    lengths.every((key) => finite(style[key]) && style[key] >= 0) &&
    style.fontSize > 0 && style.outputGamma > 0 &&
    style.faceGrow <= style.keyline && style.keyline <= style.whiteBorder && style.whiteBorder <= style.rim &&
    finite(style.shadowOpacity) && style.shadowOpacity >= 0 && style.shadowOpacity <= 1 &&
    finite(style.topLocation) && style.topLocation >= 0 && style.topLocation <= 1 &&
    finite(style.bottomLocation) && style.bottomLocation >= style.topLocation && style.bottomLocation <= 1 &&
    HEX.test(style.gradientTop) && HEX.test(style.gradientBottom);
  if (!ok) fail("invalid style");
  return style;
}

// MotionCaptionTitle.validate().
export function checkTitle(title) {
  const frames = title?.graphemeFrames;
  const ok =
    typeof title?.text === "string" && title.text.length > 0 &&
    Number.isInteger(title.startFrame) && Number.isInteger(title.endFrame) &&
    title.startFrame >= 0 && title.startFrame < title.endFrame &&
    Array.isArray(frames) && frames.length === graphemes(title.text).length &&
    frames.every((f, i) => Number.isInteger(f) && f >= title.startFrame && f < title.endFrame && (i === 0 || frames[i - 1] <= f)) &&
    finite(title.x) && finite(title.y) &&
    (title.alignment === undefined || title.alignment === null || title.alignment === "left");
  if (!ok) fail(`invalid title at frame ${title?.startFrame}`);
  if (title.style !== undefined && title.style !== null) checkStyle(title.style);
  return title;
}

// Version 2's speaker table (docs/caption-plan.md "Multiple speakers"), in lane order.
// iconData is the checked icon the catalog build inlines for the browser.
const SPEAKER_KEYS = new Set(["id", "name", "colorTop", "colorBottom", "laneX", "icon", "iconSha256", "iconData"]);
export const UNASSIGNED = "unassigned";
function checkSpeakers(speakers) {
  if (!Array.isArray(speakers) || speakers.length < 2) fail("version 2 needs at least two speakers");
  const ids = new Set();
  for (const speaker of speakers) {
    const ok =
      Object.keys(speaker ?? {}).every((key) => SPEAKER_KEYS.has(key)) &&
      typeof speaker.id === "string" && speaker.id.length > 0 && speaker.id !== UNASSIGNED && !ids.has(speaker.id) &&
      typeof speaker.name === "string" && speaker.name.length > 0 &&
      HEX.test(speaker.colorTop) && HEX.test(speaker.colorBottom) && finite(speaker.laneX) &&
      (speaker.icon === undefined) === (speaker.iconSha256 === undefined);
    if (!ok) fail(`invalid speaker ${speaker?.id}`);
    ids.add(speaker.id);
  }
  return ids;
}

// Titles of version 1 never overlap; in version 2 only one speaker's titles never overlap (the
// unassigned ones may, and are stacked), and starts never decrease.
function checkOrder(plan, ids) {
  const ends = new Map();
  plan.titles.forEach((title, i) => {
    const previous = plan.titles[i - 1];
    if (plan.version === 1 && previous && previous.endFrame > title.startFrame) fail(`titles overlap at frame ${title.startFrame}`);
    if (plan.version === 1) return;
    const speaker = title.speaker ?? null;
    if (speaker !== null && !ids.has(speaker)) fail(`unknown speaker ${speaker}`);
    if (previous && previous.startFrame > title.startFrame) fail(`titles go back at frame ${title.startFrame}`);
    if (speaker !== null && (ends.get(speaker) ?? 0) > title.startFrame) fail(`${speaker}'s titles overlap at frame ${title.startFrame}`);
    if (speaker !== null) ends.set(speaker, title.endFrame);
  });
}

// A version 1 or 2 delivery-clock plan, validated as the native CaptionPlan decodes it.
export function parsePlan(plan) {
  if (![1, 2].includes(plan?.version) || plan.clock !== "delivery") fail("unsupported version or clock");
  if (typeof plan.burned !== "boolean") fail("burned must be a boolean");
  for (const key of ["width", "height", "rate", "frames"]) {
    if (!Number.isInteger(plan[key]) || plan[key] <= 0) fail(`${key} must be a positive integer`);
  }
  const { safeLeft, safeRight } = plan;
  if ((safeLeft === undefined) !== (safeRight === undefined)) fail("safeLeft and safeRight come together");
  if (safeLeft !== undefined && !(Number.isInteger(safeLeft) && Number.isInteger(safeRight) && safeLeft >= 0 && safeRight >= 0 && safeLeft + safeRight < plan.width)) {
    fail("safeLeft and safeRight must leave part of the width");
  }
  checkStyle(plan.style);
  if (!Array.isArray(plan.titles)) fail("titles must be a list");
  plan.titles.forEach(checkTitle);
  checkOrder(plan, plan.version === 2 ? checkSpeakers(plan.speakers) : new Set());
  return plan;
}

export const frameAt = (seconds, rate) => Math.floor(seconds * rate + FRAME_EPSILON);

// Graphemes shown on `frame`: Type On End shows each from its onset onward.
export function visibleAt(title, frame) {
  if (frame < title.startFrame || frame >= title.endFrame) return 0;
  return title.graphemeFrames.filter((onset) => onset <= frame).length;
}

// Count of titles starting on or before `frame` (starts never decrease).
function startedBy(titles, frame) {
  let low = 0;
  let high = titles.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (titles[middle].startFrame <= frame) low = middle + 1;
    else high = middle;
  }
  return low;
}

const longestOf = new WeakMap();
// Indices of the titles showing on `frame`, in plan order (later ones are drawn on top).
export function titlesAt(titles, frame) {
  if (!longestOf.has(titles)) longestOf.set(titles, Math.max(0, ...titles.map((t) => t.endFrame - t.startFrame)));
  const longest = longestOf.get(titles);
  const shown = [];
  for (let index = startedBy(titles, frame) - 1; index >= 0 && titles[index].startFrame > frame - longest; index--) {
    if (frame < titles[index].endFrame) shown.unshift(index);
  }
  return shown;
}

// Lines of a title with the title-wide index of their first grapheme ("\n" counts as one).
export function lineSpans(text) {
  let first = 0;
  return text.split("\n").map((line) => {
    const parts = graphemes(line);
    const span = { parts, first };
    first += parts.length + 1;
    return span;
  });
}

// The visible prefix of a line, drawn at the full line's position.
export const linePrefix = (span, visible) => span.parts.slice(0, Math.max(0, visible - span.first)).join("");

// Motion's position (first line's baseline centre, or start when left-aligned; origin at the canvas
// centre, y up) in canvas pixels from the top left.
export const anchor = (title, width, height) => ({ x: width / 2 + title.x, baseline: height / 2 - title.y });

// Where a canvas of `width`×`height` shows inside a box, letterboxed as <video> shows its movie.
export function fitRect(boxWidth, boxHeight, width, height) {
  const scale = Math.min(boxWidth / width, boxHeight / height);
  return { x: (boxWidth - width * scale) / 2, y: (boxHeight - height * scale) / 2, scale };
}

export const parseHex = (hex) => [1, 3, 5].map((at) => Number.parseInt(hex.slice(at, at + 2), 16) / 255);
const linear = (value) => (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);

// MotionCaptionStyle.applying(CaptionFill): the viewer's face colours; with the gradient off the
// face is one fill in the top colour.
export function applyFill(style, fill) {
  if (!fill) return style;
  return checkStyle({ ...style, gradientTop: fill.top, gradientBottom: fill.gradient ? fill.bottom : fill.top });
}

// Linear fill colour of an output row: each line's text box maps the gradient, Motion easing with
// smootherstep between the sRGB-encoded stops.
export function gradientAt(style, y, boxTop, boxHeight) {
  const inBox = (y - boxTop) % boxHeight;
  const location = (inBox < 0 ? inBox + boxHeight : inBox) / boxHeight;
  const span = style.bottomLocation - style.topLocation;
  const t = span > 0 ? Math.min(Math.max((location - style.topLocation) / span, 0), 1) : 0;
  const mix = t * t * t * (t * (6 * t - 15) + 10);
  const top = parseHex(style.gradientTop);
  const bottom = parseHex(style.gradientBottom);
  return top.map((value, i) => linear(value + (bottom[i] - value) * mix));
}

// Running maximum over `size` (odd) samples along rows or columns, the window truncated at the
// edges (van Herk/Gil-Werman over a zero-padded line: coverage is never negative, so zero padding
// is truncation).
export function runningMax(values, width, height, size, horizontal) {
  const radius = (size - 1) / 2;
  const count = horizontal ? width : height;
  const lines = horizontal ? height : width;
  const step = horizontal ? 1 : width;
  const lineStep = horizontal ? width : 1;
  const padded = count + 2 * radius;
  const forward = new Float32Array(padded);
  const backward = new Float32Array(padded);
  const result = new Float32Array(values.length);
  for (let line = 0; line < lines; line++) {
    const base = line * lineStep;
    const at = (k) => (k < radius || k >= count + radius ? 0 : values[base + (k - radius) * step]);
    for (let k = 0; k < padded; k++) forward[k] = k % size === 0 ? at(k) : Math.max(forward[k - 1], at(k));
    for (let k = padded - 1; k >= 0; k--) {
      backward[k] = k % size === size - 1 || k === padded - 1 ? at(k) : Math.max(backward[k + 1], at(k));
    }
    for (let i = 0; i < count; i++) result[base + i * step] = Math.max(backward[i], forward[i + 2 * radius]);
  }
  return result;
}

// Coverage dilated by an axis-aligned square of half-side `radius` (Motion's outline growth), a
// fractional radius blending the two nearest whole windows.
export function grow(coverage, width, height, radius) {
  if (!(radius > 0)) return coverage;
  const whole = Math.floor(radius);
  const part = radius - whole;
  const blended = (values, horizontal) => {
    const inner = runningMax(values, width, height, 2 * whole + 1, horizontal);
    if (part === 0) return inner;
    const outer = runningMax(values, width, height, 2 * whole + 3, horizontal);
    return inner.map((value, i) => (1 - part) * value + part * outer[i]);
  };
  return blended(blended(coverage, true), false);
}

// Glyph coverage blurred by the shadow's Gaussian (zero outside), moved down and scaled by its opacity.
export function shadowed(coverage, width, height, style) {
  const sigma = style.shadowSigma;
  const radius = Math.max(1, Math.ceil(3 * sigma));
  const kernel = Array.from({ length: 2 * radius + 1 }, (_, i) => Math.exp(-((i - radius) ** 2) / (2 * sigma * sigma)));
  const total = kernel.reduce((sum, value) => sum + value, 0);
  const weights = kernel.map((value) => value / total);
  const pass = (values, horizontal) => {
    const out = new Float32Array(values.length);
    for (let row = 0; row < height; row++) {
      for (let column = 0; column < width; column++) {
        const at = horizontal ? column : row;
        const limit = horizontal ? width : height;
        const stride = horizontal ? 1 : width;
        const centre = row * width + column;
        let sum = 0;
        for (let k = Math.max(-radius, -at); k <= Math.min(radius, limit - 1 - at); k++) {
          sum += weights[k + radius] * values[centre + k * stride];
        }
        out[centre] = sum;
      }
    }
    return out;
  };
  const blurred = pass(pass(coverage, true), false);
  const whole = Math.floor(style.shadowOffsetY);
  const part = style.shadowOffsetY - whole;
  const result = new Float32Array(coverage.length);
  for (let row = 0; row < height; row++) {
    const upper = row - whole - 1;
    const lower = row - whole;
    for (let column = 0; column < width; column++) {
      const above = upper >= 0 && upper < height ? blurred[upper * width + column] : 0;
      const below = lower >= 0 && lower < height ? blurred[lower * width + column] : 0;
      result[row * width + column] = style.shadowOpacity * (part * above + (1 - part) * below);
    }
  }
  return result;
}

const byte = (value) => Math.floor(Math.min(Math.max(value, 0), 1) * 255 + 0.5);

// Straight-alpha RGBA8 of the title from its glyph coverage: shadow, black rim, white band, black
// keyline and the gradient face, "over" back to front in linear light, then encoded with the
// output gamma (the burned movie's code values).
export function compose({ coverage, width, height, originY, style, boxTop, boxHeight }) {
  const fill = grow(coverage, width, height, style.faceGrow);
  const keyline = grow(coverage, width, height, style.keyline);
  const white = grow(coverage, width, height, style.whiteBorder);
  const rim = grow(coverage, width, height, style.rim);
  const shadow = shadowed(coverage, width, height, style);
  const exponent = 1 / style.outputGamma;
  const rgba = new Uint8ClampedArray(width * height * 4);
  for (let row = 0; row < height; row++) {
    const colour = gradientAt(style, originY + row + 0.5, boxTop, boxHeight);
    for (let column = 0; column < width; column++) {
      const pixel = row * width + column;
      let alpha = shadow[pixel];
      alpha = rim[pixel] + (1 - rim[pixel]) * alpha;
      const band = white[pixel];
      let red = band;
      let green = band;
      let blue = band;
      alpha = band + (1 - band) * alpha;
      const line = keyline[pixel];
      red *= 1 - line;
      green *= 1 - line;
      blue *= 1 - line;
      alpha = line + (1 - line) * alpha;
      const face = fill[pixel];
      red = face * colour[0] + (1 - face) * red;
      green = face * colour[1] + (1 - face) * green;
      blue = face * colour[2] + (1 - face) * blue;
      alpha = face + (1 - face) * alpha;
      const offset = pixel * 4;
      rgba[offset + 3] = byte(alpha);
      if (!(alpha > 0)) continue;
      rgba[offset] = byte((red / alpha) ** exponent);
      rgba[offset + 1] = byte((green / alpha) ** exponent);
      rgba[offset + 2] = byte((blue / alpha) ** exponent);
    }
  }
  return rgba;
}

// The title's fixed layout. `measure(text)` gives the browser's advance width and ink bounds
// (TextMetrics) of a line; `ascent`/`descent` are the font's (CTFontGetAscent/Descent).
export function layoutTitle({ title, style, canvasWidth, canvasHeight, measure, ascent, descent }) {
  const advance = ascent + descent;
  const origin = anchor(title, canvasWidth, canvasHeight);
  const lines = lineSpans(title.text).map((span, index) => {
    const text = span.parts.join("");
    const metrics = text ? measure(text) : null;
    const left = title.alignment === "left" || !metrics ? origin.x : origin.x - metrics.width / 2;
    return { span, left, baseline: origin.baseline + index * advance, metrics };
  });
  const inked = lines.filter((line) => line.metrics);
  if (inked.length === 0) fail("title draws no glyphs");
  const minX = Math.min(...inked.map((l) => l.left - l.metrics.actualBoundingBoxLeft));
  const maxX = Math.max(...inked.map((l) => l.left + l.metrics.actualBoundingBoxRight));
  const minY = Math.min(...inked.map((l) => l.baseline - l.metrics.actualBoundingBoxAscent));
  const maxY = Math.max(...inked.map((l) => l.baseline + l.metrics.actualBoundingBoxDescent));
  const spread = style.rim + 2;
  const side = Math.max(spread, 3 * style.shadowSigma + 2);
  const below = Math.max(spread, style.shadowOffsetY + 3 * style.shadowSigma + 2);
  const originX = Math.floor(minX - side);
  const originY = Math.floor(minY - spread);
  return {
    lines,
    originX,
    originY,
    width: Math.ceil(maxX + side) - originX,
    height: Math.ceil(maxY + below) - originY,
    boxTop: origin.baseline - ascent,
    boxHeight: advance,
  };
}

// Caption position (docs/caption-plan.md "Caption position"): a mode and offsets per delivery,
// resolved per title with the same formulas as replay.caption_layout_file.resolve.
export const LAYOUT_MODES = ["plan", "aboveFace", "bottom", "top", "center"];
const LAYOUT_DEFAULTS = { offsetX: 0, offsetY: 0, margin: 120, faceGap: 60, minScore: 0.5, maxDrift: 150 };
const DEFAULT_LAYOUT = { version: 1, mode: "plan" };
const PADDING = 24;
const RIM_ANTIALIAS = 2;
const EM_ASCENT = 1.16;
const EM_ADVANCE = 1.448;
// One line's native measurement at the style-20 calibration: 150 px at 82.95580110497238 pt.
const EM_LINE_HEIGHT = 150 / 82.95580110497238;

// A viewer's stored override: only a known mode and finite offsets are kept.
function storedLayout(text) {
  try {
    const value = JSON.parse(text ?? "{}");
    return Object.fromEntries(
      Object.entries(value).filter(([key, item]) =>
        key === "mode" ? LAYOUT_MODES.includes(item) : ["offsetX", "offsetY"].includes(key) && finite(item),
      ),
    );
  } catch (error) {
    console.warn(`captions: ignoring a stored layout: ${error}`);
    return {};
  }
}

// The options a title resolves with: defaults, the layout's top-level values, then its speaker's.
const optionsFor = (layout, title) => ({ ...LAYOUT_DEFAULTS, ...layout, ...(layout.speakers?.[title.speaker ?? ""] ?? {}) });
const STACK_GAP = 12;

// Version 2's unassigned titles, in plan order, move up over the earlier ones they overlap in time
// (speaker_subtitles._LaneStack); `placed` is their box top. Not clamped afterwards.
function stacked(titles, placements) {
  const done = [];
  return placements.map((item, index) => {
    if (!item.stack) return item.placed;
    const { startFrame } = titles[index];
    const active = done.filter((other) => other.endFrame > startFrame);
    const height = item.box;
    const candidates = [item.placed + item.box, ...active.map((other) => other.top - STACK_GAP)].sort((a, b) => b - a);
    const fits = (bottom) => active.every((other) => bottom <= other.top - STACK_GAP || bottom - height >= other.bottom + STACK_GAP);
    const bottom = candidates.find(fits) ?? candidates.at(-1);
    done.push({ endFrame: titles[index].endFrame, top: bottom - item.box, bottom });
    return bottom - item.box;
  });
}

// CaptionPlacement.estimatedBlockWidth: the widest line at 0.6 em per ASCII character, 0.5 em per
// half-width form and 1 em per other character, with the rim on both sides.
export function estimatedBlockWidth(title, style) {
  const em = (character) => {
    const point = character.codePointAt(0);
    if (point < 0x80) return 0.6;
    return point >= 0xff61 && point <= 0xffdc ? 0.5 : 1;
  };
  const widest = Math.max(
    ...title.text.split("\n").map((line) => Array.from(line).reduce((total, character) => total + em(character), 0)),
  );
  return widest * style.fontSize + 2 * (style.rim + RIM_ANTIALIAS);
}

// CaptionPlacement.insideSafeZone: a centred title's block between the plan's margins, in the middle
// of them when it is wider than they leave. A plan without margins keeps only the anchor inside.
export function insideSafeZone(x, title, plan) {
  if (plan.safeLeft === undefined || plan.safeRight === undefined || title.alignment) return x;
  const low = plan.safeLeft - plan.width / 2;
  const high = plan.width / 2 - plan.safeRight;
  const half = estimatedBlockWidth(title, title.style ?? plan.style) / 2;
  return high - low <= 2 * half ? (low + high) / 2 : Math.max(low + half, Math.min(x, high - half));
}

// Every title's Motion position under `layout` and the basis of its vertical position.
// `faceTops` (one per title, from the catalog build) is [top, "face"|"held"] or null: the face top
// replay.caption_layout_file reads with that title's minScore and maxDrift (and, in version 2, its
// speaker's lane).
export function resolveTitles(plan, layout, faceTops) {
  const { height, width } = plan;
  const placements = plan.titles.map((title, index) => {
    const options = optionsFor(layout, title);
    const size = (title.style ?? plan.style).fontSize;
    const lines = title.text.split("\n").length;
    const top = height / 2 - title.y - PADDING - EM_ASCENT * size;
    const box = EM_LINE_HEIGHT * size + (lines - 1) * EM_ADVANCE * size;
    const face = faceTops?.[index] ?? null;
    const modes = {
      plan: [top, "plan"],
      top: [options.margin, "mode"],
      bottom: [height - options.margin - box, "mode"],
      center: [(height - box) / 2, "mode"],
      aboveFace: face
        ? [Math.max(options.margin, Math.min(height - options.margin, face[0] - options.faceGap) - box), face[1]]
        : [top, "plan"],
    };
    const [placed, basis] = modes[options.mode];
    return {
      placed: Math.max(0, Math.min(placed + options.offsetY, height - box)),
      box,
      size,
      basis,
      x: insideSafeZone(Math.max(-width / 2, Math.min(title.x + options.offsetX, width / 2)), title, plan),
      stack: plan.version === 2 && !title.speaker,
    };
  });
  return stacked(plan.titles, placements).map((placed, index) => ({
    x: placements[index].x,
    y: height / 2 - (placed + PADDING + EM_ASCENT * placements[index].size),
    basis: placements[index].basis,
  }));
}

// Viewer settings. Captions (per video) and the icon default to off when the plan says they are
// already burned into the movie; the face colours are global and default to the plan's.
export const SETTING_KEYS = {
  captions: (id) => `captions:show:${id}`,
  icon: (id) => `captions:icon:${id}`,
  layout: (id) => `captions:layout:${id}`,
  hidden: (id) => `captions:hidden:${id}`,
  gradient: "captions:gradient",
  top: "captions:top",
  bottom: "captions:bottom",
};

function read(storage, key) {
  try {
    return storage().getItem(key);
  } catch (error) {
    console.warn(`captions: reading ${key} failed: ${error}`);
    return null;
  }
}

export function writeSetting(storage, key, value) {
  try {
    if (value === null) storage().removeItem(key);
    else storage().setItem(key, value);
    return true;
  } catch (error) {
    console.warn(`captions: writing ${key} failed: ${error}`);
    return false;
  }
}

const flag = (value, fallback) => (value === "1" ? true : value === "0" ? false : fallback);
const colour = (value, fallback) => (value && HEX.test(value) ? value.toUpperCase() : fallback);

export function readSettings(storage, id, plan) {
  const gradient = flag(read(storage, SETTING_KEYS.gradient), true);
  const top = colour(read(storage, SETTING_KEYS.top), plan.style.gradientTop);
  const bottom = colour(read(storage, SETTING_KEYS.bottom), plan.style.gradientBottom);
  const custom = !gradient || top !== plan.style.gradientTop || bottom !== plan.style.gradientBottom;
  return {
    captions: flag(read(storage, SETTING_KEYS.captions(id)), !plan.burned),
    // Speaker icons are never burned, so a plan with them shows icons by default.
    icon: flag(
      read(storage, SETTING_KEYS.icon(id)),
      speakerIcons(plan).length > 0 || (plan.nameplate ? !plan.nameplate.burned : false),
    ),
    gradient,
    top,
    bottom,
    // Untouched settings keep every title's own colours, as the native overlay with no fill does.
    fill: custom ? { top, bottom, gradient } : null,
    // The delivery's caption layout, with this viewer's own position and offsets over it.
    layout: viewerLayout(plan.layout ?? DEFAULT_LAYOUT, storedLayout(read(storage, SETTING_KEYS.layout(id)))),
    // Speaker ids (and "unassigned") this viewer hid; positions are still resolved with them.
    hidden: storedHidden(read(storage, SETTING_KEYS.hidden(id))),
  };
}

// The viewer's choice wins over the file's, its per-speaker values included.
function viewerLayout(file, own) {
  if (!file.speakers) return { ...file, ...own };
  const speakers = Object.fromEntries(
    Object.entries(file.speakers).map(([id, options]) => [
      id,
      Object.fromEntries(Object.entries(options).filter(([key]) => !(key in own))),
    ]),
  );
  return { ...file, ...own, speakers };
}

function storedHidden(text) {
  try {
    const value = JSON.parse(text ?? "[]");
    return new Set(Array.isArray(value) ? value.filter((item) => typeof item === "string") : []);
  } catch (error) {
    console.warn(`captions: ignoring stored hidden speakers: ${error}`);
    return new Set();
  }
}

// The browser half: font, rasterizer and the overlay canvas.

// Loads the caption font once; the weight range makes the canvas use the variable font's real
// wght 700 instance rather than synthesizing bold from the default one.
let fontLoad = null;
export function loadFont(document, api) {
  fontLoad ??= new FontFace(FONT_FAMILY, `url(${api}${FONT_PATH}) format("woff2")`, { weight: "100 900" })
    .load()
    .then(
      (face) => {
        document.fonts.add(face);
        return face;
      },
      (error) => {
        // A failed fetch is retried by the next video instead of disabling captions until reload.
        fontLoad = null;
        throw error;
      },
    );
  return fontLoad;
}

export function fontOf(style) {
  const font = FONTS[style.fontName];
  if (!font) fail(`unsupported caption font ${style.fontName}`);
  return { css: `${font.weight} ${style.fontSize}px ${FONT_FAMILY}`, ascent: font.ascent * style.fontSize, descent: font.descent * style.fontSize };
}

const scratch = (width, height) => {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  return canvas;
};

// One title: laid out once, and its bitmap for any number of visible graphemes.
export function titleRenderer(title, style, plan) {
  const font = fontOf(style);
  const context = scratch(1, 1).getContext("2d");
  context.font = font.css;
  // Canvas text is not kerned unless asked; CoreText kerns (Noto's kana pairs move up to 5 px).
  context.textRendering = "geometricPrecision";
  const layout = layoutTitle({
    title,
    style,
    canvasWidth: plan.width,
    canvasHeight: plan.height,
    measure: (text) => context.measureText(text),
    ascent: font.ascent,
    descent: font.descent,
  });
  return {
    layout,
    // Straight-alpha RGBA8 of the layout's box with `visible` graphemes shown.
    pixels(visible) {
      const { width, height, originX, originY } = layout;
      const ctx = scratch(width, height).getContext("2d", { willReadFrequently: true });
      ctx.font = font.css;
      ctx.textRendering = "geometricPrecision";
      ctx.fillStyle = "#fff";
      ctx.translate(-originX, -originY);
      const coverage = new Float32Array(width * height);
      for (const line of layout.lines) {
        const text = linePrefix(line.span, visible);
        if (!text) continue;
        // Skia snaps a text baseline to whole pixels (x keeps subpixel positions): draw on the
        // pixel above and move the coverage down by the fraction, as CoreText places it.
        const whole = Math.floor(line.baseline);
        const part = line.baseline - whole;
        ctx.clearRect(originX, originY, width, height);
        ctx.fillText(text, line.left, whole);
        const pixels = ctx.getImageData(0, 0, width, height).data;
        for (let i = 0; i < coverage.length; i++) {
          const above = i >= width ? pixels[(i - width) * 4 + 3] : 0;
          coverage[i] = Math.min(1, coverage[i] + ((1 - part) * pixels[i * 4 + 3] + part * above) / 255);
        }
      }
      return compose({ coverage, width, height, originY, style, boxTop: layout.boxTop, boxHeight: layout.boxHeight });
    },
    bitmap(visible) {
      const canvas = scratch(layout.width, layout.height);
      canvas.getContext("2d").putImageData(new ImageData(this.pixels(visible), layout.width, layout.height), 0, 0);
      return canvas;
    },
  };
}

// The nameplate as replay.caption_layout.badge_image draws it, on a canvas of the plan's size.
async function loadImage(source) {
  const image = new Image();
  image.src = source;
  await image.decode();
  return image;
}

// A round icon: a ring-coloured disc with the icon, scaled to fill it, inside the ring.
function drawIconDisc(ctx, { image, x, y, size, ring, colour }) {
  const radius = size / 2;
  ctx.fillStyle = colour;
  ctx.beginPath();
  ctx.arc(x + radius, y + radius, radius, 0, 2 * Math.PI);
  ctx.fill();
  ctx.save();
  ctx.beginPath();
  ctx.arc(x + radius, y + radius, radius - ring, 0, 2 * Math.PI);
  ctx.clip();
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(image, x, y, size, size);
  ctx.restore();
}

// Version 2 speaker icons (docs/caption-plan.md "Speaker icons"): integer squares at the bottom
// edge centred on each lane, whatever the captions' layout.
const roundHalfUp = (value) => Math.floor(value + 0.5);
export function speakerIcons(plan) {
  const size = roundHalfUp(0.08 * plan.height);
  const ring = Math.max(2, roundHalfUp((size * 4) / 150));
  const y = plan.height - roundHalfUp(0.02 * plan.height) - size;
  return (plan.speakers ?? [])
    .filter((speaker) => speaker.icon !== undefined)
    .map((speaker) => ({
      id: speaker.id,
      x: Math.min(Math.max(roundHalfUp(plan.width / 2 + speaker.laneX - size / 2), 0), plan.width - size),
      y,
      size,
      ring,
    }));
}

export async function nameplateCanvas(plate, width, height) {
  const canvas = scratch(width, height);
  const ctx = canvas.getContext("2d");
  const image = await loadImage(plate.iconData);
  drawIconDisc(ctx, { image, x: plate.iconX, y: plate.iconY, size: plate.iconSize, ring: plate.ringWidth, colour: plate.ringColour });
  const font = fontOf({ fontName: "NotoSansJP-Thin_Bold", fontSize: plate.nameSize });
  ctx.font = font.css;
  ctx.textRendering = "geometricPrecision";
  ctx.textAlign = "center";
  const baseline = plate.nameY + font.ascent;
  ctx.lineJoin = "round";
  ctx.lineWidth = 2 * plate.nameStrokeWidth;
  ctx.strokeStyle = plate.nameStrokeColour;
  ctx.strokeText(plate.name, plate.nameX, baseline);
  ctx.fillStyle = plate.nameColour;
  ctx.fillText(plate.name, plate.nameX, baseline);
  return canvas;
}

// The caption states that follow `frame`: each showing title's next onset and the next title's
// first state (drawn ahead while idle, so a reveal never waits for its bitmap).
export function upcoming(titles, frame) {
  const onsets = titlesAt(titles, frame).flatMap((index) => {
    const onset = titles[index].graphemeFrames.find((at) => at > frame);
    return onset === undefined ? [] : [{ index, visible: visibleAt(titles[index], onset) }];
  });
  const next = startedBy(titles, frame);
  return next < titles.length ? [...onsets, { index: next, visible: visibleAt(titles[next], titles[next].graphemeFrames[0]) }] : onsets;
}

// Draws the plan's captions over `video` on `canvas` (which CSS keeps over the video's box) every
// animation frame. Only the showing state and the next one keep bitmaps, so memory stays bounded
// however long the plan. Titles stand where `settings.layout` resolves them (resolveTitles).
// `set()` changes settings; `stop()` ends drawing.
export function overlay({ video, canvas, plan, settings, window }) {
  const positioned = (next) => resolveTitles(plan, next.layout, plan.faceTops ?? null);
  const speakers = new Map((plan.speakers ?? []).map((speaker) => [speaker.id, speaker]));
  // A speaker's titles take its two colours (one, its top, without the gradient); the viewer's own
  // colours recolour the unassigned titles (every title of a version 1 plan).
  const fillFor = (title) => {
    const speaker = speakers.get(title.speaker);
    return speaker ? { top: speaker.colorTop, bottom: speaker.colorBottom, gradient: state.settings.gradient } : state.settings.fill;
  };
  const shownTitle = (index) => !state.settings.hidden.has(plan.titles[index].speaker ?? UNASSIGNED);
  const state = { settings, positions: positioned(settings), plate: null, renderers: new Map(), bitmaps: new Map(), drawn: "", frame: 0, idle: 0 };
  const later = window.requestIdleCallback ? (task) => window.requestIdleCallback(task, { timeout: 200 }) : (task) => window.setTimeout(task, 0);
  const renderer = (index) => {
    let found = state.renderers.get(index);
    if (!found) {
      const title = plan.titles[index];
      const { x, y } = state.positions[index];
      found = titleRenderer({ ...title, x, y }, applyFill(title.style ?? plan.style, fillFor(title)), plan);
      state.renderers.set(index, found);
    }
    return found;
  };
  const bitmap = (index, visible) => {
    const key = `${index}:${visible}`;
    if (!state.bitmaps.has(key)) state.bitmaps.set(key, renderer(index).bitmap(visible));
    return state.bitmaps.get(key);
  };
  // Keeps only the given states (and their titles' layouts).
  const keep = (states) => {
    const keys = new Set(states.map((item) => `${item.index}:${item.visible}`));
    for (const key of state.bitmaps.keys()) if (!keys.has(key)) state.bitmaps.delete(key);
    const titles = new Set(states.map((item) => item.index));
    for (const index of state.renderers.keys()) if (!titles.has(index)) state.renderers.delete(index);
  };
  const plateReady = plan.nameplate?.iconData
    ? nameplateCanvas(plan.nameplate, plan.width, plan.height).then((plate) => {
        state.plate = plate;
        state.drawn = "";
      }, (error) => console.warn(`captions: nameplate failed: ${error}`))
    : null;
  const iconSources = new Map((plan.speakers ?? []).map((speaker) => [speaker.id, speaker.iconData]));
  Promise.all(
    speakerIcons(plan).map(async (place) => ({ ...place, image: await loadImage(iconSources.get(place.id)) })),
  ).then((icons) => {
    state.speakerIcons = icons;
    state.drawn = "";
  }, (error) => console.warn(`captions: speaker icons failed: ${error}`));
  const draw = () => {
    const ratio = window.devicePixelRatio || 1;
    const boxWidth = canvas.clientWidth;
    const boxHeight = canvas.clientHeight;
    const frame = frameAt(video.currentTime, plan.rate);
    const showing = state.settings.captions
      ? titlesAt(plan.titles, frame)
          .filter(shownTitle)
          .map((index) => ({ index, visible: visibleAt(plan.titles[index], frame) }))
          .filter((item) => item.visible > 0)
      : [];
    const icon = state.settings.icon && state.plate;
    const icons = state.settings.icon ? (state.speakerIcons ?? []).filter((item) => !state.settings.hidden.has(item.id)) : [];
    const key = `${boxWidth}x${boxHeight}@${ratio}:${showing.map((item) => `${item.index}.${item.visible}`).join()}:${Boolean(icon)}:${icons.map((item) => item.id).join()}`;
    if (key === state.drawn) return;
    state.drawn = key;
    const shown = showing.map((item) => ({ ...item, bitmap: bitmap(item.index, item.visible) }));
    const next = state.settings.captions ? upcoming(plan.titles, frame).filter((item) => shownTitle(item.index)) : [];
    keep([...showing, ...next]);
    window.cancelIdleCallback?.(state.idle);
    if (next.length > 0) state.idle = later(() => next.forEach((item) => bitmap(item.index, item.visible)));
    const width = Math.round(boxWidth * ratio);
    const height = Math.round(boxHeight * ratio);
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
    const ctx = canvas.getContext("2d");
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, width, height);
    if (!width || !height) return;
    const fit = fitRect(width, height, plan.width, plan.height);
    ctx.setTransform(fit.scale, 0, 0, fit.scale, fit.x, fit.y);
    ctx.imageSmoothingQuality = "high";
    if (icon) ctx.drawImage(state.plate, 0, 0);
    for (const item of icons) drawIconDisc(ctx, { ...item, colour: "#FFFFFF" });
    for (const item of shown) ctx.drawImage(item.bitmap, renderer(item.index).layout.originX, renderer(item.index).layout.originY);
  };
  const tick = () => {
    try {
      draw();
    } catch (error) {
      console.warn(`captions: drawing failed: ${error}`);
    }
    state.frame = window.requestAnimationFrame(tick);
  };
  state.frame = window.requestAnimationFrame(tick);
  return {
    ready: plateReady,
    set(next) {
      state.settings = next;
      state.positions = positioned(next);
      keep([]);
      state.drawn = "";
    },
    stop() {
      window.cancelAnimationFrame(state.frame);
      window.cancelIdleCallback?.(state.idle);
      keep([]);
      canvas.getContext("2d").clearRect(0, 0, canvas.width, canvas.height);
    },
  };
}
