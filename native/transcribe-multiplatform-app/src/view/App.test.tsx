// Runs with bun: the window chrome, language, tabs, clock and failure banner, wired to the real store.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, type Mock, vi } from "vitest";
import type { Settings, Snapshot } from "../backend/types.ts";
import { enMessages } from "../i18n/locale-en.ts";
import { App, hardwareLabel } from "./App.tsx";

type DropPayload = { type: "drop"; paths: string[] } | { type: "enter" | "over" | "leave" };
type DropCallback = (event: { payload: DropPayload }) => void;

const onDragDropEvent: Mock<(callback: DropCallback) => Promise<() => void>> = vi.hoisted(() => vi.fn());
const SETTINGS: Settings = {
  palette: [{ fill: "#111111", outline: "#222222" }],
  unassigned: { fill: "#AAAAAA", outline: "#000000" },
  font_family: "Sans",
  font_ratio: 0.05,
  outline_ratio: 0.1,
  shadow: { enabled: false, strength: 0.5 },
  width_ratio: 0.8,
  placement: { anchor: "video_bottom", offset_x_percent: 0, offset_y_percent: 0 },
  language: "auto",
  diarize_model: "b",
  face_model: "here",
  model_dirs: [],
  rim_ratio: 0.034,
  line_height: 1.448,
  speaker_count: 0,
};
const SNAPSHOT: Snapshot = {
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
      hidden: false,
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
      hidden: false,
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
      hidden: false,
    },
  ],
  models_dir: "/models",
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

it("names the Apple GPU on a Mac", () => {
  expect(hardwareLabel(SNAPSHOT, enMessages)).toBe("Apple GPU (Metal) / memory 16 GB");
});

it("names the CPU elsewhere", () => {
  expect(
    hardwareLabel({ backend: "default", hardware: { is_mac: false, memory_gib: 8, gpu: null } }, enMessages),
  ).toBe("CPU only / memory 8 GB");
});

it("names a GPU with its memory and no runtime by default", () => {
  expect(
    hardwareLabel(
      {
        backend: "default",
        hardware: {
          is_mac: false,
          memory_gib: 8,
          gpu: { vendor: "nvidia", name: "RTX 4090", memory_gib: 24 },
        },
      },
      enMessages,
    ),
  ).toBe("RTX 4090 (24 GB) / memory 8 GB");
});

it("names the CUDA runtime", () => {
  expect(
    hardwareLabel(
      {
        backend: "cuda",
        hardware: {
          is_mac: false,
          memory_gib: 8,
          gpu: { vendor: "nvidia", name: "RTX 4090", memory_gib: 24 },
        },
      },
      enMessages,
    ),
  ).toBe("RTX 4090 (24 GB, CUDA) / memory 8 GB");
});

it("names the Vulkan runtime", () => {
  expect(
    hardwareLabel(
      {
        backend: "vulkan",
        hardware: {
          is_mac: false,
          memory_gib: 8,
          gpu: { vendor: "radeon", name: "RX 7900", memory_gib: 16 },
        },
      },
      enMessages,
    ),
  ).toBe("RX 7900 (16 GB, Vulkan) / memory 8 GB");
});

it("sets the document language and title before showing the app", () => {
  render(<App initial={{ ...SNAPSHOT, locale: "ja-JP" }} />);
  expect(document.documentElement.lang).toBe("ja");
  expect(document.title).toBe("Transcribe for Resolve");
  expect(screen.getByRole("tab", { selected: true }).textContent).toBe("文字起こし");
});

it("opens on the jobs tab with the hardware in the header", () => {
  render(<App initial={SNAPSHOT} />);
  expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Transcribe for Resolve");
  expect(screen.getByRole("tab", { selected: true }).textContent).toBe("Transcribe");
  expect(screen.getByText("Apple GPU (Metal) / memory 16 GB").tagName).toBe("P");
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

it("shows a background failure in the banner", async () => {
  render(<App initial={{ ...SNAPSHOT, models: [] }} />);
  vi.mocked(invoke).mockRejectedValue(new Error("refused"));
  act((): void => onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "drop", paths: ["/a.mp4"] } }));
  await waitFor((): void => {
    expect(screen.getByText("Something went wrong: Error: refused").getAttribute("role")).toBe("alert");
  });
});
