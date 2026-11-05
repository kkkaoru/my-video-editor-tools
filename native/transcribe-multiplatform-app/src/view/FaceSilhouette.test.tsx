// Runs with bun: the bust stands where captions keep away from, and reads on both halves of the preview.
import { cleanup, render } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { sampleFace } from "../layout/preview-layout.ts";
import { contrastRatio } from "../styling/contrast.ts";
import { bustPath, FaceSilhouette, fringePath, SILHOUETTE_FILL, SKIN_FILL } from "./FaceSilhouette.tsx";

const FACE = { centerX: 100, top: 44, bottom: 100, headTop: 0 };

afterEach(cleanup);

it("reads at 3:1 or better on the bright and the dark half of the preview", () => {
  expect(contrastRatio(SILHOUETTE_FILL, "#a9d8ef")).toBeGreaterThanOrEqual(3);
  expect(contrastRatio(SILHOUETTE_FILL, "#1f2530")).toBeGreaterThanOrEqual(3);
});

it("shows the face apart from the hair", () => {
  expect(contrastRatio(SKIN_FILL, SILHOUETTE_FILL)).toBeGreaterThanOrEqual(3);
});

it("keeps the head inside the area captions avoid: hair from its top, chin at its bottom", () => {
  const { container } = render(
    <svg>
      <title>figure</title>
      <FaceSilhouette face={FACE} frame={{ width: 200, height: 300 }} />
    </svg>,
  );
  const [hair, skin] = Array.from(container.querySelectorAll("ellipse"));
  expect(Number(hair?.getAttribute("cy")) - Number(hair?.getAttribute("ry"))).toBe(0);
  expect(Number(skin?.getAttribute("cy")) + Number(skin?.getAttribute("ry"))).toBe(100);
});

it("gives a face wider than a sliver and taller than it is wide, as a person's is", () => {
  const { container } = render(
    <svg>
      <title>figure</title>
      <FaceSilhouette face={FACE} frame={{ width: 200, height: 300 }} />
    </svg>,
  );
  const [, skin] = Array.from(container.querySelectorAll("ellipse"));
  const width: number = Number(skin?.getAttribute("rx")) * 2;
  const height: number = Number(skin?.getAttribute("ry")) * 2;
  expect(width / height).toBeGreaterThan(0.7);
  expect(width / height).toBeLessThan(0.95);
});

it("draws the hair over the crown, parted to one side", () => {
  expect(fringePath(FACE)).toBe(
    "M 64 56 C 64 -10, 136 -10, 136 50 Q 128.8 30.000000000000004, 112.6 28.000000000000004 Q 89.2 46, 64 56 Z",
  );
});

it("starts the bust under the chin, wider than the head, and runs down to the chest", () => {
  expect(bustPath(FACE, { width: 200, height: 400 })).toBe(
    "M 90 114 C 80 126, 28 128, 28 150 L 28 210 L 172 210 L 172 150 C 172 128, 120 126, 110 114 Z",
  );
});

it("cuts the chest off at the bottom of the frame", () => {
  expect(bustPath(FACE, { width: 200, height: 180 })).toBe(
    "M 90 114 C 80 126, 28 128, 28 150 L 28 180 L 172 180 L 172 150 C 172 128, 120 126, 110 114 Z",
  );
});

it("follows the sample face at any frame size", () => {
  const face = sampleFace({ width: 1080, height: 1920 });
  expect(face.bottom - face.top).toBe(216);
  expect(bustPath(face, { width: 1080, height: 1920 }).startsWith("M 491.4 ")).toBe(true);
});
