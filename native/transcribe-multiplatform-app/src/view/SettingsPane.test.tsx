// Runs with bun: the settings show the version first and save the language and the appearance.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, expect, it, type Mock, vi } from "vitest";
import type { Settings } from "../backend/types.ts";
import { INITIAL_UPDATE } from "../state/update-state.ts";
import type { Updater } from "../state/use-updater.ts";
import { SettingsPane } from "./SettingsPane.tsx";

const SETTINGS = { ui_language: "", ui_theme: "system" } satisfies Partial<Settings>;

const UPDATER: Updater = { state: INITIAL_UPDATE, check: vi.fn(), install: vi.fn() };

afterEach(cleanup);

function show(settings: Partial<Settings>): Mock<(next: Settings) => void> {
  const onChange: Mock<(next: Settings) => void> = vi.fn();
  render(
    <main>
      <SettingsPane
        version="1.2.3"
        settings={{ ...SETTINGS, ...settings } as Settings}
        onChange={onChange}
        updater={UPDATER}
      />
    </main>,
  );
  return onChange;
}

async function choose(combobox: string, option: string): Promise<void> {
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: combobox }));
  });
  await act(async (): Promise<void> => {
    fireEvent.pointerDown(screen.getByRole("option", { name: option }));
    fireEvent.click(screen.getByRole("option", { name: option }));
  });
}

it("shows the version first", () => {
  const { container } = render(
    <main>
      <SettingsPane version="1.2.3" settings={SETTINGS as Settings} onChange={vi.fn()} updater={UPDATER} />
    </main>,
  );
  expect(container.querySelector("p")?.textContent).toBe("Version 1.2.3");
});

it("starts from the system for both", () => {
  show({});
  expect(screen.getByRole("combobox", { name: "Display language" }).textContent).toBe("Same as the system");
  expect(screen.getByRole("combobox", { name: "Appearance" }).textContent).toBe("Same as the system");
});

it("lists every language under its own name", async () => {
  show({});
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Display language" }));
  });
  expect(
    screen.getAllByRole("option").map(({ textContent }: HTMLElement): string | null => textContent),
  ).toStrictEqual([
    "Same as the system",
    "English",
    "日本語",
    "简体中文",
    "繁體中文",
    "한국어",
    "Deutsch",
    "Français",
    "Español",
  ]);
  expect(screen.getByRole("option", { name: "日本語" }).getAttribute("lang")).toBe("ja");
});

it("saves a chosen language", async () => {
  const onChange = show({});
  await choose("Display language", "Deutsch");
  expect(onChange.mock.lastCall?.[0].ui_language).toBe("de");
});

it("saves a chosen appearance", async () => {
  const onChange = show({});
  await choose("Appearance", "Dark");
  expect(onChange.mock.lastCall?.[0].ui_theme).toBe("dark");
});

it("goes back to the system's language by saving none", async () => {
  const onChange = show({ ui_language: "de" });
  expect(screen.getByRole("combobox", { name: "Display language" }).textContent).toBe("Deutsch");
  await choose("Display language", "Same as the system");
  expect(onChange.mock.lastCall?.[0].ui_language).toBe("");
});

it("offers the update check right under the version", () => {
  show({});
  expect(screen.getByText("Version 1.2.3").nextElementSibling?.querySelector("button")?.textContent).toBe(
    "Check for updates",
  );
});
