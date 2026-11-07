// Runs with bun: the store's subscriptions, debounced saving and failure reporting.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, cleanup, renderHook, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, type Mock, vi } from "vitest";
import type { Settings, Snapshot } from "../backend/types.ts";
import {
  type AppStore,
  onlySubtitles,
  onlyVideos,
  reportFailure,
  SUBTITLE_EXTENSIONS,
  useAppStore,
  VIDEO_EXTENSIONS,
} from "./use-app-store.ts";

type DropPayload = { type: "drop"; paths: string[] } | { type: "enter" | "over" | "leave" };
type DropCallback = (event: { payload: DropPayload }) => void;

interface HookView {
  result: { current: AppStore };
  unmount: () => void;
}

const onDragDropEvent: Mock<(callback: DropCallback) => Promise<() => void>> = vi.hoisted(() => vi.fn());
const SETTINGS: Settings = {
  palette: [],
  font_family: "Sans",
  font_ratio: 0.05,
  width_ratio: 0.8,
  safe_left_percent: 5,
  safe_right_percent: 5,
  placement: {
    anchor: "video_top",
    offset_x_percent: 0,
    offset_y_percent: 0,
  },
  language: "auto",
  face_model: "c",
  model_dirs: [],
  active_profile: "",
  line_height: 1.448,
  separation_backend: "onnx",
  speaker_count: 1,
  subtitle_format: "fcpxml",
  ui_language: "",
  ui_theme: "system",
};
const SNAPSHOT: Snapshot = {
  version: "1.2.3",
  locale: "en",
  hardware: { is_mac: true, memory_gib: 16, gpu: null },
  backend: "default",
  settings: SETTINGS,
  models: [],
  speaker_plan: null,
  speaker_missing: null,
  models_dir: "/models",
  profiles: [],
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent }) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const READY: Snapshot = {
  ...SNAPSHOT,
  models: [
    {
      id: "t",
      role: "transcribe",
      label: "T",
      source_url: "",
      download_bytes: 1,
      found_at: "/t",
      blocked: null,
      recommended: true,
    },
    {
      id: "d",
      role: "diarize",
      label: "D",
      source_url: "",
      download_bytes: 1,
      found_at: "/d",
      blocked: null,
      recommended: true,
    },
  ],
};

const emitBackend = (name: string, payload: unknown): void =>
  vi
    .mocked(listen)
    .mock.calls.find(([event]): boolean => event === name)?.[1]({ event: name, id: 0, payload });

const mounted = (): HookView => renderHook((): AppStore => useAppStore(SNAPSHOT));

const mountedReady = (): HookView => renderHook((): AppStore => useAppStore(READY));

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
  const view: HookView = mountedReady();
  act((): void => onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "enter" } }));
  act((): void =>
    onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "drop", paths: ["/a.mp4", "/b.txt"] } }),
  );
  expect(view.result.current.state.hovering).toBe(false);
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["add_videos", { paths: ["/a.mp4"] }]]);
});

it("reports videos the backend refuses", async () => {
  const view: HookView = mountedReady();
  vi.mocked(invoke).mockRejectedValue(new Error("refused"));
  act((): void => view.result.current.dropFiles(["/a.mp4"]));
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
    [
      "save_settings",
      {
        settings: {
          palette: [],
          font_family: "Sans",
          font_ratio: 0.05,
          width_ratio: 0.8,
          safe_left_percent: 5,
          safe_right_percent: 5,
          placement: {
            anchor: "video_top",
            offset_x_percent: 0,
            offset_y_percent: 0,
          },
          language: "ko",
          face_model: "c",
          model_dirs: [],
          active_profile: "",
          line_height: 1.448,
          separation_backend: "onnx",
          speaker_count: 1,
          subtitle_format: "fcpxml",
          ui_language: "",
          ui_theme: "system",
        },
      },
    ],
  ]);
});

it("takes the models and the language from a save, so newer settings stay, and ends an old failure", async () => {
  vi.useFakeTimers();
  const view: HookView = mounted();
  act((): void => view.result.current.setState((state) => ({ ...state, loadError: { code: "internal" } })));
  vi.mocked(invoke).mockResolvedValue({
    ...SNAPSHOT,
    locale: "de",
    settings: { ...SETTINGS, language: "stale" },
    models: [
      {
        id: "m",
        role: "face",
        label: "M",
        source_url: "",
        download_bytes: 0,
        found_at: null,
        blocked: null,
        recommended: false,
      },
    ],
  });
  act((): void => view.result.current.updateSettings({ ...SETTINGS, language: "ja" }));
  await act(async (): Promise<void> => {
    await vi.advanceTimersByTimeAsync(250);
  });
  expect(view.result.current.state.snapshot.models.map(({ id }): string => id)).toStrictEqual(["m"]);
  expect(view.result.current.state.snapshot.settings.language).toBe("ja");
  expect(view.result.current.state.snapshot.locale).toBe("de");
  expect(view.result.current.state.loadError).toBe(null);
});

it("does not rescan the models for download progress", () => {
  const view: HookView = mounted();
  act((): void => emitBackend("download", { state: "running", id: "m", done_bytes: 1, total_bytes: 2 }));
  expect(view.result.current.state.downloads.get("m")?.doneBytes).toBe(1);
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
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

it("lists the subtitle files the picker offers", () => {
  expect(SUBTITLE_EXTENSIONS).toStrictEqual(["drt", "fcpxml", "srt", "ttml", "ass", "json"]);
});

it("keeps only subtitle paths, in any case", () => {
  expect(
    onlySubtitles(["/a.SRT", "/b.ttml", "/c.fcpxml", "/d.transcript.json", "/e.json", "/f.mp4"]),
  ).toStrictEqual(["/a.SRT", "/b.ttml", "/c.fcpxml", "/d.transcript.json"]);
});

it("passes the backend's translatable errors through", () => {
  const setState: Mock<AppStore["setState"]> = vi.fn();
  reportFailure(setState, { code: "profile_exists", name: "A" });
  expect(setState.mock.calls[0]?.[0](mounted().result.current.state).loadError).toStrictEqual({
    code: "profile_exists",
    name: "A",
  });
});

it("reports other rejected objects as text", () => {
  const setState: Mock<AppStore["setState"]> = vi.fn();
  reportFailure(setState, { code: 7 });
  expect(setState.mock.calls[0]?.[0](mounted().result.current.state).loadError).toStrictEqual({
    code: "internal",
    detail: '{"code":7}',
  });
});

it("reports a rejected null as text", () => {
  const setState: Mock<AppStore["setState"]> = vi.fn();
  reportFailure(setState, null);
  expect(setState.mock.calls[0]?.[0](mounted().result.current.state).loadError).toStrictEqual({
    code: "internal",
    detail: "null",
  });
});

it("sends a mixed drop to the right jobs", () => {
  mountedReady();
  act((): void =>
    onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "drop", paths: ["/a.mp4", "/b.ttml", "/c.txt"] } }),
  );
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([
    ["add_videos", { paths: ["/a.mp4"] }],
    ["reformat_files", { paths: ["/b.ttml"] }],
  ]);
});

it("starts nothing for a drop with neither videos nor subtitles", () => {
  mountedReady();
  act((): void => onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "drop", paths: ["/a.txt"] } }));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
});

it("refuses videos without their models, naming what is missing, and starts nothing", () => {
  const view: HookView = mounted();
  act((): void => view.result.current.dropFiles(["/a.mp4"]));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  expect(view.result.current.state.loadError).toStrictEqual({
    code: "models_missing",
    roles: ["transcribe"],
  });
});

it("refuses videos when the backend says speaker models are missing, and starts nothing", () => {
  const heavy: Snapshot = { ...READY, speaker_missing: { missing: ["separate"], suggested: "m" } };
  const view: HookView = renderHook((): AppStore => useAppStore(heavy));
  act((): void => view.result.current.dropFiles(["/a.mp4"]));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  expect(view.result.current.state.loadError).toStrictEqual({
    code: "speaker_models_missing",
    missing: ["separate"],
    suggested: "m",
  });
});

it("adds videos for several speakers when the backend has a plan", () => {
  const planned: Snapshot = {
    ...READY,
    settings: { ...READY.settings, speaker_count: 0 },
    speaker_plan: {
      diarize: "d",
      whisper_on: "gpu",
      diarize_on: "gpu",
      separate_on: "cpu",
      separator: "s",
      deep: false,
    },
  };
  const view: HookView = renderHook((): AppStore => useAppStore(planned));
  act((): void => view.result.current.dropFiles(["/a.mp4"]));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["add_videos", { paths: ["/a.mp4"] }]]);
});

it("still rewrites subtitle files without any model", () => {
  const view: HookView = mounted();
  act((): void => view.result.current.dropFiles(["/a.srt"]));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["reformat_files", { paths: ["/a.srt"] }]]);
  expect(view.result.current.state.loadError).toBe(null);
});

it("reports subtitle files the backend refuses", async () => {
  const view: HookView = mounted();
  vi.mocked(invoke).mockRejectedValue(new Error("refused"));
  act((): void => view.result.current.dropFiles(["/a.srt"]));
  await waitFor((): void => {
    expect(view.result.current.state.loadError).toStrictEqual({ code: "internal", detail: "Error: refused" });
  });
});

it("drops a waiting save when a snapshot replaces the settings", async () => {
  vi.useFakeTimers();
  const view: HookView = mounted();
  act((): void => view.result.current.updateSettings({ ...SETTINGS, language: "ja" }));
  act((): void =>
    view.result.current.replaceSnapshot({ ...SNAPSHOT, settings: { ...SETTINGS, language: "ko" } }),
  );
  await act(async (): Promise<void> => {
    await vi.advanceTimersByTimeAsync(250);
  });
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  expect(view.result.current.state.snapshot.settings.language).toBe("ko");
});

it("replaces the snapshot when no save is waiting", () => {
  const view: HookView = mounted();
  act((): void => view.result.current.replaceSnapshot({ ...SNAPSHOT, locale: "de" }));
  expect(view.result.current.state.snapshot.locale).toBe("de");
});

it("refuses a drop with nothing it can use, once", () => {
  const view: HookView = mountedReady();
  act((): void =>
    onDragDropEvent.mock.calls[0]?.[0]({ payload: { type: "drop", paths: ["/a.txt", "/b.pdf"] } }),
  );
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  expect(view.result.current.state.loadError).toStrictEqual({ code: "no_supported_files" });
});

it("refuses a plain .json, because only a transcript can be written again", () => {
  const view: HookView = mountedReady();
  act((): void => view.result.current.dropFiles(["/settings.json"]));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([]);
  expect(view.result.current.state.loadError).toStrictEqual({ code: "no_supported_files" });
});

it("says nothing when a drop brings no file at all", () => {
  const view: HookView = mountedReady();
  act((): void => view.result.current.dropFiles([]));
  expect(view.result.current.state.loadError).toBe(null);
});

it("does not call a usable file unsupported because another one is", () => {
  const view: HookView = mountedReady();
  act((): void => view.result.current.dropFiles(["/a.txt", "/b.srt"]));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["reformat_files", { paths: ["/b.srt"] }]]);
  expect(view.result.current.state.loadError).toBe(null);
});
