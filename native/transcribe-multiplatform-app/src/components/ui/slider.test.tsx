// Runs with bun: the slider names its thumb and reports key presses.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import { Slider } from "./slider.tsx";

// The edge-aligned thumb stays visibility:hidden until measured and happy-dom measures nothing,
// which also empties its accessible name, so its input is found by the label attribute.
const SLIDER_INPUT: { selector: string } = { selector: "input[type=range]" };

afterEach(cleanup);

it("reports a key press on the named thumb", () => {
  const onValueChange: Mock<(value: number) => void> = vi.fn();
  render(
    <Slider
      thumbLabel="Size"
      className="w-40"
      value={1}
      min={0}
      max={2}
      onValueChange={(next: number): void => onValueChange(next)}
    />,
  );
  fireEvent.keyDown(screen.getByLabelText("Size", SLIDER_INPUT), { key: "ArrowRight" });
  expect(onValueChange.mock.calls).toStrictEqual([[2]]);
});
