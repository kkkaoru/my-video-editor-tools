// Runs with bun: progress exposes its value, or none while indeterminate.
import { cleanup, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Progress } from "./progress.tsx";

afterEach(cleanup);

it("shows a value", () => {
  render(<Progress value={0.5} min={0} max={1} aria-label="Work" />);
  expect(screen.getByRole("progressbar", { name: "Work" }).getAttribute("aria-valuenow")).toBe("0.5");
});

it("shows no value while indeterminate", () => {
  render(<Progress value={null} aria-label="Work" className="mt-2" />);
  const bar: HTMLElement = screen.getByRole("progressbar", { name: "Work" });
  expect(bar.getAttribute("aria-valuenow")).toBe(null);
  expect(bar.className).toBe("w-full mt-2");
});
