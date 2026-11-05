// Runs with bun: mirrors src-tauri/src/captions.rs so the sliders preview what the files will contain.
import type { Settings } from "../backend/types.ts";

export interface FrameSize {
  width: number;
  height: number;
}

export interface FaceBox {
  centerX: number;
  top: number;
  bottom: number;
  headTop: number;
}

export interface PreviewLayout {
  fontPx: number;
  outlinePx: number;
  rimPx: number;
  lineHeightPx: number;
  lines: string[];
  x: number;
  y: number;
  face: FaceBox | null;
}

interface BlockSize {
  width: number;
  height: number;
}

interface Metrics {
  fontPx: number;
  outlinePx: number;
  maxLinePx: number;
}

interface WordRun {
  start: number;
  end: number;
}

interface WrapText {
  glyphs: string[];
  /** Running width: `widths[n]` is the first `n` glyphs. */
  widths: number[];
  wordRuns: WordRun[];
  maxLinePx: number;
}

interface BreakQuery {
  text: WrapText;
  start: number;
  end: number;
  keepWords: boolean;
}

/** The sample face: below the middle of the frame, a fifth of the short edge tall. */
const FACE_CENTER_Y_RATIO: number = 0.6;
const FACE_HEIGHT_RATIO: number = 0.2;
/** `HEAD_EXTENSION_FACE_HEIGHT_RATIO`: hair and head reach this far above a measured face. */
const HEAD_EXTENSION_RATIO: number = 1.25;
/** `caption_layout_file`: one line's measured box, and the distances at a 1080 short edge. */
const EM_LINE_HEIGHT: number = 150 / 82.955_801_104_972_38;
const REFERENCE_SHORT_EDGE: number = 1080;
const MARGIN_PX: number = 120;
const FACE_GAP_PX: number = 60;
const PERCENT: number = 100;
/** `caption_safe_wrap` in captions.rs: a conservative estimate, so real glyphs fit. */
const NARROW_GLYPH_EM: number = 0.6;
const HALF_WIDTH_GLYPH_EM: number = 0.5;
const LAST_ASCII: string = "\u007f";
const HALF_WIDTH: RegExp = /^[\uff61-\uffdc]$/;
const SPACE: RegExp = /^\s$/;
const WORD_RUN: RegExp = /[\p{Script=Latin}\p{N}_]+|[\p{Script=Katakana}ー]+/gu;
/** `NO_LINE_START` with `DEPENDENT_KANA`, and `NO_LINE_END`, from captions.rs. */
const NO_LINE_START: ReadonlySet<string> = new Set(
  Array.from("、。！？!?.,;:…」』）)]｝〜～ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶーﾞﾟ々ゝゞヽヾ"),
);
const NO_LINE_END: ReadonlySet<string> = new Set(Array.from("「『（([｛"));
const FACE_ANCHORS: ReadonlySet<string> = new Set(["face_above", "face_below"]);

export const metricsFor = ({ width, height }: FrameSize, settings: Settings): Metrics => {
  const fontPx: number = Math.max(1, Math.round(Math.min(width, height) * settings.font_ratio));
  const outlinePx: number = Math.max(1, fontPx * settings.outline_ratio);
  const edgePx: number = outlinePx + fontPx * settings.rim_ratio;
  return { fontPx, outlinePx, maxLinePx: Math.max(fontPx, width * settings.width_ratio - 2 * edgePx) };
};

const glyphWidth = (glyph: string, fontPx: number): number => {
  if (glyph <= LAST_ASCII) {
    return fontPx * NARROW_GLYPH_EM;
  }
  return HALF_WIDTH.test(glyph) ? fontPx * HALF_WIDTH_GLYPH_EM : fontPx;
};

const measureLine = (line: string, fontPx: number): number => {
  const [first, ...rest] = Array.from(line);
  return first === undefined ? 0 : glyphWidth(first, fontPx) + measureLine(rest.join(""), fontPx);
};

const textWidth = ({ widths }: WrapText, start: number, end: number): number => widths[end] - widths[start];

/** `start..end` without the spaces at its edges, as `trimmed` in captions.rs. */
function visibleSpan({ glyphs }: WrapText, start: number, end: number): [number, number] {
  const isText = (glyph: string): boolean => !SPACE.test(glyph);
  const inside: string[] = glyphs.slice(start, end);
  const first: number = inside.findIndex(isText);
  return first < 0 ? [start, start] : [start + first, start + inside.findLastIndex(isText) + 1];
}

/** Kinsoku at a line break, looking past the space the break removes. */
function mayBreakLine(text: WrapText, point: number): boolean {
  const [before, after] = visibleSpan(text, point, text.glyphs.length);
  const [head, last] = visibleSpan(text, 0, point);
  return (
    before < after &&
    head < last &&
    !NO_LINE_START.has(text.glyphs[before]) &&
    !NO_LINE_END.has(text.glyphs[last - 1])
  );
}

const wordRunsOf = (
  glyphs: string[],
  widthOf: (start: number, end: number) => number,
  maxLinePx: number,
): WordRun[] =>
  Array.from(glyphs.join("").matchAll(WORD_RUN), (match: RegExpExecArray): WordRun => {
    const start: number = Array.from(glyphs.join("").slice(0, match.index)).length;
    return { start, end: start + Array.from(match[0]).length };
  }).filter(({ start, end }: WordRun): boolean => end - start > 1 && widthOf(start, end) <= maxLinePx);

const breakPoints = ({ text, start, end, keepWords }: BreakQuery): number[] =>
  Array.from(
    { length: Math.max(0, end - start - 1) },
    (_, offset: number): number => start + offset + 1,
  ).filter(
    (point: number): boolean =>
      mayBreakLine(text, point) &&
      !(keepWords && text.wordRuns.some((run: WordRun): boolean => run.start < point && point < run.end)),
  );

const sidesOf = (text: WrapText, start: number, point: number, end: number): [number, number] => {
  const [leftStart, leftEnd] = visibleSpan(text, start, point);
  const [rightStart, rightEnd] = visibleSpan(text, point, end);
  return [textWidth(text, leftStart, leftEnd), textWidth(text, rightStart, rightEnd)];
};

const lineOf = (text: WrapText, start: number, end: number): string => {
  const [first, last] = visibleSpan(text, start, end);
  return text.glyphs.slice(first, last).join("");
};

/** The evenest split into two lines that both fit, as `balanced_split` in captions.rs. */
function evenestSplit(text: WrapText, points: number[], start: number, end: number): number | undefined {
  const fitting: number[] = points.filter((point: number): boolean =>
    sidesOf(text, start, point, end).every((side: number): boolean => side <= text.maxLinePx),
  );
  const imbalance = (point: number): number => {
    const [left, right] = sidesOf(text, start, point, end);
    return Math.abs(left - right);
  };
  return fitting.toSorted((first: number, second: number): number => imbalance(first) - imbalance(second))[0];
}

/** Two evenly filled lines when they can hold it; words stay whole unless one alone is too long. */
function wrapSpan(text: WrapText, start: number, end: number): string[] {
  const [first, last] = visibleSpan(text, start, end);
  if (first === last) {
    return [];
  }
  if (textWidth(text, first, last) <= text.maxLinePx) {
    return [lineOf(text, first, last)];
  }
  const wholeWords: number[] = breakPoints({ text, start: first, end: last, keepWords: true });
  const points: number[] =
    wholeWords.length > 0 ? wholeWords : breakPoints({ text, start: first, end: last, keepWords: false });
  const split: number | undefined = evenestSplit(text, points, first, last);
  if (split !== undefined) {
    return [lineOf(text, first, split), lineOf(text, split, last)];
  }
  // shortcut: the files move what does not fit two lines to a later cue, with morphology; the sample shows it as more lines.
  const longest: number | undefined = points.findLast(
    (point: number): boolean => sidesOf(text, first, point, last)[0] <= text.maxLinePx,
  );
  return longest === undefined
    ? [lineOf(text, first, last)]
    : [lineOf(text, first, longest), ...wrapSpan(text, longest, last)];
}

/**
 * The same glyph widths, whole words, kinsoku and even two-line split as captions.rs. Its phrase ends need the
 * morphological dictionary, which the window does not have, so Japanese may break elsewhere within a line pair.
 */
export function wrapText(
  line: string,
  { fontPx, maxLinePx }: Pick<Metrics, "fontPx" | "maxLinePx">,
): string[] {
  const glyphs: string[] = Array.from(line);
  const widths: number[] = [0];
  glyphs.forEach((glyph: string, index: number): void => {
    widths.push(widths[index] + glyphWidth(glyph, fontPx));
  });
  const wordRuns: WordRun[] = wordRunsOf(
    glyphs,
    (start: number, end: number): number => widths[end] - widths[start],
    maxLinePx,
  );
  return wrapSpan({ glyphs, widths, wordRuns, maxLinePx }, 0, glyphs.length);
}

const clampCenter = (center: number, block: number, frame: number): number =>
  block >= frame ? frame / 2 : Math.min(frame - block / 2, Math.max(block / 2, center));

export const sampleFace = ({ width, height }: FrameSize): FaceBox => {
  const faceHeight: number = Math.min(width, height) * FACE_HEIGHT_RATIO;
  const top: number = height * FACE_CENTER_Y_RATIO - faceHeight / 2;
  return {
    centerX: width / 2,
    top,
    bottom: top + faceHeight,
    headTop: top - faceHeight * HEAD_EXTENSION_RATIO,
  };
};

/** Above the head when there is room, else below the face: captions never sit on the face. */
const aroundFaceY = (above: boolean, face: FaceBox, block: BlockSize, gap: number): number => {
  const aboveY: number = face.headTop - gap - block.height / 2;
  return above && aboveY >= block.height / 2 ? aboveY : face.bottom + gap + block.height / 2;
};

const anchorY = (anchor: Settings["placement"]["anchor"], block: BlockSize, frame: FrameSize): number => {
  const scale: number = Math.min(frame.width, frame.height) / REFERENCE_SHORT_EDGE;
  return {
    face_above: (): number => aroundFaceY(true, sampleFace(frame), block, FACE_GAP_PX * scale),
    face_below: (): number => aroundFaceY(false, sampleFace(frame), block, FACE_GAP_PX * scale),
    video_top: (): number => MARGIN_PX * scale + block.height / 2,
    video_bottom: (): number => frame.height - MARGIN_PX * scale - block.height / 2,
  }[anchor]();
};

export function previewLayout(text: string, frame: FrameSize, settings: Settings): PreviewLayout {
  const { fontPx, outlinePx, maxLinePx } = metricsFor(frame, settings);
  const lines: string[] = wrapText(text, { fontPx, maxLinePx });
  const block: BlockSize = {
    width:
      Math.max(...lines.map((line: string): number => measureLine(line, fontPx))) +
      2 * (outlinePx + fontPx * settings.rim_ratio),
    height: fontPx * (EM_LINE_HEIGHT + (lines.length - 1) * settings.line_height),
  };
  const { anchor, offset_x_percent: offsetX, offset_y_percent: offsetY } = settings.placement;
  return {
    fontPx,
    outlinePx,
    rimPx: fontPx * settings.rim_ratio,
    lineHeightPx: fontPx * settings.line_height,
    lines,
    x: clampCenter(frame.width / 2 + (offsetX / PERCENT) * frame.width, block.width, frame.width),
    y: clampCenter(
      anchorY(anchor, block, frame) + (offsetY / PERCENT) * frame.height,
      block.height,
      frame.height,
    ),
    face: FACE_ANCHORS.has(anchor) ? sampleFace(frame) : null,
  };
}
