// Runs with bun: the parts work together: type, filter, choose, inside the main landmark.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import type { VNode } from "preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxStatus,
  ComboboxTrigger,
  ComboboxValue,
} from "./combobox.tsx";

afterEach(cleanup);

function show(onValueChange: (value: string | null) => void): void {
  render(
    <main>
      <Combobox items={["Apple", "Pear"]} value="Apple" onValueChange={onValueChange}>
        <ComboboxTrigger aria-label="Fruit" className="w-40">
          <ComboboxValue />
        </ComboboxTrigger>
        <ComboboxContent className="p-3">
          <ComboboxInput aria-label="Search" className="w-40" />
          <ComboboxStatus className="p-1">Ready</ComboboxStatus>
          <ComboboxEmpty className="p-2">None</ComboboxEmpty>
          <ComboboxList className="max-h-40">
            {(fruit: string): VNode => (
              <ComboboxItem key={fruit} value={fruit} className="pl-4">
                {fruit}
              </ComboboxItem>
            )}
          </ComboboxList>
        </ComboboxContent>
      </Combobox>
    </main>,
  );
}

it("opens inside main, marks the chosen item and reports another", async () => {
  const onValueChange: Mock<(value: string | null) => void> = vi.fn();
  show(onValueChange);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Fruit" }));
  });
  expect(screen.getByRole("listbox").closest("main")?.tagName).toBe("MAIN");
  expect(screen.getByRole("option", { name: "Apple" }).getAttribute("aria-selected")).toBe("true");
  // The library adds a live region's first text just after mounting, so that it is announced.
  expect((await screen.findByText("Ready")).getAttribute("role")).toBe("status");
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("option", { name: "Pear" }));
  });
  expect(onValueChange.mock.calls[0]?.[0]).toBe("Pear");
});

it("shows the empty text when nothing matches", async () => {
  show(vi.fn());
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Fruit" }));
  });
  await act(async (): Promise<void> => {
    fireEvent.input(screen.getByRole("combobox", { name: "Search" }), { target: { value: "zzz" } });
  });
  expect(screen.getByText("None").tagName).toBe("DIV");
});
