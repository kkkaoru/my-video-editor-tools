// Runs with bun: the colour square offers the usual colours, transparent where allowed, and any colour.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import { ColorPicker } from "./ColorPicker.tsx";

afterEach(cleanup);

async function openPicker(name: string): Promise<void> {
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name }));
  });
}

function show(value: string | null, allowTransparent: boolean): Mock<(next: string | null) => void> {
  const onChange: Mock<(next: string | null) => void> = vi.fn();
  render(
    <main>
      <ColorPicker
        label="Speaker 1: Fill"
        value={value}
        allowTransparent={allowTransparent}
        onChange={onChange}
      />
    </main>,
  );
  return onChange;
}

it("names a usual colour, a free colour and transparent on the square", () => {
  show("#FFFFFF", false);
  expect(screen.getByRole("button", { name: "Speaker 1: Fill: White" }).dataset.slot).toBe("color-swatch");
  cleanup();
  show("#123456", false);
  expect(screen.getByRole("button", { name: "Speaker 1: Fill: #123456" }).dataset.slot).toBe("color-swatch");
  cleanup();
  show(null, true);
  expect(screen.getByRole("button", { name: "Speaker 1: Fill: Transparent" }).dataset.slot).toBe(
    "color-swatch",
  );
});

it("paints the square itself, and shows a checkerboard for transparent", () => {
  show("#123456", false);
  const painted: HTMLElement = screen.getByRole("button", { name: "Speaker 1: Fill: #123456" });
  expect(painted.getAttribute("style")).toBe("background-color: #123456;");
  expect(painted.childElementCount).toBe(0);
  cleanup();
  show(null, true);
  expect(screen.getByRole("button", { name: "Speaker 1: Fill: Transparent" }).getAttribute("style")).toBe(
    null,
  );
});

it("chooses a usual colour and closes", async () => {
  const onChange = show("#FFFFFF", false);
  await openPicker("Speaker 1: Fill: White");
  expect(screen.getByRole("button", { name: "White" }).getAttribute("aria-pressed")).toBe("true");
  expect(screen.getByRole("button", { name: "Red" }).getAttribute("aria-pressed")).toBe("false");
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Red" }));
  });
  expect(onChange.mock.calls).toStrictEqual([["#D0021B"]]);
  expect(screen.getByRole("button", { name: "Speaker 1: Fill: White" }).getAttribute("aria-expanded")).toBe(
    "false",
  );
});

it("offers no transparent where it is not allowed", async () => {
  show("#FFFFFF", false);
  await openPicker("Speaker 1: Fill: White");
  expect(screen.queryByRole("button", { name: "Transparent" })).toBe(null);
});

it("chooses transparent", async () => {
  const onChange = show("#000000", true);
  await openPicker("Speaker 1: Fill: Black");
  expect(screen.getByRole("button", { name: "Transparent" }).getAttribute("aria-pressed")).toBe("false");
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Transparent" }));
  });
  expect(onChange.mock.calls).toStrictEqual([[null]]);
});

it("marks transparent as chosen, and a usual colour turns it off", async () => {
  const onChange = show(null, true);
  await openPicker("Speaker 1: Fill: Transparent");
  expect(screen.getByRole("button", { name: "Transparent" }).getAttribute("aria-pressed")).toBe("true");
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Black" }));
  });
  expect(onChange.mock.calls).toStrictEqual([["#000000"]]);
});

it("takes any colour in capitals and stays open for more", async () => {
  const onChange = show("#FFFFFF", false);
  await openPicker("Speaker 1: Fill: White");
  fireEvent.input(screen.getByLabelText("Any colour"), { target: { value: "#abcdef" } });
  expect(onChange.mock.calls).toStrictEqual([["#ABCDEF"]]);
  expect(screen.getByRole("button", { name: "Speaker 1: Fill: White" }).getAttribute("aria-expanded")).toBe(
    "true",
  );
});

it("starts any colour from black when nothing is chosen", async () => {
  show(null, true);
  await openPicker("Speaker 1: Fill: Transparent");
  expect(screen.getByLabelText<HTMLInputElement>("Any colour").value).toBe("#000000");
});

it("takes a typed colour once it is whole, in capitals", async () => {
  const onChange = show("#FFFFFF", false);
  await openPicker("Speaker 1: Fill: White");
  const typed = screen.getByRole<HTMLInputElement>("textbox", { name: "Any colour (#RRGGBB)" });
  expect(typed.value).toBe("#FFFFFF");
  fireEvent.input(typed, { target: { value: "#12" } });
  expect(onChange.mock.calls).toStrictEqual([]);
  expect(typed.value).toBe("#12");
  fireEvent.input(typed, { target: { value: "#12ab3c" } });
  expect(onChange.mock.calls).toStrictEqual([["#12AB3C"]]);
});

it("goes back to the colour when typing stops halfway", async () => {
  show("#FFFFFF", false);
  await openPicker("Speaker 1: Fill: White");
  const typed = screen.getByRole<HTMLInputElement>("textbox", { name: "Any colour (#RRGGBB)" });
  fireEvent.input(typed, { target: { value: "#12" } });
  fireEvent.focusOut(typed);
  expect(typed.value).toBe("#FFFFFF");
});

it("leaves the typed colour empty for transparent", async () => {
  show(null, true);
  await openPicker("Speaker 1: Fill: Transparent");
  expect(screen.getByRole<HTMLInputElement>("textbox", { name: "Any colour (#RRGGBB)" }).value).toBe("");
});
