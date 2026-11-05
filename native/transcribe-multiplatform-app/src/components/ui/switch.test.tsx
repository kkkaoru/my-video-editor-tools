// Runs with bun: the switch reports the state it changes to.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import { Switch } from "./switch.tsx";

afterEach(cleanup);

it("reports turning off", () => {
  const onCheckedChange: Mock<(checked: boolean) => void> = vi.fn();
  render(
    <Switch
      aria-label="Shadow"
      className="p-1"
      checked
      onCheckedChange={(next: boolean): void => onCheckedChange(next)}
    />,
  );
  const toggle: HTMLElement = screen.getByRole("switch", { name: "Shadow" });
  expect(toggle.getAttribute("aria-checked")).toBe("true");
  fireEvent.click(toggle);
  expect(onCheckedChange.mock.calls).toStrictEqual([[false]]);
});
