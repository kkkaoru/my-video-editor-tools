// Runs with bun: an input keeps its slot and merges a caller's classes.
import { cleanup, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Input } from "./input.tsx";

afterEach(cleanup);

it("merges a caller's classes", () => {
  render(<Input aria-label="Name" className="w-24" />);
  const input: HTMLElement = screen.getByRole("textbox", { name: "Name" });
  expect(input.getAttribute("data-slot")).toBe("input");
  expect(input.className).toBe(
    "min-h-11 min-w-0 rounded-lg border border-border bg-background px-3 py-1 text-sm text-foreground transition-colors w-24",
  );
});
