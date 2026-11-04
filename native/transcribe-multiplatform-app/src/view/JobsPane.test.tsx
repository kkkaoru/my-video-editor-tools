// Runs with bun: the drop target, readiness note and every job state.
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ModelStatus, Settings, Snapshot } from "../backend/types.ts";
import { enMessages } from "../i18n/locale-en.ts";
import { initialState, type JobRow } from "../state/app-state.ts";
import type { AppStore } from "../state/use-app-store.ts";
import { elapsedParts, fileName, JobsPane, missingRoles, modelLabel } from "./JobsPane.tsx";

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
const WHISPER: ModelStatus = {
  id: "whisper",
  role: "transcribe",
  label: "Whisper",
  source_url: "",
  download_bytes: 0,
  found_at: "/w",
  blocked: null,
  recommended: true,
  hidden: false,
};
const DIARIZER: ModelStatus = {
  ...WHISPER,
  id: "diarizer",
  role: "diarize",
  label: "Diarizer",
  found_at: "/d",
};
const READY: Snapshot = {
  locale: "en-US",
  hardware: { is_mac: true, memory_gib: 16, nvidia_name: null, nvidia_memory_gib: 0 },
  settings: SETTINGS,
  models: [WHISPER, DIARIZER],
  models_dir: "/models",
};
const QUEUED_JOB: JobRow = {
  id: 1,
  path: "/videos/talk.mp4",
  state: "queued",
  startedAt: 0,
  progress: null,
  outcome: null,
  error: null,
};

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const storeWith = (snapshot: Snapshot, jobs: JobRow[], hovering: boolean): AppStore => ({
  state: { ...initialState(snapshot), jobs, hovering },
  setState: vi.fn(),
  updateSettings: vi.fn(),
  applySnapshot: vi.fn(),
  dropVideos: vi.fn(),
});

function showJob(job: JobRow, now: number): AppStore {
  const store: AppStore = storeWith(READY, [job], false);
  render(<JobsPane store={store} snapshot={READY} now={now} />);
  return store;
}

const failureOf = (store: AppStore): unknown =>
  vi.mocked(store.setState).mock.calls[0]?.[0](initialState(READY)).loadError;

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(invoke).mockResolvedValue(null);
});

afterEach(cleanup);

it("takes the file name from either kind of path", () => {
  expect(fileName("/videos/talk.mp4")).toBe("talk.mp4");
  expect(fileName("C:\\videos\\talk.mp4")).toBe("talk.mp4");
  expect(fileName("talk.mp4")).toBe("talk.mp4");
});

it("splits elapsed time into minutes and seconds", () => {
  expect(elapsedParts(61_500)).toStrictEqual({ minutes: 1, seconds: 1 });
});

it("needs a present, unblocked model for each required role", () => {
  expect(missingRoles(READY, enMessages)).toStrictEqual([]);
  expect(
    missingRoles({ ...READY, models: [{ ...WHISPER, found_at: null }, DIARIZER] }, enMessages),
  ).toStrictEqual(["transcription"]);
  expect(
    missingRoles(
      { ...READY, models: [WHISPER, { ...DIARIZER, blocked: { code: "needs_nvidia" } }] },
      enMessages,
    ),
  ).toStrictEqual(["speaker separation"]);
});

it("labels a model by id, and falls back to the id or nothing", () => {
  expect(modelLabel(READY, "whisper")).toBe("Whisper");
  expect(modelLabel(READY, "unknown")).toBe("unknown");
  expect(modelLabel(READY, undefined)).toBe("");
});

it("invites a drop and hides the readiness note when ready", () => {
  render(<JobsPane store={storeWith(READY, [], false)} snapshot={READY} now={0} />);
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Drop videos here");
  expect(screen.queryByRole("alert")).toBe(null);
  expect(screen.getByRole("region", { name: "Drop videos here" }).className).toBe(
    "flex flex-col items-center gap-2 rounded-lg border-2 border-dashed p-8 text-center border-line bg-panel",
  );
});

it("changes the drop target while files hover", () => {
  render(<JobsPane store={storeWith(READY, [], true)} snapshot={READY} now={0} />);
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Release to add the videos");
  expect(screen.getByRole("region", { name: "Drop videos here" }).className).toBe(
    "flex flex-col items-center gap-2 rounded-lg border-2 border-dashed p-8 text-center border-accent bg-canvas",
  );
});

it("names the missing roles", () => {
  const snapshot: Snapshot = { ...READY, models: [] };
  render(<JobsPane store={storeWith(snapshot, [], false)} snapshot={snapshot} now={0} />);
  expect(screen.getByRole("alert").textContent).toBe(
    "No model is available for: transcription and speaker separation. Download one in Models, or add a folder that already has it.",
  );
});

it("names the missing roles in a POSIX locale that is not a language tag", () => {
  const snapshot: Snapshot = { ...READY, locale: "C", models: [] };
  render(<JobsPane store={storeWith(snapshot, [], false)} snapshot={snapshot} now={0} />);
  expect(screen.getByRole("alert").textContent).toBe(
    "No model is available for: transcription and speaker separation. Download one in Models, or add a folder that already has it.",
  );
});

it("adds the chosen videos", async () => {
  vi.mocked(open).mockResolvedValue(["/a.mp4"]);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} now={0} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  await waitFor((): void => {
    expect(vi.mocked(store.dropVideos).mock.calls).toStrictEqual([[["/a.mp4"]]]);
  });
});

it("adds nothing when the file dialog is dismissed", async () => {
  vi.mocked(open).mockResolvedValue(null);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} now={0} />);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  });
  expect(vi.mocked(store.dropVideos).mock.calls).toStrictEqual([]);
});

it("reports a file dialog that fails", async () => {
  vi.mocked(open).mockRejectedValue(new Error("no dialog"));
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} now={0} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: no dialog" });
  });
});

it("shows a queued job that can be cancelled", async () => {
  showJob(QUEUED_JOB, 0);
  expect(screen.getByRole("heading", { level: 2, name: "talk.mp4" }).textContent).toBe("talk.mp4");
  expect(screen.getByRole("status").textContent).toBe("Waiting in line");
  fireEvent.click(screen.getByRole("button", { name: "Cancel talk.mp4" }));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["cancel_job", { id: 1 }]]);
});

it("reports a cancel that fails", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("too late"));
  const store: AppStore = showJob(QUEUED_JOB, 0);
  fireEvent.click(screen.getByRole("button", { name: "Cancel talk.mp4" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: too late" });
  });
});

it("shows a running job before its first progress report", () => {
  showJob({ ...QUEUED_JOB, state: "running" }, 0);
  expect(screen.getByText("Preparing").tagName).toBe("P");
  expect(screen.queryByRole("progressbar")).toBe(null);
});

it("shows percent and elapsed time of a determinate stage", () => {
  showJob(
    { ...QUEUED_JOB, state: "running", progress: { stage: "transcribe", overall: 0.42, determinate: true } },
    125_000,
  );
  expect(screen.getByRole("status").textContent).toBe("Transcribing");
  expect(screen.getByText("42% / 2 min 5 s elapsed").tagName).toBe("P");
  const bar: HTMLElement = screen.getByRole("progressbar", { name: "Progress of talk.mp4" });
  expect(bar.getAttribute("aria-valuenow")).toBe("0.42");
  expect(bar.getAttribute("aria-valuetext")).toBe("Transcribing: 42%");
});

it("says so when a stage reports no percentage", () => {
  showJob(
    { ...QUEUED_JOB, state: "running", progress: { stage: "diarize", overall: 0, determinate: false } },
    0,
  );
  expect(screen.getByText("this model reports no percentage / 0 min 0 s elapsed").tagName).toBe("P");
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe(null);
});

it("shows a cancelled job with no action", () => {
  showJob({ ...QUEUED_JOB, state: "cancelled" }, 0);
  expect(screen.getByRole("status").textContent).toBe("Cancelled");
  expect(screen.queryByRole("button", { name: "Cancel talk.mp4" })).toBe(null);
});

it("shows the results of a finished job and can write it again", () => {
  showJob(
    {
      ...QUEUED_JOB,
      state: "finished",
      outcome: {
        outputs: ["/a.srt", "/a.fcpxml"],
        base: "/videos/talk",
        speaker_count: 2,
        cue_count: 9,
        faceless: false,
      },
    },
    0,
  );
  expect(screen.getByRole("status").textContent).toBe("Finished: 9 captions, 2 speakers");
  expect(
    screen
      .getAllByRole("listitem")
      .slice(1)
      .map((item: HTMLElement): string | null => item.textContent),
  ).toStrictEqual(["/a.srt", "/a.fcpxml"]);
  expect(screen.queryByText("Captions where no face was found were placed at the bottom of the video.")).toBe(
    null,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Write talk.mp4 again with the current position and colours" }),
  );
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([
    ["relayout_transcript", { path: "/videos/talk.transcript.json" }],
  ]);
});

it("mentions captions placed without a face", () => {
  showJob(
    {
      ...QUEUED_JOB,
      state: "finished",
      outcome: { outputs: [], base: "/videos/talk", speaker_count: 1, cue_count: 1, faceless: true },
    },
    0,
  );
  expect(
    screen.getByText("Captions where no face was found were placed at the bottom of the video.").tagName,
  ).toBe("P");
});

it("shows no results for a finished job without an outcome", () => {
  showJob({ ...QUEUED_JOB, state: "finished" }, 0);
  expect(screen.queryByRole("status")).toBe(null);
  expect(screen.queryByRole("button", { name: /talk\.mp4/ })).toBe(null);
});

it("reports a rewrite that fails", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("gone"));
  const store: AppStore = showJob(
    {
      ...QUEUED_JOB,
      state: "finished",
      outcome: { outputs: [], base: "/videos/talk", speaker_count: 1, cue_count: 1, faceless: false },
    },
    0,
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Write talk.mp4 again with the current position and colours" }),
  );
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: gone" });
  });
});

it("explains a failed job with the model's label", () => {
  showJob({ ...QUEUED_JOB, state: "failed", error: { code: "model_missing", model: "whisper" } }, 0);
  expect(screen.getByRole("alert").textContent).toBe("Whisper is not installed. Download it in Models.");
});
