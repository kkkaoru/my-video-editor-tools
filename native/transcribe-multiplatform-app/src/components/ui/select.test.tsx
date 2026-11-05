// Runs with bun: the popup opens inside <main> and marks the chosen item.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./select.tsx";

afterEach(cleanup);

it("opens inside the main landmark and marks the chosen item", async () => {
  render(
    <main>
      <Select value="a" items={[{ value: "a", label: "Apple" }]}>
        <SelectTrigger aria-label="Fruit" className="w-fit">
          <SelectValue className="flex-none" />
        </SelectTrigger>
        <SelectContent className="p-3">
          <SelectItem value="a" className="pl-4">
            Apple
          </SelectItem>
        </SelectContent>
      </Select>
    </main>,
  );
  const trigger: HTMLElement = screen.getByRole("combobox", { name: "Fruit" });
  expect(trigger.textContent).toBe("Apple");
  await act(async (): Promise<void> => {
    fireEvent.click(trigger);
  });
  expect(screen.getByRole("listbox").closest("main")?.tagName).toBe("MAIN");
  expect(screen.getByRole("option", { name: "Apple" }).getAttribute("aria-selected")).toBe("true");
});
