// Runs with bun: the drop target, readiness note and every job state.
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ModelStatus, Settings, Snapshot } from "../backend/types.ts";
import { enMessages } from "../i18n/locale-en.ts";
import { initialState, type JobRow } from "../state/app-state.ts";
import type { AppStore } from "../state/use-app-store.ts";
import {
  fileName,
  JobsPane,
  missingRoles,
  modelLabel,
  outputPurpose,
  remainingText,
  slowProcessingNotice,
  subtitleTargetText,
} from "./JobsPane.tsx";

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
  active_profile: "",
  rim_ratio: 0.034,
  line_height: 1.448,
  speaker_count: 0,
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
};
const DIARIZER: ModelStatus = {
  ...WHISPER,
  id: "diarizer",
  role: "diarize",
  label: "Diarizer",
  found_at: "/d",
};
const READY: Snapshot = {
  version: "1.2.3",
  locale: "en-US",
  hardware: { is_mac: true, memory_gib: 16, gpu: null },
  backend: "default",
  settings: SETTINGS,
  models: [WHISPER, DIARIZER],
  models_dir: "/models",
  profiles: [],
};
const QUEUED_JOB: JobRow = {
  id: 1,
  path: "/videos/talk.mp4",
  state: "queued",
  startedAt: 0,
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
  replaceSnapshot: vi.fn(),
  dropFiles: vi.fn(),
});

function showJob(job: JobRow): AppStore {
  const store: AppStore = storeWith(READY, [job], false);
  render(<JobsPane store={store} snapshot={READY} />);
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

it("says the estimate is not ready", () => {
  expect(remainingText(null, enMessages)).toBe("Estimating the time left");
});

it("does not count down the last minute", () => {
  expect(remainingText(59, enMessages)).toBe("Less than a minute left");
});

it("rounds the time left up to whole minutes", () => {
  expect(remainingText(60, enMessages)).toBe("About 1 min left");
  expect(remainingText(61, enMessages)).toBe("About 2 min left");
});

it("names what each output file is for", () => {
  expect(outputPurpose("/a.ttml", enMessages)).toBe("Styled subtitles (colour, outline, position)");
  expect(outputPurpose("/a.srt", enMessages)).toBe(
    "Subtitles with words and times (colours may not show; no outlines or positions)",
  );
  expect(outputPurpose("/a.fcpxml", enMessages)).toBe("Timeline with titles");
  expect(outputPurpose("/a.transcript.json", enMessages)).toBe("Transcript data for writing again");
});

it("shows the extension of a file it has no name for", () => {
  expect(outputPurpose("/a.b.xyz", enMessages)).toBe("xyz");
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
  render(<JobsPane store={storeWith(READY, [], false)} snapshot={READY} />);
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Drop videos or subtitle files here");
  expect(screen.queryByRole("alert")).toBe(null);
  expect(screen.getByRole("region", { name: "Drop videos or subtitle files here" }).className).toBe(
    "flex flex-col items-center gap-3 rounded-xl border-2 border-dashed p-10 text-center transition-colors border-border bg-card",
  );
});

it("changes the drop target while files hover", () => {
  render(<JobsPane store={storeWith(READY, [], true)} snapshot={READY} />);
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Release to add the files");
  expect(screen.getByRole("region", { name: "Drop videos or subtitle files here" }).className).toBe(
    "flex flex-col items-center gap-3 rounded-xl border-2 border-dashed p-10 text-center transition-colors border-primary bg-muted",
  );
});

it("names the missing roles", () => {
  const snapshot: Snapshot = { ...READY, models: [] };
  render(<JobsPane store={storeWith(snapshot, [], false)} snapshot={snapshot} />);
  expect(screen.getByRole("alert").textContent).toBe(
    "No model is available for: transcription and speaker separation. Download one in Models, or add a folder that already has it.",
  );
});

it("names the missing roles in a POSIX locale that is not a language tag", () => {
  const snapshot: Snapshot = { ...READY, locale: "C", models: [] };
  render(<JobsPane store={storeWith(snapshot, [], false)} snapshot={snapshot} />);
  expect(screen.getByRole("alert").textContent).toBe(
    "No model is available for: transcription and speaker separation. Download one in Models, or add a folder that already has it.",
  );
});

it("adds the chosen videos", async () => {
  vi.mocked(open).mockResolvedValue(["/a.mp4"]);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  await waitFor((): void => {
    expect(vi.mocked(store.dropFiles).mock.calls).toStrictEqual([[["/a.mp4"]]]);
  });
});

it("adds nothing when the file dialog is dismissed", async () => {
  vi.mocked(open).mockResolvedValue(null);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} />);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  });
  expect(vi.mocked(store.dropFiles).mock.calls).toStrictEqual([]);
});

it("reports a file dialog that fails", async () => {
  vi.mocked(open).mockRejectedValue(new Error("no dialog"));
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: no dialog" });
  });
});

it("draws no job list before the first job", () => {
  render(<JobsPane store={storeWith(READY, [], false)} snapshot={READY} />);
  expect(screen.queryByRole("list")).toBe(null);
});

it("shows a queued job that can be cancelled", async () => {
  showJob(QUEUED_JOB);
  expect(screen.getByRole("heading", { level: 2, name: "talk.mp4" }).textContent).toBe("talk.mp4");
  expect(screen.getByRole("status").textContent).toBe("Waiting in line");
  fireEvent.click(screen.getByRole("button", { name: "Cancel talk.mp4" }));
  expect(vi.mocked(invoke).mock.calls).toStrictEqual([["cancel_job", { id: 1 }]]);
});

it("reports a cancel that fails", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("too late"));
  const store: AppStore = showJob(QUEUED_JOB);
  fireEvent.click(screen.getByRole("button", { name: "Cancel talk.mp4" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: too late" });
  });
});

it("shows the step, stage percent, overall bar and time left of a measured stage", () => {
  showJob({
    ...QUEUED_JOB,
    state: "running",
    progress: {
      stage: "transcribe",
      overall: 0.42,
      stage_fraction: 0.7,
      determinate: true,
      plan: ["extract_audio", "transcribe", "export"],
      remaining_seconds: 125,
    },
  });
  expect(screen.getByRole("status").textContent).toBe("Step 2 of 3: Transcribing");
  expect(screen.getByText("70%").tagName).toBe("P");
  expect(screen.getByText("About 3 min left").tagName).toBe("P");
  const bar: HTMLElement = screen.getByRole("progressbar", { name: "Progress of talk.mp4" });
  expect(bar.getAttribute("aria-valuenow")).toBe("0.42");
  expect(bar.getAttribute("aria-valuetext")).toBe("42%");
});

it("says in words which steps are done, running and waiting", () => {
  showJob({
    ...QUEUED_JOB,
    state: "running",
    progress: {
      stage: "transcribe",
      overall: 0.42,
      stage_fraction: 0.7,
      determinate: true,
      plan: ["extract_audio", "transcribe", "export"],
      remaining_seconds: 125,
    },
  });
  expect(
    screen
      .getAllByRole("listitem")
      .slice(1)
      .map((item: HTMLElement): string | null => item.textContent),
  ).toStrictEqual(["Extracting audiodone", "Transcribingin progress", "Writing subtitle fileswaiting"]);
});

it("says so when a stage reports no percentage or estimate", () => {
  showJob({
    ...QUEUED_JOB,
    state: "running",
    progress: {
      stage: "diarize",
      overall: 0,
      stage_fraction: 0,
      determinate: false,
      plan: ["diarize"],
      remaining_seconds: null,
    },
  });
  expect(screen.getByText("this model reports no percentage").tagName).toBe("P");
  expect(screen.getByText("Estimating the time left").tagName).toBe("P");
});

it("shows a cancelled job with no action", () => {
  showJob({ ...QUEUED_JOB, state: "cancelled" });
  expect(screen.getByRole("status").textContent).toBe("Cancelled");
  expect(screen.queryByRole("button", { name: "Cancel talk.mp4" })).toBe(null);
});

it("shows the results of a finished job and can write it again", () => {
  showJob({
    ...QUEUED_JOB,
    state: "finished",
    outcome: {
      outputs: ["/a.srt", "/a.fcpxml"],
      base: "/videos/talk",
      speaker_count: 2,
      cue_count: 9,
      faceless: false,
      requested_speakers: 0,
    },
  });
  expect(screen.getByRole("status").textContent).toBe("Finished: 9 captions, 2 speakers");
  expect(
    screen
      .getAllByRole("listitem")
      .slice(1)
      .map((item: HTMLElement): string | null => item.textContent),
  ).toStrictEqual([
    "Subtitles with words and times (colours may not show; no outlines or positions): /a.srt",
    "Timeline with titles: /a.fcpxml",
  ]);
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
  showJob({
    ...QUEUED_JOB,
    state: "finished",
    outcome: {
      outputs: [],
      base: "/videos/talk",
      speaker_count: 1,
      cue_count: 1,
      faceless: true,
      requested_speakers: 0,
    },
  });
  expect(
    screen.getByText("Captions where no face was found were placed at the bottom of the video.").tagName,
  ).toBe("P");
});

it("reports a rewrite that fails", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("gone"));
  const store: AppStore = showJob({
    ...QUEUED_JOB,
    state: "finished",
    outcome: {
      outputs: [],
      base: "/videos/talk",
      speaker_count: 1,
      cue_count: 1,
      faceless: false,
      requested_speakers: 0,
    },
  });
  fireEvent.click(
    screen.getByRole("button", { name: "Write talk.mp4 again with the current position and colours" }),
  );
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: gone" });
  });
});

it("explains a failed job with the model's label", () => {
  showJob({ ...QUEUED_JOB, state: "failed", error: { code: "model_missing", model: "whisper" } });
  expect(screen.getByRole("alert").textContent).toBe("Whisper is not installed. Download it in Models.");
});

it("says subtitle files follow the selected profile", () => {
  expect(
    subtitleTargetText(
      {
        ...READY,
        settings: { ...SETTINGS, active_profile: "A" },
        profiles: [
          {
            name: "A",
            look: {
              palette: [],
              unassigned: { fill: "#AAAAAA", outline: "#000000" },
              font_family: "Sans",
              font_ratio: 0.05,
              outline_ratio: 0.1,
              rim_ratio: 0.03,
              line_height: 1.4,
              shadow: { enabled: false, strength: 0.5 },
              width_ratio: 0.8,
              placement: { anchor: "video_top", offset_x_percent: 0, offset_y_percent: 0 },
            },
          },
        ],
      },
      enMessages,
    ),
  ).toBe("Dropped subtitle files are written again as new files with the saved profile A.");
});

it("says subtitle files follow the current settings without a saved selection", () => {
  expect(
    subtitleTargetText({ ...READY, settings: { ...SETTINGS, active_profile: "Gone" } }, enMessages),
  ).toBe("Dropped subtitle files are written again as new files with the current caption settings.");
});

it("keeps the speaker count above the drop target and explains it", async () => {
  render(
    <main>
      <JobsPane store={storeWith(READY, [], false)} snapshot={READY} />
    </main>,
  );
  const count: HTMLElement = screen.getByRole("combobox", { name: "Number of speakers" });
  const target: HTMLElement = screen.getByRole("region", { name: "Drop videos or subtitle files here" });
  expect(count.compareDocumentPosition(target) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(4);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Why set the number of speakers?" }));
  });
  expect(screen.getByRole("dialog", { name: "Why set the number of speakers?" }).textContent).toBe(
    "Speaker colours depend on telling voices apart. Saying how many people speak keeps similar voices from merging and one voice from splitting. With 1 speaker, speaker separation is skipped, so it is faster and needs no speaker model.",
  );
});

it("refuses to choose videos without their models, saying what is missing", () => {
  const snapshot: Snapshot = { ...READY, models: [] };
  const store: AppStore = storeWith(snapshot, [], false);
  render(<JobsPane store={store} snapshot={snapshot} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  expect(vi.mocked(open).mock.calls).toStrictEqual([]);
  expect(failureOf(store)).toStrictEqual({ code: "models_missing", roles: ["transcribe", "diarize"] });
});

it("rewrites the chosen subtitle files", async () => {
  vi.mocked(open).mockResolvedValue(["/a.srt"]);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose subtitle files" }));
  await waitFor((): void => {
    expect(vi.mocked(store.dropFiles).mock.calls).toStrictEqual([[["/a.srt"]]]);
  });
  expect(vi.mocked(open).mock.calls[0]?.[0]).toStrictEqual({
    multiple: true,
    filters: [{ name: "Subtitle files", extensions: ["fcpxml", "srt", "ttml", "json"] }],
  });
});

it("rewrites nothing when the subtitle dialog is dismissed", async () => {
  vi.mocked(open).mockResolvedValue(null);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} />);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Choose subtitle files" }));
  });
  expect(vi.mocked(store.dropFiles).mock.calls).toStrictEqual([]);
});

it("reports a subtitle dialog that fails", async () => {
  vi.mocked(open).mockRejectedValue(new Error("no dialog"));
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose subtitle files" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: no dialog" });
  });
});

it("saves the chosen number of speakers", async () => {
  const store: AppStore = storeWith(READY, [], false);
  render(
    <main>
      <JobsPane store={store} snapshot={READY} />
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Number of speakers" }));
  });
  await act(async (): Promise<void> => {
    fireEvent.pointerDown(screen.getByRole("option", { name: "3 speakers" }));
    fireEvent.click(screen.getByRole("option", { name: "3 speakers" }));
  });
  expect(vi.mocked(store.updateSettings).mock.calls[0]?.[0].speaker_count).toBe(3);
});

it("says nothing about speed on a Mac or with a usable GPU", () => {
  expect(slowProcessingNotice(READY, enMessages)).toBe(null);
  expect(
    slowProcessingNotice(
      {
        ...READY,
        backend: "vulkan",
        hardware: { is_mac: false, memory_gib: 8, gpu: { vendor: "radeon", name: "RX", memory_gib: 16 } },
      },
      enMessages,
    ),
  ).toBe(null);
});

it("warns when speakers run on the CPU until the GeForce runtime is here", () => {
  const snapshot: Snapshot = {
    ...READY,
    backend: "speakers_on_cpu",
    hardware: { is_mac: false, memory_gib: 8, gpu: { vendor: "nvidia", name: "RTX", memory_gib: 16 } },
  };
  render(<JobsPane store={storeWith(snapshot, [], false)} snapshot={snapshot} />);
  expect(
    screen.getByText(
      "Telling speakers apart runs on the CPU until you download the GeForce runtime in Models, so it is slower.",
    ).tagName,
  ).toBe("P");
});

it("warns when no usable GPU was found", () => {
  expect(
    slowProcessingNotice({ ...READY, hardware: { is_mac: false, memory_gib: 8, gpu: null } }, enMessages),
  ).toBe("No GPU this version can use was found, so everything runs on the CPU. It is slower.");
});
