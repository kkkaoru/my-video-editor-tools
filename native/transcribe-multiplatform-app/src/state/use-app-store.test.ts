// Runs with bun: the store's subscriptions, debounced saving and failure reporting.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, cleanup, renderHook, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, type Mock, vi } from "vitest";
import type { Settings, Snapshot } from "../backend/types.ts";
import { type AppStore, onlyVideos, reportFailure, useAppStore, VIDEO_EXTENSIONS } from "./use-app-store.ts";

type DropPayload = { type: "drop"; paths: string[] } | { type: "enter" | "over" | "leave" };
type DropCallback = (event: { payload: DropPayload }) => void;

interface HookView {
  result: { current: AppStore };
  unmount: () => void;
}

const onDragDropEvent: Mock<(callback: DropCallback) => Promise<() => void>> = vi.hoisted(() => vi.fn());
const SETTINGS: Settings = {
  palette: [],
  unassigned: { fill: "#AAAAAA", outline: "#000000" },
  font_family: "Sans",
  font_ratio: 0.05,
  outline_ratio: 0.1,
  width_ratio: 0.8,
  placement: { anchor: "video_top", offset_x_percent: 0, offset_y_percent: 0 },
  language: "auto",
  transcribe_model: "a",
  diarize_model: "b",
  face_model: "c",
  model_dirs: [],
};
const SNAPSHOT: Snapshot = {
  locale: "en",
  hardware: { is_mac: true, memory_gib: 16, nvidia_name: null, nvidia_memory_gib: 0 },
  settings: SETTINGS,
  models: [],
  models_dir: "/models",
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent }) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const emitBackend = (name: string, payload: unknown): void =>
  vi
    .mocked(listen)
    .mock.calls.find(([event]): boolean => event === name)?.[1]({ event: name, id: 0, payload });

const mounted = (): HookView => renderHook((): AppStore => useAppStore(SNAPSHOT));

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

it("lists the extensions the file picker offers", () => {
  expect(VIDEO_EXTENSIONS).toStrictEqual(["mp4", "mov", "mkv", "m4v", "webm", "avi", "mts", "mxf", "ts"]);
});

it("keeps only video paths, in any case", () => {
  expect(onlyVideos(["/a.MP4", "/b.txt", "/c.mkv", "/d"])).toStrictEqual(["/a.MP4", "/c.mkv"]);
});

it("turns a failure into a load error", () => {
  const setState: Mock<AppStore["setState"]> = vi.fn();
  reportFailure(setState, new Error("boom"));
  expect(setState.mock.calls[0]?.[0](mounted().result.current.state).loadError).toStrictEqual({
    code: "internal",
    detail: "Error: boom",
  });
});

it("starts from the snapshot and subscribes once", () => {
  const view: HookView = mounted();
  expect(view.result.current.state.snapshot.locale).toBe("en");
  expect(vi.mocked(listen).mock.calls.map(([event]): unknown => event)).toStrictEqual(["job", "download"]);
  expect(onDragDropEvent).toHaveBeenCalledTimes(1);
});

it("applies job events from the backend", () => {
  const view: HookView = mounted();
  act((): void => emitBackend("job", { state: "queued", id: 1, path: "/a.mp4" }));
  expect(view.result.current.state.jobs.map(({ path }): string => path)).toStrictEqual(["/a.mp4"]);
});

it("applies a download event and refreshes the snapshot", async () => {
  const view: HookView = mounted();
  vi.mocked(invoke).mockResolvedValue({ ...SNAPSHOT, locale: "ja" });
  act((): void => emitBackend("download", { state: "failed", id: "m", error: { code: "internal" } }));
  expect(view.result.current.state.downloads.get("m")?.error).toStrictEqual({ code: "internal" });
  await waitFor((): void => {
    expect(view.result.current.state.snapshot.locale).toBe("ja");
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["snapshot"]]);
});

it("reports a failed snapshot refresh", async () => {
  const view: HookView = mounted();
  vi.mocked(invoke).mockRejectedValue(new Error("gone"));
  act((): void => emitBackend("download", { state: "finished", id: "m" }));
  await waitFor((): void => {
    expect(view.result.current.state.loadError).toStrictEqual({ code: "internal", detail: "Error: gone" });
  });
});

it("tracks hovering files", () => {
  const view: HookView = mounted();
  act((): void => onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "enter" } }));
  expect(view.result.current.state.hovering).toBe(true);
  act((): void => onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "leave" } }));
  expect(view.result.current.state.hovering).toBe(false);
});

it("adds only the videos of a drop and stops hovering", () => {
  const view: HookView = mounted();
  act((): void => onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "enter" } }));
  act((): void =>
    onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "drop", paths: ["/a.mp4", "/b.txt"] } }),
  );
  expect(view.result.current.state.hovering).toBe(false);
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["add_videos", { paths: ["/a.mp4"] }]]);
});

it("reports videos the backend refuses", async () => {
  const view: HookView = mounted();
  vi.mocked(invoke).mockRejectedValue(new Error("refused"));
  act((): void => view.result.current.dropVideos(["/a.mp4"]));
  await waitFor((): void => {
    expect(view.result.current.state.loadError).toStrictEqual({ code: "internal", detail: "Error: refused" });
  });
});

it("shows new settings at once and saves only the last of a burst", async () => {
  vi.useFakeTimers();
  const view: HookView = mounted();
  act((): void => view.result.current.updateSettings({ ...SETTINGS, language: "ja" }));
  act((): void => view.result.current.updateSettings({ ...SETTINGS, language: "ko" }));
  expect(view.result.current.state.snapshot.settings.language).toBe("ko");
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  await act(async (): Promise<void> => {
    await vi.advanceTimersByTimeAsync(250);
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([
    ["save_settings", { settings: { ...SETTINGS, language: "ko" } }],
  ]);
});

it("applies the snapshot a save returns and clears an old failure", async () => {
  vi.useFakeTimers();
  const view: HookView = mounted();
  act((): void => view.result.current.setState((state) => ({ ...state, loadError: { code: "internal" } })));
  vi.mocked(invoke).mockResolvedValue({ ...SNAPSHOT, locale: "de" });
  act((): void => view.result.current.updateSettings(SETTINGS));
  await act(async (): Promise<void> => {
    await vi.advanceTimersByTimeAsync(250);
  });
  expect(view.result.current.state.snapshot.locale).toBe("de");
  expect(view.result.current.state.loadError).toBe(null);
});

it("reports a failed save", async () => {
  vi.useFakeTimers();
  const view: HookView = mounted();
  vi.mocked(invoke).mockRejectedValue(new Error("read only"));
  act((): void => view.result.current.updateSettings(SETTINGS));
  await act(async (): Promise<void> => {
    await vi.advanceTimersByTimeAsync(250);
  });
  expect(view.result.current.state.loadError).toStrictEqual({ code: "internal", detail: "Error: read only" });
});

it("stops every subscription when unmounted", async () => {
  const stopJob: Mock<() => void> = vi.fn();
  const stopDownload: Mock<() => void> = vi.fn();
  const stopDrop: Mock<() => void> = vi.fn();
  vi.mocked(listen).mockResolvedValueOnce(stopJob).mockResolvedValueOnce(stopDownload);
  onDragDropEvent.mockResolvedValue(stopDrop);
  mounted().unmount();
  await waitFor((): void => {
    expect(stopJob).toHaveBeenCalledTimes(1);
  });
  expect(stopDownload).toHaveBeenCalledTimes(1);
  expect(stopDrop).toHaveBeenCalledTimes(1);
});

it("shows a subscription that could not start while the app is open", async () => {
  vi.mocked(listen).mockRejectedValueOnce(new Error("no events"));
  const view: HookView = mounted();
  await waitFor((): void => {
    expect(view.result.current.state.loadError).toStrictEqual({
      code: "internal",
      detail: "Error: no events",
    });
  });
});

it("survives a subscription that fails to stop", async () => {
  vi.mocked(listen).mockResolvedValueOnce((): void => {
    throw new Error("stuck");
  });
  const stopDownload: Mock<() => void> = vi.fn();
  vi.mocked(listen).mockResolvedValueOnce(stopDownload);
  mounted().unmount();
  await waitFor((): void => {
    expect(stopDownload).toHaveBeenCalledTimes(1);
  });
});
