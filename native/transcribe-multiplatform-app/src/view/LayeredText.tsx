// Runs with bun: draws a caption the way the subtitle files do: shadow, black rim, outline band, then the bare fill,
// so one glyph's outline never paints over its neighbour.
import type { VNode } from "preact";
import type { CaptionShadow, SpeakerColor } from "../backend/types.ts";

export interface LayeredTextProps {
  lines: string[];
  /** Centre of the block. */
  x: number;
  y: number;
  fontFamily: string;
  fontPx: number;
  lineHeightPx: number;
  color: SpeakerColor;
  outlinePx: number;
  rimPx: number;
  shadow: CaptionShadow;
}

interface Layer {
  fill: string;
  stroke: string;
  strokeWidth: number;
}

const BLACK: string = "#000000";
/** Stands in for a chosen family this computer does not have. */
const FALLBACK_FONTS: string = '"Noto Sans JP", "Hiragino Sans", "Yu Gothic UI", sans-serif';
/** Style 20's shadow straight down, in em. */
const SHADOW_OFFSET_EM: number = 16 / 88;
const SHADOW_BLUR_EM: number = 8 / 88;
const BASELINE_EM: number = 0.35;
const STROKE_SIDES: number = 2;

export const captionLayers = ({ fill, outline }: SpeakerColor, outlinePx: number, rimPx: number): Layer[] => [
  ...(rimPx > 0 && outline.toUpperCase() !== BLACK
    ? [{ fill: BLACK, stroke: BLACK, strokeWidth: STROKE_SIDES * (outlinePx + rimPx) }]
    : []),
  { fill: outline, stroke: outline, strokeWidth: STROKE_SIDES * outlinePx },
  { fill, stroke: "none", strokeWidth: 0 },
];

export const fontStack = (family: string): string => `"${family}", ${FALLBACK_FONTS}`;

export const shadowFilter = ({ enabled, strength }: CaptionShadow, fontPx: number): string =>
  enabled
    ? `drop-shadow(0 ${fontPx * SHADOW_OFFSET_EM}px ${fontPx * SHADOW_BLUR_EM}px rgb(0 0 0 / ${strength}))`
    : "none";

export const LayeredText = ({
  lines,
  x,
  y,
  fontFamily,
  fontPx,
  lineHeightPx,
  color,
  outlinePx,
  rimPx,
  shadow,
}: LayeredTextProps): VNode => (
  <g
    fontFamily={fontStack(fontFamily)}
    fontSize={fontPx}
    fontWeight={700}
    textAnchor="middle"
    strokeLinejoin="round"
    style={{ filter: shadowFilter(shadow, fontPx) }}
  >
    {captionLayers(color, outlinePx, rimPx).map(
      ({ fill, stroke, strokeWidth }: Layer, layer: number): VNode => (
        <g key={String(layer)} fill={fill} stroke={stroke} strokeWidth={strokeWidth}>
          {lines.map(
            (line: string, index: number): VNode => (
              <text
                key={String(index)}
                x={x}
                y={y + (index - (lines.length - 1) / 2) * lineHeightPx + fontPx * BASELINE_EM}
              >
                {line}
              </text>
            ),
          )}
        </g>
      ),
    )}
  </g>
);
