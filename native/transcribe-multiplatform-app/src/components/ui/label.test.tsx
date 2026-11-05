// Runs with bun: a label names the control it points at.
import { cleanup, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Label } from "./label.tsx";

afterEach(cleanup);

it("names the control and merges a caller's classes", () => {
  render(
    <>
      <Label htmlFor="name" className="p-1">
        Name
      </Label>
      <input id="name" />
    </>,
  );
  expect(screen.getByRole("textbox", { name: "Name" }).id).toBe("name");
  expect(screen.getByText("Name").className).toBe("block text-sm font-medium p-1");
});
