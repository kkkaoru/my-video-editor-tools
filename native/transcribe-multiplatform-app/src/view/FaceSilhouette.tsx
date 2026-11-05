// Runs with bun: the person in the preview, a bust whose head fills exactly the area captions keep away from.
import type { VNode } from "preact";
import type { FaceBox, FrameSize } from "../layout/preview-layout.ts";

export interface FaceSilhouetteProps {
  face: FaceBox;
  frame: FrameSize;
}

/** Between the preview's bright and dark halves: 3:1 or better against both (WCAG 1.4.11). */
export const SILHOUETTE_FILL: string = "#707070";
export const SKIN_FILL: string = "#e8c9b4";

/** Head proportions, relative to the head's height: `headTop` to the chin, the area captions avoid. */
const HEAD_WIDTH_RATIO: number = 0.36;
const HAIR_BACK_RATIO: number = 0.5;
const FACE_TOP_RATIO: number = 0.3;
const FACE_WIDTH_RATIO: number = 0.29;
const TEMPLE_RATIO: number = 0.5;
const FRINGE_RATIO: number = 0.46;
/** Body proportions, relative to the head's height too, so the figure keeps its shape in any frame. */
const NECK_WIDTH_RATIO: number = 0.1;
const NECK_BASE_RATIO: number = 0.14;
const SHOULDER_WIDTH_RATIO: number = 0.72;
const SHOULDER_DROP_RATIO: number = 0.36;
const CHEST_DROP_RATIO: number = 1.1;

const headHeight = ({ bottom, headTop }: FaceBox): number => bottom - headTop;

/** Hair over the crown, parted off-centre, sweeping down across the forehead to one temple. */
export const fringePath = (face: FaceBox): string => {
  const { centerX, headTop } = face;
  const height: number = headHeight(face);
  const half: number = height * HEAD_WIDTH_RATIO;
  const temple: number = headTop + height * TEMPLE_RATIO;
  const part: number = centerX + half * 0.35;
  const partY: number = headTop + height * 0.28;
  return [
    `M ${centerX - half} ${temple + height * 0.06}`,
    `C ${centerX - half} ${headTop - height * 0.1}, ${centerX + half} ${headTop - height * 0.1}, ${centerX + half} ${temple}`,
    `Q ${centerX + half * 0.8} ${partY + height * 0.02}, ${part} ${partY}`,
    `Q ${centerX - half * 0.3} ${headTop + height * FRINGE_RATIO}, ${centerX - half} ${temple + height * 0.06}`,
    "Z",
  ].join(" ");
};

/** Shoulders and chest, from the base of the neck down; cut off by the frame if it ends first. */
export const bustPath = (face: FaceBox, { height }: FrameSize): string => {
  const { centerX, bottom } = face;
  const unit: number = headHeight(face);
  const neck: number = unit * NECK_WIDTH_RATIO;
  const shoulder: number = unit * SHOULDER_WIDTH_RATIO;
  const base: number = bottom + unit * NECK_BASE_RATIO;
  const shoulderY: number = bottom + unit * SHOULDER_DROP_RATIO;
  const chestY: number = Math.min(height, bottom + unit * CHEST_DROP_RATIO);
  return [
    `M ${centerX - neck} ${base}`,
    `C ${centerX - neck * 2} ${shoulderY - unit * 0.1}, ${centerX - shoulder} ${shoulderY - unit * 0.08}, ${centerX - shoulder} ${shoulderY + unit * 0.14}`,
    `L ${centerX - shoulder} ${chestY}`,
    `L ${centerX + shoulder} ${chestY}`,
    `L ${centerX + shoulder} ${shoulderY + unit * 0.14}`,
    `C ${centerX + shoulder} ${shoulderY - unit * 0.08}, ${centerX + neck * 2} ${shoulderY - unit * 0.1}, ${centerX + neck} ${base}`,
    "Z",
  ].join(" ");
};

export function FaceSilhouette({ face, frame }: FaceSilhouetteProps): VNode {
  const { centerX, headTop, bottom } = face;
  const height: number = headHeight(face);
  const faceTop: number = headTop + height * FACE_TOP_RATIO;
  const hairRadius: number = height * HAIR_BACK_RATIO;
  const neck: number = height * NECK_WIDTH_RATIO;
  return (
    <g data-slot="face-silhouette">
      {/* Hair behind the head, then neck and body, then the face, then the fringe on top of it. */}
      <ellipse
        cx={centerX}
        cy={headTop + hairRadius}
        rx={height * HEAD_WIDTH_RATIO}
        ry={hairRadius}
        fill={SILHOUETTE_FILL}
      />
      <rect
        x={centerX - neck}
        y={bottom - height * 0.1}
        width={neck * 2}
        height={height * (NECK_BASE_RATIO + 0.1 + 0.04)}
        fill={SKIN_FILL}
      />
      <path d={bustPath(face, frame)} fill={SILHOUETTE_FILL} />
      <ellipse
        cx={centerX}
        cy={(faceTop + bottom) / 2}
        rx={height * FACE_WIDTH_RATIO}
        ry={(bottom - faceTop) / 2}
        fill={SKIN_FILL}
      />
      <path d={fringePath(face)} fill={SILHOUETTE_FILL} />
    </g>
  );
}
