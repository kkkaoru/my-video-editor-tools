// Runs with bun: the preview paints the same layers, in the same order, as the subtitle files.
import { cleanup, render } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { captionLayers, LayeredText, shadowFilter } from "./LayeredText.tsx";

afterEach(cleanup);

it("puts a black rim under a coloured outline, then the band, then the bare fill", () => {
  expect(captionLayers({ fill: "#005A9C", outline: "#FFFFFF" }, 5, 2)).toStrictEqual([
    { fill: "#000000", stroke: "#000000", strokeWidth: 14 },
    { fill: "#FFFFFF", stroke: "#FFFFFF", strokeWidth: 10 },
    { fill: "#005A9C", stroke: "none", strokeWidth: 0 },
  ]);
});

it("needs no rim around a black outline", () => {
  expect(captionLayers({ fill: "#FFFFFF", outline: "#000000" }, 5, 2)).toStrictEqual([
    { fill: "#000000", stroke: "#000000", strokeWidth: 10 },
    { fill: "#FFFFFF", stroke: "none", strokeWidth: 0 },
  ]);
});

it("draws no rim when its width is zero", () => {
  expect(captionLayers({ fill: "#005A9C", outline: "#FFFFFF" }, 5, 0)).toStrictEqual([
    { fill: "#FFFFFF", stroke: "#FFFFFF", strokeWidth: 10 },
    { fill: "#005A9C", stroke: "none", strokeWidth: 0 },
  ]);
});

it("has no shadow when it is off", () => {
  expect(shadowFilter({ enabled: false, strength: 0.5 }, 88)).toBe("none");
});

it("casts the shadow straight down at its strength, scaled to the text", () => {
  expect(shadowFilter({ enabled: true, strength: 0.5 }, 88)).toBe("drop-shadow(0 16px 8px rgb(0 0 0 / 0.5))");
});

it("centres the lines on the block, one line height apart", () => {
  const { container } = render(
    <svg aria-hidden="true">
      <LayeredText
        lines={["a", "b"]}
        x={10}
        y={100}
        fontPx={20}
        lineHeightPx={30}
        color={{ fill: "#FFFFFF", outline: "#000000" }}
        outlinePx={2}
        rimPx={1}
        shadow={{ enabled: false, strength: 0 }}
      />
    </svg>,
  );
  const baselines: (string | null)[] = Array.from(container.querySelectorAll("g g:last-child text")).map(
    (text: Element): string | null => text.getAttribute("y"),
  );
  expect(baselines).toStrictEqual(["92", "122"]);
});
