// Runs with bun: the group marks the pressed item and reports another.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import { ToggleGroup, ToggleGroupItem } from "./toggle-group.tsx";

afterEach(cleanup);

it("marks the pressed item and reports another", () => {
  const onValueChange: Mock<(value: string[]) => void> = vi.fn();
  render(
    <ToggleGroup
      aria-label="Where"
      className="p-1"
      value={["top"]}
      onValueChange={(next: string[]): void => onValueChange(next)}
    >
      <ToggleGroupItem value="top" className="p-2">
        Top
      </ToggleGroupItem>
      <ToggleGroupItem value="bottom">Bottom</ToggleGroupItem>
    </ToggleGroup>,
  );
  expect(screen.getByRole("button", { name: "Top" }).getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "Bottom" }));
  expect(onValueChange.mock.calls).toStrictEqual([[["bottom"]]]);
});
