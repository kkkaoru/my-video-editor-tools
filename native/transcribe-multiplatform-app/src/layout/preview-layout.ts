// Runs with bun: mirrors src-tauri/src/captions.rs so the sliders preview what the files will contain.
import type { Anchor, Settings } from "../backend/types.ts";

export interface FrameSize {
  width: number;
  height: number;
}

export interface PreviewLayout {
  fontPx: number;
  outlinePx: number;
  lines: string[];
  x: number;
  y: number;
  usesFace: boolean;
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

const FACE_TOP_RATIO: number = 0.22;
const FACE_BOTTOM_RATIO: number = 0.5;
const FACE_GAP_EM: number = 0.46;
const VIDEO_MARGIN_RATIO: number = 0.05;
const LINE_HEIGHT: number = 1.25;
const PERCENT: number = 100;
const FACE_ANCHORS: ReadonlySet<string> = new Set(["face_above", "face_below"]);

export const metricsFor = ({ width, height }: FrameSize, settings: Settings): Metrics => {
  const fontPx: number = Math.max(1, Math.round(Math.min(width, height) * settings.font_ratio));
  const outlinePx: number = Math.max(1, fontPx * settings.outline_ratio);
  return { fontPx, outlinePx, maxLinePx: Math.max(fontPx, width * settings.width_ratio - 2 * outlinePx) };
};

export const wrapText = (text: string, perLine: number): string[] =>
  Array.from({ length: Math.ceil(text.length / perLine) }, (_, index: number) =>
    text.slice(index * perLine, (index + 1) * perLine),
  );

const clampCenter = (center: number, block: number, frame: number): number =>
  block >= frame ? frame / 2 : Math.min(frame - block / 2, Math.max(block / 2, center));

const anchorY = (anchor: Anchor, block: BlockSize, frame: FrameSize, gap: number): number =>
  (
    ({
      face_above: frame.height * FACE_TOP_RATIO - gap - block.height / 2,
      face_below: frame.height * FACE_BOTTOM_RATIO + gap + block.height / 2,
      video_top: frame.height * VIDEO_MARGIN_RATIO + block.height / 2,
      video_bottom: frame.height * (1 - VIDEO_MARGIN_RATIO) - block.height / 2,
    }) satisfies Record<Anchor, number>
  )[anchor];

export function previewLayout(text: string, frame: FrameSize, settings: Settings): PreviewLayout {
  const { fontPx, outlinePx, maxLinePx } = metricsFor(frame, settings);
  const lines: string[] = wrapText(text, Math.max(1, Math.floor(maxLinePx / fontPx)));
  const block: BlockSize = {
    width: Math.max(...lines.map((line: string): number => line.length)) * fontPx + 2 * outlinePx,
    height: lines.length * fontPx * LINE_HEIGHT + 2 * outlinePx,
  };
  const { anchor, offset_x_percent: offsetX, offset_y_percent: offsetY } = settings.placement;
  return {
    fontPx,
    outlinePx,
    lines,
    x: clampCenter(frame.width / 2 + (offsetX / PERCENT) * frame.width, block.width, frame.width),
    y: clampCenter(
      anchorY(anchor, block, frame, fontPx * FACE_GAP_EM) + (offsetY / PERCENT) * frame.height,
      block.height,
      frame.height,
    ),
    usesFace: FACE_ANCHORS.has(anchor),
  };
}
