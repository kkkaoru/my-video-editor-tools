// Runs with bun: the window chrome, language, tabs, clock and failure banner, wired to the real store.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, type Mock, vi } from "vitest";
import type { Settings, Snapshot } from "../backend/types.ts";
import { App } from "./App.tsx";

type DropPayload = { type: "drop"; paths: string[] } | { type: "enter" | "over" | "leave" };
type DropCallback = (event: { payload: DropPayload }) => void;

const onDragDropEvent: Mock<(callback: DropCallback) => Promise<() => void>> = vi.hoisted(() => vi.fn());
const SETTINGS: Settings = {
  palette: [
    {
      fill: "#111111",
      fill_width: 0,
      outline: "#222222",
      outline_width: 0.1,
      rim: null,
      rim_width: 0.034,
      shadow: null,
      shadow_x: 0.03,
      shadow_y: -0.03,
      shadow_blur: 5,
    },
  ],
  unassigned: {
    fill: "#AAAAAA",
    fill_width: 0,
    outline: "#000000",
    outline_width: 0.1,
    rim: null,
    rim_width: 0.034,
    shadow: null,
    shadow_x: 0.03,
    shadow_y: -0.03,
    shadow_blur: 5,
  },
  font_family: "Sans",
  font_ratio: 0.05,
  width_ratio: 0.8,
  safe_left_percent: 5,
  safe_right_percent: 5,
  placement: {
    anchor: "video_bottom",
    offset_x_percent: 0,
    offset_y_percent: 0,
  },
  language: "auto",
  diarize_model: "b",
  face_model: "here",
  model_dirs: [],
  active_profile: "",
  line_height: 1.448,
  speaker_count: 0,
  subtitle_format: "fcpxml",
  ui_language: "",
  ui_theme: "system",
};
const SNAPSHOT: Snapshot = {
  version: "1.2.3",
  locale: "en-US",
  hardware: { is_mac: true, memory_gib: 16, gpu: null },
  backend: "default",
  settings: SETTINGS,
  models: [
    {
      id: "here",
      role: "face",
      label: "Face here",
      source_url: "",
      download_bytes: 0,
      found_at: "/f",
      blocked: null,
      recommended: false,
    },
    {
      id: "away",
      role: "face",
      label: "Face away",
      source_url: "",
      download_bytes: 0,
      found_at: null,
      blocked: null,
      recommended: false,
    },
    {
      id: "speech",
      role: "transcribe",
      label: "Speech",
      source_url: "",
      download_bytes: 0,
      found_at: "/s",
      blocked: null,
      recommended: false,
    },
  ],
  models_dir: "/models",
  profiles: [],
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent }) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const emitJob = (payload: unknown): void =>
  vi
    .mocked(listen)
    .mock.calls.find(([event]): boolean => event === "job")?.[1]({ event: "job", id: 0, payload });

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(listen).mockResolvedValue(vi.fn());
  vi.mocked(invoke).mockResolvedValue(SNAPSHOT);
  onDragDropEvent.mockResolvedValue(vi.fn());
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

it("sets the document language and title before showing the app", () => {
  render(<App initial={{ ...SNAPSHOT, locale: "ja-JP" }} />);
  expect(document.documentElement.lang).toBe("ja");
  expect(document.title).toBe("Transcribe Captions");
  expect(screen.getByRole("tab", { selected: true }).textContent).toBe("字幕を作る");
});

it("opens on the jobs tab with the hardware in the header", () => {
  render(<App initial={SNAPSHOT} />);
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Transcribe Captions");
  expect(screen.getByRole("tab", { selected: true }).textContent).toBe("Transcribe");
  expect(screen.queryByText(/memory 16 GB/)).toBe(null);
  expect(screen.getByRole("button", { name: "Choose videos" }).tagName).toBe("BUTTON");
});

it("offers only present face models as choices on the style tab", async () => {
  render(<App initial={SNAPSHOT} />);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("tab", { name: "Caption look and position" }));
  });
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Face detection model" }));
  });
  expect(screen.getByRole("option", { name: "Face here" }).getAttribute("aria-disabled")).toBe(null);
  expect(screen.getByRole("option", { name: "Face away" }).getAttribute("aria-disabled")).toBe("true");
  expect(screen.queryByRole("option", { name: "Speech" })).toBe(null);
});

it("shows the models on the models tab", async () => {
  render(<App initial={SNAPSHOT} />);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("tab", { name: "Models" }));
  });
  expect(screen.getByRole("heading", { name: "Face here" }).tagName).toBe("H3");
});

it("shows job progress from the backend", () => {
  render(<App initial={SNAPSHOT} />);
  act((): void => emitJob({ state: "queued", id: 1, path: "/talk.mp4" }));
  expect(screen.getByRole("heading", { name: "talk.mp4" }).tagName).toBe("H2");
  expect(screen.getByText("Waiting in line").getAttribute("role")).toBe("status");
});

it("refuses a dropped video without its models, saying what to do", () => {
  render(<App initial={{ ...SNAPSHOT, models: [] }} />);
  act((): void => onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "drop", paths: ["/a.mp4"] } }));
  expect(
    screen
      .getByText(
        "Something went wrong: The videos were not added because no model is available for: transcription. Download them in Models, or add a folder that already has them.",
      )
      .getAttribute("role"),
  ).toBe("alert");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
});

it("shows a background failure in the banner", async () => {
  render(<App initial={{ ...SNAPSHOT, settings: { ...SETTINGS, speaker_count: 1 } }} />);
  vi.mocked(invoke).mockRejectedValue(new Error("refused"));
  act((): void => onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "drop", paths: ["/a.mp4"] } }));
  await waitFor((): void => {
    expect(screen.getByText("Something went wrong: Error: refused").getAttribute("role")).toBe("alert");
  });
});

it("shows the version only inside the open settings tab", async () => {
  render(<App initial={SNAPSHOT} />);
  const version: HTMLElement = screen.getByText("Version 1.2.3");
  expect(version.closest("[role=tabpanel]")?.hasAttribute("hidden")).toBe(true);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
  });
  expect(screen.getByRole("tab", { selected: true }).textContent).toBe("Settings");
  expect(version.closest("[role=tabpanel]")?.hasAttribute("hidden")).toBe(false);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("tab", { name: "Models" }));
  });
  await waitFor((): void => {
    expect(version.closest("[role=tabpanel]")?.hasAttribute("hidden")).toBe(true);
  });
});

it("puts the tabs in order, with the settings last", () => {
  render(<App initial={SNAPSHOT} />);
  expect(screen.getAllByRole("tab").map((tab: HTMLElement): string | null => tab.textContent)).toStrictEqual([
    "Transcribe",
    "Caption look and position",
    "Models",
    "How to use",
    "Settings",
  ]);
});

it("follows the system's language until the settings choose one, at once", () => {
  render(<App initial={{ ...SNAPSHOT, locale: "ja-JP" }} />);
  expect(document.documentElement.lang).toBe("ja");
  cleanup();
  render(<App initial={{ ...SNAPSHOT, locale: "ja-JP", settings: { ...SETTINGS, ui_language: "de" } }} />);
  expect(document.documentElement.lang).toBe("de");
  expect(screen.getByRole("tab", { name: "Einstellungen" }).getAttribute("role")).toBe("tab");
});

it("marks the page with a chosen theme, and with none for the system's", () => {
  render(<App initial={{ ...SNAPSHOT, settings: { ...SETTINGS, ui_theme: "dark" } }} />);
  expect(document.documentElement.dataset.theme).toBe("dark");
  cleanup();
  render(<App initial={SNAPSHOT} />);
  expect(document.documentElement.dataset.theme).toBe(undefined);
});

async function chooseLanguage(combobox: string, option: string): Promise<void> {
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: combobox }));
  });
  await act(async (): Promise<void> => {
    fireEvent.pointerDown(screen.getByRole("option", { name: option }));
    fireEvent.click(screen.getByRole("option", { name: option }));
  });
}

it("shows the system's language, with that chosen, when no language is saved", () => {
  render(<App initial={{ ...SNAPSHOT, locale: "ja-JP", settings: { ...SETTINGS, ui_language: "" } }} />);
  expect(document.documentElement.lang).toBe("ja");
  expect(screen.getByRole("tab", { name: "設定" }).getAttribute("role")).toBe("tab");
  expect(screen.getByRole("combobox", { name: "画面の言語", hidden: true }).textContent).toBe(
    "システムに合わせる",
  );
});

it("changes language at once, and back to the system's", async () => {
  // As the backend does: the snapshot's locale is the chosen language, or the system's.
  vi.mocked(invoke).mockImplementation(
    (_command: string, args?: unknown): Promise<unknown> =>
      Promise.resolve({
        ...SNAPSHOT,
        locale:
          args !== undefined && Reflect.get(Reflect.get(Object(args), "settings"), "ui_language") === "de"
            ? "de"
            : "ja-JP",
      }),
  );
  render(<App initial={{ ...SNAPSHOT, locale: "ja-JP" }} />);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("tab", { name: "設定" }));
  });
  await chooseLanguage("画面の言語", "Deutsch");
  expect(document.documentElement.lang).toBe("de");
  await waitFor((): void => {
    expect(vi.mocked(invoke).mock.lastCall?.[0]).toBe("save_settings");
  });
  await chooseLanguage("Sprache der Oberfläche", "Wie das System");
  await waitFor((): void => {
    expect(document.documentElement.lang).toBe("ja");
  });
});
