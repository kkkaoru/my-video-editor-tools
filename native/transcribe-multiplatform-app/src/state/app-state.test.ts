// Runs with bun: the pure transitions of jobs, downloads and settings.
import { expect, it } from "vitest";
import type { BackendError, Settings, Snapshot } from "../backend/types.ts";
import {
  type AppState,
  applyDownloadEvent,
  applyJobEvent,
  initialState,
  startedDownload,
  withModels,
  withSettings,
} from "./app-state.ts";

const SETTINGS: Settings = {
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
  rim_ratio: 0.034,
  line_height: 1.448,
  speaker_count: 0,
};
const SNAPSHOT: Snapshot = {
  locale: "en",
  hardware: { is_mac: true, memory_gib: 16, gpu: null },
  backend: "default",
  settings: SETTINGS,
  models: [],
  models_dir: "/models",
};
const FAILURE: BackendError = { code: "internal", detail: "boom" };

const queued = (): AppState =>
  applyJobEvent(initialState(SNAPSHOT), { state: "queued", id: 1, path: "/a.mp4" }, 100);

it("starts empty", () => {
  expect(initialState(SNAPSHOT)).toStrictEqual({
    snapshot: {
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
        rim_ratio: 0.034,
        line_height: 1.448,
        speaker_count: 0,
      },
      models: [],
      models_dir: "/models",
    },
    jobs: [],
    downloads: new Map(),
    hovering: false,
    loadError: null,
  });
});

it("puts a queued job first and stamps its start", () => {
  const state: AppState = applyJobEvent(queued(), { state: "queued", id: 2, path: "/b.mp4" }, 200);
  expect(state.jobs).toStrictEqual([
    { id: 2, path: "/b.mp4", state: "queued", startedAt: 200 },
    { id: 1, path: "/a.mp4", state: "queued", startedAt: 100 },
  ]);
});

it("records progress on the running job only", () => {
  const two: AppState = applyJobEvent(queued(), { state: "queued", id: 2, path: "/b.mp4" }, 200);
  const state: AppState = applyJobEvent(
    two,
    {
      state: "running",
      id: 1,
      progress: {
        stage: "faces",
        overall: 0.5,
        stage_fraction: 0.25,
        determinate: true,
        plan: ["faces", "export"],
        remaining_seconds: 90,
      },
    },
    300,
  );
  expect(state.jobs).toStrictEqual([
    { id: 2, path: "/b.mp4", state: "queued", startedAt: 200 },
    {
      id: 1,
      path: "/a.mp4",
      state: "running",
      startedAt: 100,
      progress: {
        stage: "faces",
        overall: 0.5,
        stage_fraction: 0.25,
        determinate: true,
        plan: ["faces", "export"],
        remaining_seconds: 90,
      },
    },
  ]);
});

it("records the outcome of a finished job", () => {
  const state: AppState = applyJobEvent(
    queued(),
    {
      state: "finished",
      id: 1,
      outcome: {
        outputs: ["/a.srt"],
        base: "/a",
        speaker_count: 2,
        cue_count: 9,
        faceless: false,
        requested_speakers: 0,
      },
    },
    300,
  );
  expect(state.jobs).toStrictEqual([
    {
      id: 1,
      path: "/a.mp4",
      state: "finished",
      startedAt: 100,
      outcome: {
        outputs: ["/a.srt"],
        base: "/a",
        speaker_count: 2,
        cue_count: 9,
        faceless: false,
        requested_speakers: 0,
      },
    },
  ]);
});

it("records the error of a failed job", () => {
  const state: AppState = applyJobEvent(queued(), { state: "failed", id: 1, error: FAILURE }, 300);
  expect(state.jobs).toStrictEqual([
    {
      id: 1,
      path: "/a.mp4",
      state: "failed",
      startedAt: 100,
      error: { code: "internal", detail: "boom" },
    },
  ]);
});

it("marks a cancelled job", () => {
  const state: AppState = applyJobEvent(queued(), { state: "cancelled", id: 1 }, 300);
  expect(state.jobs).toStrictEqual([{ id: 1, path: "/a.mp4", state: "cancelled", startedAt: 100 }]);
});

it("drops an event for an unknown job", () => {
  const state: AppState = applyJobEvent(queued(), { state: "cancelled", id: 99 }, 300);
  expect(state.jobs).toStrictEqual([{ id: 1, path: "/a.mp4", state: "queued", startedAt: 100 }]);
});

it("marks a download as started", () => {
  expect(startedDownload(initialState(SNAPSHOT), "yunet", 500).downloads).toStrictEqual(
    new Map([["yunet", { running: true, doneBytes: 0, totalBytes: 500, error: null }]]),
  );
});

it("records download progress", () => {
  const state: AppState = applyDownloadEvent(startedDownload(initialState(SNAPSHOT), "yunet", 500), {
    state: "running",
    id: "yunet",
    done_bytes: 100,
    total_bytes: 600,
  });
  expect(state.downloads).toStrictEqual(
    new Map([["yunet", { running: true, doneBytes: 100, totalBytes: 600, error: null }]]),
  );
});

it("records a failed download", () => {
  const state: AppState = applyDownloadEvent(startedDownload(initialState(SNAPSHOT), "yunet", 500), {
    state: "failed",
    id: "yunet",
    error: FAILURE,
  });
  expect(state.downloads).toStrictEqual(
    new Map([
      ["yunet", { running: false, doneBytes: 0, totalBytes: 0, error: { code: "internal", detail: "boom" } }],
    ]),
  );
});

it("forgets a finished download but keeps the others", () => {
  const two: AppState = startedDownload(startedDownload(initialState(SNAPSHOT), "yunet", 500), "other", 7);
  const state: AppState = applyDownloadEvent(two, { state: "finished", id: "yunet" });
  expect(state.downloads).toStrictEqual(
    new Map([["other", { running: true, doneBytes: 0, totalBytes: 7, error: null }]]),
  );
});

it("replaces the settings", () => {
  const state: AppState = withSettings(initialState(SNAPSHOT), { ...SETTINGS, language: "ja" });
  expect(state.snapshot.settings.language).toBe("ja");
  expect(state.snapshot.locale).toBe("en");
});

it("replaces only the models", () => {
  const failed: AppState = { ...initialState(SNAPSHOT), loadError: { code: "internal" } };
  const state: AppState = withModels(failed, []);
  expect(state.snapshot.models).toStrictEqual([]);
  expect(state.snapshot.settings.language).toBe("auto");
  expect(state.loadError).toStrictEqual({ code: "internal" });
});
