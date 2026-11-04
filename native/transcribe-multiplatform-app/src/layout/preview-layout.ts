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

/** The sample face: below the middle of the frame, a fifth of the short edge tall. */
const FACE_CENTER_Y_RATIO: number = 0.6;
const FACE_HEIGHT_RATIO: number = 0.2;
/** `HEAD_EXTENSION_FACE_HEIGHT_RATIO`: hair and head reach this far above a measured face. */
const HEAD_EXTENSION_RATIO: number = 1.25;
const FACE_GAP_EM: number = 0.46;
const VIDEO_MARGIN_RATIO: number = 0.05;
const PERCENT: number = 100;
const WRAP_ORPHAN_MAXIMUM: number = 4;
const FACE_ANCHORS: ReadonlySet<string> = new Set(["face_above", "face_below"]);

export const metricsFor = ({ width, height }: FrameSize, settings: Settings): Metrics => {
  const fontPx: number = Math.max(1, Math.round(Math.min(width, height) * settings.font_ratio));
  const outlinePx: number = Math.max(1, fontPx * settings.outline_ratio);
  return { fontPx, outlinePx, maxLinePx: Math.max(fontPx, width * settings.width_ratio - 2 * outlinePx) };
};

const sliceLines = (text: string, perLine: number): string[] =>
  Array.from({ length: Math.ceil(text.length / perLine) }, (_, index: number) =>
    text.slice(index * perLine, (index + 1) * perLine),
  );

const shareLastTwoLines = (lines: string[]): string[] => {
  const tail: string = lines.slice(-2).join("");
  return [...lines.slice(0, -2), ...sliceLines(tail, Math.ceil(tail.length / 2))];
};

/** Like captions.rs, a short last line ("す。") shares the line above it evenly; earlier lines stay. */
export const wrapText = (text: string, perLine: number): string[] => {
  const lines: string[] = sliceLines(text, perLine);
  const lastLength: number = text.length - (lines.length - 1) * perLine;
  return lines.length > 1 && lastLength <= WRAP_ORPHAN_MAXIMUM ? shareLastTwoLines(lines) : lines;
};

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

const anchorY = (
  anchor: Settings["placement"]["anchor"],
  block: BlockSize,
  frame: FrameSize,
  gap: number,
): number =>
  ({
    face_above: (): number => aroundFaceY(true, sampleFace(frame), block, gap),
    face_below: (): number => aroundFaceY(false, sampleFace(frame), block, gap),
    video_top: (): number => frame.height * VIDEO_MARGIN_RATIO + block.height / 2,
    video_bottom: (): number => frame.height * (1 - VIDEO_MARGIN_RATIO) - block.height / 2,
  })[anchor]();

export function previewLayout(text: string, frame: FrameSize, settings: Settings): PreviewLayout {
  const { fontPx, outlinePx, maxLinePx } = metricsFor(frame, settings);
  const lines: string[] = wrapText(text, Math.max(1, Math.floor(maxLinePx / fontPx)));
  const block: BlockSize = {
    width: Math.max(...lines.map((line: string): number => line.length)) * fontPx + 2 * outlinePx,
    height: lines.length * fontPx * settings.line_height + 2 * (outlinePx + fontPx * settings.rim_ratio),
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
      anchorY(anchor, block, frame, fontPx * FACE_GAP_EM) + (offsetY / PERCENT) * frame.height,
      block.height,
      frame.height,
    ),
    face: FACE_ANCHORS.has(anchor) ? sampleFace(frame) : null,
  };
}
