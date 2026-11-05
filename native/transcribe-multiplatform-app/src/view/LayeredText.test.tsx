// Runs with bun: the preview paints the same layers, in the same order, as the subtitle files.
import { cleanup, render } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { captionLayers, fontStack, LayeredText, shadowFilter } from "./LayeredText.tsx";

afterEach(cleanup);

it("stacks the outer edge, the outline and a thickened fill, each reaching past the ones inside", () => {
  expect(
    captionLayers(
      {
        fill: "#005A9C",
        fill_width: 0.125,
        outline: "#FFFFFF",
        outline_width: 0.25,
        rim: "#112233",
        rim_width: 0.125,
        shadow: null,
        shadow_x: 0.03,
        shadow_y: -0.03,
        shadow_blur: 5,
      },
      100,
    ),
  ).toStrictEqual([
    { fill: "#112233", stroke: "#112233", strokeWidth: 100 },
    { fill: "#FFFFFF", stroke: "#FFFFFF", strokeWidth: 75 },
    { fill: "#005A9C", stroke: "#005A9C", strokeWidth: 25 },
  ]);
});

it("draws no outer edge when it is transparent", () => {
  expect(
    captionLayers(
      {
        fill: "#005A9C",
        fill_width: 0,
        outline: "#FFFFFF",
        outline_width: 0.1,
        rim: null,
        rim_width: 0.2,
        shadow: null,
        shadow_x: 0.03,
        shadow_y: -0.03,
        shadow_blur: 5,
      },
      100,
    ),
  ).toStrictEqual([
    { fill: "#FFFFFF", stroke: "#FFFFFF", strokeWidth: 20 },
    { fill: "#005A9C", stroke: "none", strokeWidth: 0 },
  ]);
});

it("draws no layer of no width", () => {
  expect(
    captionLayers(
      {
        fill: "#FFFFFF",
        fill_width: 0,
        outline: "#000000",
        outline_width: 0,
        rim: "#000000",
        rim_width: 0,
        shadow: null,
        shadow_x: 0.03,
        shadow_y: -0.03,
        shadow_blur: 5,
      },
      100,
    ),
  ).toStrictEqual([{ fill: "#FFFFFF", stroke: "none", strokeWidth: 0 }]);
});

it("has no shadow filter without a shadow", () => {
  expect(
    shadowFilter(
      {
        fill: "#FFFFFF",
        fill_width: 0,
        outline: "#000000",
        outline_width: 0.1,
        rim: null,
        rim_width: 0,
        shadow: null,
        shadow_x: 0.1,
        shadow_y: 0.1,
        shadow_blur: 5,
      },
      { fontPx: 40, frame: { width: 1000, height: 1000 } },
    ),
  ).toBe("none");
});

it("offsets the shadow in ems, down for a negative y, and blurs it by the short edge", () => {
  expect(
    shadowFilter(
      {
        fill: "#FFFFFF",
        fill_width: 0,
        outline: "#000000",
        outline_width: 0.1,
        rim: null,
        rim_width: 0,
        shadow: "#112233",
        shadow_x: 0.25,
        shadow_y: -0.5,
        shadow_blur: 10,
      },
      { fontPx: 40, frame: { width: 720, height: 1280 } },
    ),
  ).toBe("drop-shadow(20px 40px 5px #112233)");
});

it("centres the lines on the block, one line height apart", () => {
  const { container } = render(
    <svg aria-hidden="true">
      <LayeredText
        lines={["a", "b"]}
        x={10}
        y={100}
        fontFamily="Test Font"
        fontPx={20}
        lineHeightPx={30}
        color={{
          fill: "#FFFFFF",
          fill_width: 0,
          outline: "#000000",
          outline_width: 0.1,
          rim: null,
          rim_width: 0.034,
          shadow: null,
          shadow_x: 0.03,
          shadow_y: -0.03,
          shadow_blur: 5,
        }}
        strokeUnitPx={10}
        frame={{ width: 100, height: 100 }}
      />
    </svg>,
  );
  const baselines: (string | null)[] = Array.from(container.querySelectorAll("g g:last-child text")).map(
    (text: Element): string | null => text.getAttribute("y"),
  );
  expect(baselines).toStrictEqual(["92", "122"]);
});

it("puts the chosen family ahead of the substitutes", () => {
  expect(fontStack("Test Font")).toBe(
    '"Test Font", "Noto Sans JP", "Hiragino Sans", "Yu Gothic UI", sans-serif',
  );
});
