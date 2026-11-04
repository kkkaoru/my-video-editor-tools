// Runs with bun: the installed families load on first open, filter as you type, and report the choice.
import { invoke } from "@tauri-apps/api/core";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, type Mock, vi } from "vitest";
import { FontField, familyChoices, matchesFamily } from "./FontField.tsx";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(invoke).mockResolvedValue(["Arial", "Hiragino Sans", "Noto Sans JP"]);
});

afterEach(cleanup);

async function openList(
  value: string,
  onChange: (family: string) => void,
  onFailure: (error: unknown) => void,
): Promise<void> {
  render(
    <main>
      <FontField value={value} onChange={onChange} onFailure={onFailure} />
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Font family" }));
  });
}

it("matches anywhere in the name, in any case", () => {
  expect(matchesFamily("Noto Sans JP", "sans")).toBe(true);
  expect(matchesFamily("Noto Sans JP", "SERIF")).toBe(false);
});

it("keeps a saved family that is not installed", () => {
  expect(familyChoices(["A"], "B")).toStrictEqual(["B", "A"]);
  expect(familyChoices(["A", "B"], "B")).toStrictEqual(["A", "B"]);
  expect(familyChoices(null, "B")).toStrictEqual(["B"]);
});

it("does not read the fonts until the list opens", () => {
  render(
    <main>
      <FontField value="Arial" onChange={vi.fn()} onFailure={vi.fn()} />
    </main>,
  );
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
});

it("lists the installed families once opened, and reads them only once", async () => {
  await openList("Arial", vi.fn(), vi.fn());
  expect(
    screen.getAllByRole("option").map((option: HTMLElement): string | null => option.textContent),
  ).toStrictEqual(["Arial", "Hiragino Sans", "Noto Sans JP"]);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("option", { name: "Arial" }));
  });
  await waitFor((): void => {
    expect(screen.queryByRole("listbox")).toBe(null);
  });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Font family" }));
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["font_families"]]);
});

it("reports the chosen family", async () => {
  const onChange: Mock<(family: string) => void> = vi.fn();
  await openList("Arial", onChange, vi.fn());
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("option", { name: "Noto Sans JP" }));
  });
  expect(onChange.mock.calls[0]?.[0]).toBe("Noto Sans JP");
});

it("narrows the list as you type", async () => {
  await openList("Arial", vi.fn(), vi.fn());
  await act(async (): Promise<void> => {
    fireEvent.input(screen.getByRole("combobox", { name: "Search the fonts" }), {
      target: { value: "hira" },
    });
  });
  expect(
    screen.getAllByRole("option").map((option: HTMLElement): string | null => option.textContent),
  ).toStrictEqual(["Hiragino Sans"]);
});

it("says when nothing matches", async () => {
  await openList("Arial", vi.fn(), vi.fn());
  await act(async (): Promise<void> => {
    fireEvent.input(screen.getByRole("combobox", { name: "Search the fonts" }), { target: { value: "zzz" } });
  });
  expect(screen.getByText("No installed font matches").tagName).toBe("DIV");
});

it("never puts more than a hundred rows in the DOM", async () => {
  vi.mocked(invoke).mockResolvedValue(
    Array.from({ length: 500 }, (_, index: number): string => `Family ${String(index).padStart(3, "0")}`),
  );
  await openList("Family 000", vi.fn(), vi.fn());
  expect(screen.getAllByRole("option").length).toBe(100);
});

it("says while the fonts are still being read", async () => {
  vi.mocked(invoke).mockReturnValue(new Promise((): void => undefined));
  await openList("Arial", vi.fn(), vi.fn());
  // The library adds a live region's first text just after mounting, so that it is announced.
  expect((await screen.findByText("Reading the installed fonts")).getAttribute("role")).toBe("status");
});

it("warns only once it knows the family is missing", async () => {
  await openList("Missing", vi.fn(), vi.fn());
  expect(
    screen.getByText(
      "Missing is not installed on this computer. The preview uses a substitute, and the subtitle files still name it.",
    ).tagName,
  ).toBe("P");
});

it("reports a failed read", async () => {
  const failure: Error = new Error("no fonts");
  vi.mocked(invoke).mockRejectedValue(failure);
  const onFailure: Mock<(error: unknown) => void> = vi.fn();
  await openList("Arial", vi.fn(), onFailure);
  expect(onFailure.mock.calls[0]?.[0]).toBeInstanceOf(Error);
});
