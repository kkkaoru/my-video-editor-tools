// Runs with bun: shared controls speak to assistive tech and report what the user changed.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import { ActionButton, AdjustField, buttonClass, ProgressBar, SelectField } from "./controls.tsx";

afterEach(cleanup);

it("gives a selected button the accent look", () => {
  expect(buttonClass(true)).toBe(
    "min-h-11 min-w-11 rounded-md border border-accent bg-accent px-3 py-2 text-left text-accent-ink",
  );
});

it("gives an unselected button the plain look", () => {
  expect(buttonClass(false)).toBe(
    "min-h-11 min-w-11 rounded-md border border-line bg-panel px-3 py-2 text-left text-ink hover:bg-canvas",
  );
});

it("runs the action when clicked", () => {
  const onClick: Mock<() => void> = vi.fn();
  render(<ActionButton onClick={onClick}>Go</ActionButton>);
  fireEvent.click(screen.getByRole("button", { name: "Go" }));
  expect(onClick).toHaveBeenCalledTimes(1);
});

it("leaves a plain button without a pressed state", () => {
  render(<ActionButton onClick={vi.fn()}>Go</ActionButton>);
  expect(screen.getByRole("button").getAttribute("aria-pressed")).toBe(null);
});

it("shows a primary button with the accent look", () => {
  render(
    <ActionButton primary onClick={vi.fn()}>
      Go
    </ActionButton>,
  );
  expect(screen.getByRole("button").className).toBe(
    "min-h-11 min-w-11 rounded-md border border-accent bg-accent px-3 py-2 text-left text-accent-ink",
  );
});

it("announces a pressed button", () => {
  render(
    <ActionButton pressed onClick={vi.fn()}>
      Go
    </ActionButton>,
  );
  expect(screen.getByRole("button").getAttribute("aria-pressed")).toBe("true");
});

it("reports the slider moved with the keyboard", () => {
  const onChange: Mock<(value: number) => void> = vi.fn();
  render(<AdjustField label="Size" value={5} min={0} max={10} step={1} onChange={onChange} />);
  fireEvent.keyDown(screen.getByRole("slider", { name: "Size" }), { key: "ArrowRight" });
  expect(onChange.mock.calls).toStrictEqual([[6]]);
});

it("reports a typed number", () => {
  const onChange: Mock<(value: number) => void> = vi.fn();
  render(<AdjustField label="Size" value={5} min={0} max={10} step={1} onChange={onChange} />);
  fireEvent.input(screen.getByRole("textbox", { name: "Size" }), { target: { value: "7" } });
  expect(onChange.mock.calls).toStrictEqual([[7]]);
});

it("keeps the saved number when the field is cleared", () => {
  const onChange: Mock<(value: number) => void> = vi.fn();
  render(<AdjustField label="Size" value={5} min={0} max={10} step={1} onChange={onChange} />);
  fireEvent.input(screen.getByRole("textbox", { name: "Size" }), { target: { value: "" } });
  expect(onChange.mock.calls).toStrictEqual([]);
});

it("shows the chosen option and reports another", async () => {
  const onChange: Mock<(value: string) => void> = vi.fn();
  render(
    <SelectField
      label="Language"
      value="ja"
      options={[
        { value: "ja", label: "Japanese", disabled: false },
        { value: "en", label: "English", disabled: false },
        { value: "ko", label: "Korean", disabled: true },
      ]}
      onChange={onChange}
    />,
  );
  const trigger: HTMLElement = screen.getByRole("combobox");
  expect(trigger.textContent).toBe("Japanese▾");
  await act(async (): Promise<void> => {
    fireEvent.click(trigger);
  });
  expect(
    screen.getAllByRole("option").map((option: HTMLElement): string | null => option.textContent),
  ).toStrictEqual(["✓Japanese", "English", "Korean"]);
  expect(screen.getByRole("option", { name: "Korean" }).getAttribute("aria-disabled")).toBe("true");
  await act(async (): Promise<void> => {
    fireEvent.pointerDown(screen.getByRole("option", { name: "English" }));
    fireEvent.click(screen.getByRole("option", { name: "English" }));
  });
  expect(onChange.mock.calls).toStrictEqual([["en"]]);
});

it("shows a determinate progress value", () => {
  render(<ProgressBar label="Download" fraction={0.5} valueText="half" />);
  const bar: HTMLElement = screen.getByRole("progressbar", { name: "Download" });
  expect(bar.getAttribute("aria-valuenow")).toBe("0.5");
  expect(bar.getAttribute("aria-valuetext")).toBe("half");
});

it("shows an indeterminate progress bar", () => {
  render(<ProgressBar label="Download" fraction={null} valueText="working" />);
  const bar: HTMLElement = screen.getByRole("progressbar", { name: "Download" });
  expect(bar.getAttribute("aria-valuenow")).toBe(null);
  expect(bar.getAttribute("aria-valuetext")).toBe("working");
});
