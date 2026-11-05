// Runs with bun: labelled fields report what the user changed, and keep the saved value otherwise.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import { AdjustField, SelectField } from "./fields.tsx";

// The edge-aligned thumb stays visibility:hidden until measured and happy-dom measures nothing,
// which also empties its accessible name, so its input is found by the label attribute.
const SLIDER_INPUT: { selector: string } = { selector: "input[type=range]" };
afterEach(cleanup);

it("labels both the slider and the typed number", () => {
  render(<AdjustField label="Size" value={5} min={0} max={10} step={1} onChange={vi.fn()} />);
  expect(screen.getByLabelText("Size", SLIDER_INPUT).getAttribute("aria-valuenow")).toBe("5");
  const typed: HTMLElement = screen.getByRole("spinbutton", { name: "Size" });
  expect(typed instanceof HTMLInputElement ? typed.value : null).toBe("5");
});

it("reports the slider moved with the keyboard", () => {
  const onChange: Mock<(value: number) => void> = vi.fn();
  render(<AdjustField label="Size" value={5} min={0} max={10} step={1} onChange={onChange} />);
  fireEvent.keyDown(screen.getByLabelText("Size", SLIDER_INPUT), { key: "ArrowRight" });
  expect(onChange.mock.calls).toStrictEqual([[6]]);
});

it("reports a typed number", () => {
  const onChange: Mock<(value: number) => void> = vi.fn();
  render(<AdjustField label="Size" value={5} min={0} max={10} step={1} onChange={onChange} />);
  fireEvent.input(screen.getByRole("spinbutton", { name: "Size" }), { target: { value: "7" } });
  expect(onChange.mock.calls).toStrictEqual([[7]]);
});

it("keeps the saved number when the field is cleared", () => {
  const onChange: Mock<(value: number) => void> = vi.fn();
  render(<AdjustField label="Size" value={5} min={0} max={10} step={1} onChange={onChange} />);
  fireEvent.input(screen.getByRole("spinbutton", { name: "Size" }), { target: { value: "" } });
  expect(onChange.mock.calls).toStrictEqual([]);
});

it("keeps the saved number when the typed one is out of range", () => {
  const onChange: Mock<(value: number) => void> = vi.fn();
  render(<AdjustField label="Size" value={5} min={0} max={10} step={1} onChange={onChange} />);
  fireEvent.input(screen.getByRole("spinbutton", { name: "Size" }), { target: { value: "99" } });
  expect(onChange.mock.calls).toStrictEqual([]);
});

it("shows the chosen option and reports another", async () => {
  const onChange: Mock<(value: string) => void> = vi.fn();
  render(
    <main>
      <SelectField
        label="Language"
        value="ja"
        options={[
          { value: "ja", label: "Japanese", disabled: false },
          { value: "en", label: "English", disabled: false },
          { value: "ko", label: "Korean", disabled: true },
        ]}
        onChange={onChange}
      />
    </main>,
  );
  const trigger: HTMLElement = screen.getByRole("combobox", { name: "Language" });
  expect(trigger.textContent).toBe("Japanese");
  await act(async (): Promise<void> => {
    fireEvent.click(trigger);
  });
  expect(
    screen.getAllByRole("option").map((option: HTMLElement): string | null => option.textContent),
  ).toStrictEqual(["Japanese", "English", "Korean"]);
  expect(screen.getByRole("option", { name: "Korean" }).getAttribute("aria-disabled")).toBe("true");
  expect(screen.getByRole("listbox").closest("main")?.tagName).toBe("MAIN");
  await act(async (): Promise<void> => {
    fireEvent.pointerDown(screen.getByRole("option", { name: "English" }));
    fireEvent.click(screen.getByRole("option", { name: "English" }));
  });
  expect(onChange.mock.calls).toStrictEqual([["en"]]);
});
