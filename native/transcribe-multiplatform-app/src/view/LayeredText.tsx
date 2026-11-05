// Runs with bun: draws a caption the way the subtitle files do: outer edge, outline, then the fill,
// so one glyph's outline never paints over its neighbour.
import type { VNode } from "preact";
import type { SpeakerColor } from "../backend/types.ts";
import type { FrameSize } from "../layout/preview-layout.ts";

export interface LayeredTextProps {
  lines: string[];
  /** Centre of the block. */
  x: number;
  y: number;
  fontFamily: string;
  fontPx: number;
  lineHeightPx: number;
  color: SpeakerColor;
  /** Pixels per unit of a layer's width ratio. */
  strokeUnitPx: number;
  /** The shadow's blur follows its short edge. */
  frame: FrameSize;
}

interface ShadowScale {
  fontPx: number;
  frame: FrameSize;
}

interface Layer {
  fill: string;
  stroke: string;
  strokeWidth: number;
}

/** Stands in for a chosen family this computer does not have. */
const FALLBACK_FONTS: string = '"Noto Sans JP", "Hiragino Sans", "Yu Gothic UI", sans-serif';
const BASELINE_EM: number = 0.35;
const STROKE_SIDES: number = 2;
const SHADOW_OFFSET_EMS: number = 2;
const SHADOW_BLUR_PER_VALUE: number = 0.5;
const SHADOW_BLUR_SHORT_EDGE: number = 720;

/** Outermost first; each stroke reaches past the layers inside it. A layer of no width is not drawn. */
export const captionLayers = (
  {
    fill,
    fill_width: fillWidth,
    outline,
    outline_width: outlineWidth,
    rim,
    rim_width: rimWidth,
  }: SpeakerColor,
  strokeUnitPx: number,
): Layer[] => [
  ...(rim !== null && rimWidth > 0
    ? [
        {
          fill: rim,
          stroke: rim,
          strokeWidth: STROKE_SIDES * strokeUnitPx * (fillWidth + outlineWidth + rimWidth),
        },
      ]
    : []),
  ...(outlineWidth > 0
    ? [
        {
          fill: outline,
          stroke: outline,
          strokeWidth: STROKE_SIDES * strokeUnitPx * (fillWidth + outlineWidth),
        },
      ]
    : []),
  fillWidth > 0
    ? { fill, stroke: fill, strokeWidth: STROKE_SIDES * strokeUnitPx * fillWidth }
    : { fill, stroke: "none", strokeWidth: 0 },
];

/**
 * The offset is measured in Resolve 21.1.2 at 720x1280: twice the value, in ems.
 * shortcut: the blur is judged by eye, and that it follows the short edge is unverified; measure it to replace this.
 */
export const shadowFilter = (
  { shadow, shadow_x: x, shadow_y: y, shadow_blur: blur }: SpeakerColor,
  { fontPx, frame }: ShadowScale,
): string =>
  shadow === null
    ? "none"
    : `drop-shadow(${x * SHADOW_OFFSET_EMS * fontPx}px ${-y * SHADOW_OFFSET_EMS * fontPx}px ${
        (blur * SHADOW_BLUR_PER_VALUE * Math.min(frame.width, frame.height)) / SHADOW_BLUR_SHORT_EDGE
      }px ${shadow})`;

export const fontStack = (family: string): string => `"${family}", ${FALLBACK_FONTS}`;

export const LayeredText = ({
  lines,
  x,
  y,
  fontFamily,
  fontPx,
  lineHeightPx,
  color,
  strokeUnitPx,
  frame,
}: LayeredTextProps): VNode => (
  <g
    fontFamily={fontStack(fontFamily)}
    fontSize={fontPx}
    fontWeight={700}
    textAnchor="middle"
    strokeLinejoin="round"
    style={{ filter: shadowFilter(color, { fontPx, frame }) }}
  >
    {captionLayers(color, strokeUnitPx).map(
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
