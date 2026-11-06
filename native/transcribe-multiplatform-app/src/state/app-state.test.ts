// Runs with bun: the pure transitions of jobs, downloads and settings.
import { expect, it } from "vitest";
import type { BackendError, ModelStatus, Settings, Snapshot } from "../backend/types.ts";
import {
  type AppState,
  applyDownloadEvent,
  applyJobEvent,
  initialState,
  isHeavyRole,
  isHeavySpeakerCount,
  isWorking,
  lookChanged,
  missingModelRoles,
  startedDownload,
  uiLocale,
  withModels,
  withSaveAnswer,
  withSettings,
} from "./app-state.ts";

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
  diarize_model: "b",
  face_model: "c",
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
  locale: "en",
  hardware: { is_mac: true, memory_gib: 16, gpu: null },
  backend: "default",
  settings: SETTINGS,
  models: [],
  models_dir: "/models",
  profiles: [],
};
const FAILURE: BackendError = { code: "internal", detail: "boom" };

const queued = (): AppState =>
  applyJobEvent(initialState(SNAPSHOT), { state: "queued", id: 1, path: "/a.mp4" }, 100);

it("starts empty", () => {
  expect(initialState(SNAPSHOT)).toStrictEqual({
    snapshot: {
      version: "1.2.3",
      locale: "en",
      hardware: { is_mac: true, memory_gib: 16, gpu: null },
      backend: "default",
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
        language: "auto",
        diarize_model: "b",
        face_model: "c",
        model_dirs: [],
        active_profile: "",
        line_height: 1.448,
        speaker_count: 0,
        subtitle_format: "fcpxml",
        ui_language: "",
        ui_theme: "system",
      },
      models: [],
      models_dir: "/models",
      profiles: [],
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
        font_missing: false,
        speakers_missing: false,
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
        font_missing: false,
        speakers_missing: false,
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

it("replaces the models and ends an earlier failure, keeping the settings", () => {
  const failed: AppState = { ...initialState(SNAPSHOT), loadError: { code: "internal" } };
  const state: AppState = withModels(failed, []);
  expect(state.snapshot.models).toStrictEqual([]);
  expect(state.snapshot.settings.language).toBe("auto");
  expect(state.loadError).toBe(null);
});

const MODEL: ModelStatus = {
  id: "t",
  role: "transcribe",
  label: "T",
  source_url: "",
  download_bytes: 1,
  found_at: "/t",
  blocked: null,
  recommended: true,
};

it("needs only a transcription model for one speaker", () => {
  const one: Settings = { ...SETTINGS, speaker_count: 1 };
  expect(missingModelRoles({ settings: one, models: [] })).toStrictEqual(["transcribe"]);
  expect(missingModelRoles({ settings: one, models: [MODEL] })).toStrictEqual([]);
  expect(
    missingModelRoles({ settings: one, models: [{ ...MODEL, blocked: { code: "needs_nvidia" } }] }),
  ).toStrictEqual(["transcribe"]);
});

it("needs every model of the chain for automatic detection and for more speakers", () => {
  const diarizer: ModelStatus = { ...MODEL, id: "n", role: "diarize" };
  const separator: ModelStatus = { ...MODEL, id: "m", role: "separate" };
  expect(missingModelRoles({ settings: { ...SETTINGS, speaker_count: 0 }, models: [MODEL] })).toStrictEqual([
    "diarize",
    "separate",
  ]);
  expect(
    missingModelRoles({ settings: { ...SETTINGS, speaker_count: 2 }, models: [MODEL, diarizer] }),
  ).toStrictEqual(["separate"]);
  expect(
    missingModelRoles({ settings: { ...SETTINGS, speaker_count: 2 }, models: [MODEL, diarizer, separator] }),
  ).toStrictEqual([]);
  expect(
    missingModelRoles({
      settings: { ...SETTINGS, speaker_count: 2 },
      models: [MODEL, { ...diarizer, blocked: { code: "needs_nvidia" } }, separator],
    }),
  ).toStrictEqual(["diarize"]);
});

it("calls only one speaker light", () => {
  expect(isHeavySpeakerCount({ speaker_count: 1 })).toBe(false);
  expect(isHeavySpeakerCount({ speaker_count: 0 })).toBe(true);
  expect(isHeavySpeakerCount({ speaker_count: 2 })).toBe(true);
});

it("knows which kinds of model only the heavy chain needs", () => {
  expect(isHeavyRole("separate")).toBe(true);
  expect(isHeavyRole("diarize")).toBe(true);
  expect(isHeavyRole("transcribe")).toBe(false);
});

it("notices a look that differs from the saved one, and ignores everything else", () => {
  expect(lookChanged(SETTINGS, SETTINGS)).toBe(false);
  expect(lookChanged({ ...SETTINGS, language: "ja", speaker_count: 2 }, SETTINGS)).toBe(false);
  expect(lookChanged({ ...SETTINGS, font_ratio: 0.06 }, SETTINGS)).toBe(true);
  expect(lookChanged({ ...SETTINGS, font_family: "Other" }, SETTINGS)).toBe(true);
});

it("counts a safe zone as part of the look a profile saves", () => {
  expect(lookChanged({ ...SETTINGS, safe_left_percent: 9 }, SETTINGS)).toBe(true);
});

it("shows the language chosen in the settings, or the system's when none is", () => {
  expect(uiLocale({ locale: "ja-JP", settings: SETTINGS })).toBe("ja-JP");
  expect(uiLocale({ locale: "ja-JP", settings: { ...SETTINGS, ui_language: "de" } })).toBe("de");
});

it("takes the models and the language from a save's answer, and keeps newer settings on screen", () => {
  const state = initialState(SNAPSHOT);
  const answer: Snapshot = { ...SNAPSHOT, locale: "fr", settings: { ...SETTINGS, language: "older" } };
  const updated = withSaveAnswer(state, answer);
  expect(updated.snapshot.locale).toBe("fr");
  expect(updated.snapshot.settings).toStrictEqual(SETTINGS);
});

it("counts waiting and running videos and running downloads as work", () => {
  const job = { id: 1, path: "/a.mp4", startedAt: 0 };
  const download = { running: true, doneBytes: 0, totalBytes: 1, error: null };
  expect(isWorking({ jobs: [], downloads: new Map() })).toBe(false);
  expect(isWorking({ jobs: [{ ...job, state: "queued" }], downloads: new Map() })).toBe(true);
  expect(isWorking({ jobs: [], downloads: new Map([["m", download]]) })).toBe(true);
  expect(isWorking({ jobs: [], downloads: new Map([["m", { ...download, running: false }]]) })).toBe(false);
});
