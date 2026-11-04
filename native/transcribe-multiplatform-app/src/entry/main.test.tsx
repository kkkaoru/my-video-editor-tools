// Runs with bun: the entry point mounts the app or says why it could not.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, cleanup, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Snapshot } from "../backend/types.ts";
import { mount, reportStartupFailure, start } from "./main.tsx";

const SNAPSHOT: Snapshot = {
  version: "1.2.3",
  locale: "en",
  hardware: { is_mac: true, memory_gib: 16, gpu: null },
  backend: "default",
  settings: {
    palette: [],
    unassigned: { fill: "#AAAAAA", outline: "#000000" },
    font_family: "Sans",
    font_ratio: 0.05,
    outline_ratio: 0.1,
    shadow: { enabled: false, strength: 0.5 },
    width_ratio: 0.8,
    placement: { anchor: "video_top", offset_x_percent: 0, offset_y_percent: 0 },
    language: "auto",
    diarize_model: "b",
    face_model: "c",
    model_dirs: [],
    active_profile: "",
    rim_ratio: 0.034,
    line_height: 1.448,
    speaker_count: 0,
  },
  models: [],
  models_dir: "/models",
  profiles: [],
};

// main.tsx starts itself on import, so its host and backend must be ready before the imports above run.
vi.hoisted((): void => {
  document.body.innerHTML = '<div id="app"></div>';
});

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.reject(new Error("at import"))) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: () => Promise.resolve(() => undefined) }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const newHost = (): HTMLElement => document.body.appendChild(document.createElement("div"));

beforeEach(() => {
  vi.mocked(listen).mockResolvedValue(vi.fn());
});

afterEach(cleanup);

it("reports a failure of its own start on import", async () => {
  await waitFor((): void => {
    expect(document.querySelector("#app [role=alert]")?.textContent).toBe("Startup failed: Error: at import");
  });
});

it("mounts the app into the host", async () => {
  const host: HTMLElement = newHost();
  await act((): void => mount(host, SNAPSHOT));
  expect(host.querySelector("h1")?.textContent).toBe("Transcribe for Resolve");
});

it("replaces the host's content with a startup failure", () => {
  const host: HTMLElement = newHost();
  host.textContent = "Loading";
  reportStartupFailure(host, new Error("boom"));
  expect(host.textContent).toBe("Startup failed: Error: boom");
  expect(host.firstElementChild?.getAttribute("role")).toBe("alert");
});

it("refuses to start without a host", async () => {
  await expect(start(null)).rejects.toThrow("the #app element is missing");
});

it("starts the app from the first snapshot", async () => {
  vi.mocked(invoke).mockResolvedValueOnce(SNAPSHOT);
  const host: HTMLElement = newHost();
  await act((): Promise<void> => start(host));
  expect(host.querySelector("h1")?.textContent).toBe("Transcribe for Resolve");
});

it("shows why the first snapshot failed", async () => {
  vi.mocked(invoke).mockRejectedValueOnce(new Error("no backend"));
  const host: HTMLElement = newHost();
  await start(host);
  expect(host.textContent).toBe("Startup failed: Error: no backend");
});
