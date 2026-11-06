// Runs with bun: the drop target, readiness note and every job state.
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, expect, it, type Mock, vi } from "vitest";
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
  speakerCountOptions,
  subtitleFormatOptions,
  subtitleTargetText,
} from "./JobsPane.tsx";

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
  speaker_count: 1,
  subtitle_format: "fcpxml",
  ui_language: "",
  ui_theme: "system",
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
  id: "nemotron-3.5-transcribe-iq4",
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
  render(<JobsPane store={store} snapshot={READY} onOpenModels={vi.fn()} />);
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
  ).toStrictEqual([]);
});

it("labels a model by id, and falls back to the id or nothing", () => {
  expect(modelLabel(READY, "whisper")).toBe("Whisper");
  expect(modelLabel(READY, "unknown")).toBe("unknown");
  expect(modelLabel(READY, undefined)).toBe("");
});

it("invites a drop and hides the readiness note when ready", () => {
  render(<JobsPane store={storeWith(READY, [], false)} snapshot={READY} onOpenModels={vi.fn()} />);
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Drop videos or subtitle files here");
  expect(screen.queryByRole("alert")).toBe(null);
  expect(screen.getByRole("region", { name: "Drop videos or subtitle files here" }).className).toBe(
    "flex flex-col items-center gap-3 rounded-xl border-2 border-dashed p-10 text-center transition-colors border-border bg-card",
  );
});

it("changes the drop target while files hover", () => {
  render(<JobsPane store={storeWith(READY, [], true)} snapshot={READY} onOpenModels={vi.fn()} />);
  expect(screen.getByRole("heading", { level: 2 }).textContent).toBe("Release to add the files");
  expect(screen.getByRole("region", { name: "Drop videos or subtitle files here" }).className).toBe(
    "flex flex-col items-center gap-3 rounded-xl border-2 border-dashed p-10 text-center transition-colors border-primary bg-muted",
  );
});

it("names the missing roles", () => {
  const snapshot: Snapshot = { ...READY, models: [] };
  render(<JobsPane store={storeWith(snapshot, [], false)} snapshot={snapshot} onOpenModels={vi.fn()} />);
  expect(screen.getByRole("alert").textContent).toBe(
    "No model is available for: transcription. Download one in Models, or add a folder that already has it.",
  );
});

it("names the missing roles in a POSIX locale that is not a language tag", () => {
  const snapshot: Snapshot = { ...READY, locale: "C", models: [] };
  render(<JobsPane store={storeWith(snapshot, [], false)} snapshot={snapshot} onOpenModels={vi.fn()} />);
  expect(screen.getByRole("alert").textContent).toBe(
    "No model is available for: transcription. Download one in Models, or add a folder that already has it.",
  );
});

it("adds the chosen videos", async () => {
  vi.mocked(open).mockResolvedValue(["/a.mp4"]);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} onOpenModels={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  await waitFor((): void => {
    expect(vi.mocked(store.dropFiles).mock.calls).toStrictEqual([[["/a.mp4"]]]);
  });
});

it("adds nothing when the file dialog is dismissed", async () => {
  vi.mocked(open).mockResolvedValue(null);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} onOpenModels={vi.fn()} />);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  });
  expect(vi.mocked(store.dropFiles).mock.calls).toStrictEqual([]);
});

it("reports a file dialog that fails", async () => {
  vi.mocked(open).mockRejectedValue(new Error("no dialog"));
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} onOpenModels={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: no dialog" });
  });
});

it("draws no job list before the first job", () => {
  render(<JobsPane store={storeWith(READY, [], false)} snapshot={READY} onOpenModels={vi.fn()} />);
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

it("warns when the speaker model heard nobody", () => {
  showJob({
    ...QUEUED_JOB,
    state: "finished",
    outcome: {
      outputs: ["/a.fcpxml"],
      base: "/videos/talk",
      speaker_count: 1,
      cue_count: 9,
      faceless: false,
      font_missing: false,
      speakers_missing: true,
      requested_speakers: 0,
    },
  });
  expect(
    screen.getByText("The speaker model heard nobody in this video, so every caption has one colour.")
      .tagName,
  ).toBe("P");
});

it("warns when the chosen font was missing", () => {
  showJob({
    ...QUEUED_JOB,
    state: "finished",
    outcome: {
      outputs: ["/a.fcpxml"],
      base: "/videos/talk",
      speaker_count: 1,
      cue_count: 9,
      faceless: false,
      font_missing: true,
      speakers_missing: false,
      requested_speakers: 0,
    },
  });
  expect(
    screen.getByText("The chosen font is not on this computer, so the caption widths are an estimate.")
      .tagName,
  ).toBe("P");
});

it("says speaker, not speakers, when a finished job found one", () => {
  showJob({
    ...QUEUED_JOB,
    state: "finished",
    outcome: {
      outputs: ["/a.srt"],
      base: "/videos/talk",
      speaker_count: 1,
      cue_count: 9,
      faceless: false,
      font_missing: false,
      speakers_missing: false,
      requested_speakers: 0,
    },
  });
  expect(screen.getByRole("status").textContent).toBe("Finished: 9 captions, 1 speaker");
});

it("says each speaker count's load in the option itself", () => {
  expect(speakerCountOptions(enMessages).slice(0, 3)).toStrictEqual([
    { value: "0", label: "Detect automatically (Heavy)", disabled: false },
    { value: "1", label: "1 speaker (Light)", disabled: false },
    { value: "2", label: "2 speakers (Heavy)", disabled: false },
  ]);
  expect(speakerCountOptions(enMessages).length).toBe(9);
});

function showSpeakers(speakers: number, models: ModelStatus[]): { store: AppStore; open: Mock<() => void> } {
  const snapshot: Snapshot = { ...READY, models, settings: { ...READY.settings, speaker_count: speakers } };
  const store: AppStore = storeWith(snapshot, [], false);
  const open: Mock<() => void> = vi.fn();
  render(
    <main>
      <JobsPane store={store} snapshot={snapshot} onOpenModels={open} />
    </main>,
  );
  return { store, open };
}

const SEPARATOR: ModelStatus = {
  ...WHISPER,
  id: "mossformer2-ss-16k",
  role: "separate",
  label: "MossFormer2",
};
const HEAVY_TEXT: string =
  "Heavy: Runs Nemotron, then MossFormer2, then Whisper. It takes much longer and uses much more memory.";

const loadText = (container: Element): string | null | undefined =>
  container.querySelector('[data-slot="speaker-load"]')?.textContent;

it("says one speaker is light, in words and not only by its icon", () => {
  const snapshot: Snapshot = { ...READY, settings: { ...READY.settings, speaker_count: 1 } };
  const { container } = render(
    <JobsPane store={storeWith(snapshot, [], false)} snapshot={snapshot} onOpenModels={vi.fn()} />,
  );
  expect(loadText(container)).toBe(
    "Light: Uses Whisper only, with no voice separation or speaker separation.",
  );
});

it("says automatic detection and more speakers are heavy", () => {
  showSpeakers(0, [WHISPER, DIARIZER, SEPARATOR]);
  expect(loadText(document.body)).toBe(HEAVY_TEXT);
  cleanup();
  showSpeakers(3, [WHISPER, DIARIZER, SEPARATOR]);
  expect(loadText(document.body)).toBe(HEAVY_TEXT);
  expect(screen.getByRole("combobox", { name: "Number of speakers" }).textContent).toBe("3 speakers (Heavy)");
});

it("gives the measured Mac figures for a heavy choice, and none for one speaker", () => {
  const estimate: string =
    "Measured on a Mac: for a 4-hour video, voice separation alone takes about 2 h 20 min, and Whisper's time comes on top. Memory: up to about 8 GB for voice separation and about 22 GB for speaker separation (Nemotron). It varies by video and computer. Windows has not been measured.";
  showSpeakers(2, [WHISPER, DIARIZER, SEPARATOR]);
  expect(screen.getByText(estimate).tagName).toBe("P");
  cleanup();
  showSpeakers(1, [WHISPER, DIARIZER, SEPARATOR]);
  expect(screen.queryByText(estimate)).toBe(null);
});

it("asks for nothing when a heavy choice has every model", () => {
  showSpeakers(2, [WHISPER, DIARIZER, SEPARATOR]);
  expect(screen.queryByRole("alert")).toBe(null);
  expect(screen.queryByRole("button", { name: "Run with 1 speaker" })).toBe(null);
});

it("needs no speaker models for one speaker", () => {
  showSpeakers(1, [WHISPER]);
  expect(screen.queryByRole("alert")).toBe(null);
});

it("names what a heavy choice lacks, with a way to the models", () => {
  const { open } = showSpeakers(2, [WHISPER, DIARIZER]);
  expect(screen.getByRole("alert").textContent).toBe(
    "This number of speakers needs models that are missing: voice separation.",
  );
  fireEvent.click(screen.getByRole("button", { name: "Open the “Models” tab" }));
  expect(open).toHaveBeenCalledTimes(1);
});

it("lists both speaker models when both are missing", () => {
  showSpeakers(0, [WHISPER]);
  expect(screen.getByRole("alert").textContent).toBe(
    "This number of speakers needs models that are missing: speaker separation and voice separation.",
  );
});

it("runs for one speaker only when the person says so", () => {
  const { store } = showSpeakers(3, [WHISPER]);
  expect(store.updateSettings).toHaveBeenCalledTimes(0);
  fireEvent.click(screen.getByRole("button", { name: "Run with 1 speaker" }));
  expect(vi.mocked(store.updateSettings).mock.lastCall?.[0].speaker_count).toBe(1);
});

it("keeps a saved speaker count, whatever models there are", () => {
  showSpeakers(3, [WHISPER]);
  expect(screen.getByRole("combobox", { name: "Number of speakers" }).textContent).toBe("3 speakers (Heavy)");
});

it("offers the three subtitle formats by their names", () => {
  expect(subtitleFormatOptions()).toStrictEqual([
    { value: "drt", label: "DRT", disabled: false },
    { value: "fcpxml", label: "FCPXML", disabled: false },
    { value: "ttml", label: "TTML", disabled: false },
    { value: "srt", label: "SRT", disabled: false },
  ]);
});

it("says what the chosen subtitle format is for", () => {
  const srt: Snapshot = { ...READY, settings: { ...READY.settings, subtitle_format: "srt" } };
  render(<JobsPane store={storeWith(srt, [], false)} snapshot={srt} onOpenModels={vi.fn()} />);
  expect(screen.getByRole("combobox", { name: "Subtitle file format" }).textContent).toBe("SRT");
  expect(
    screen.getByText("Subtitles with words and times (colours may not show; no outlines or positions)")
      .tagName,
  ).toBe("P");
});

it("says what a format other than DRT cannot show, and nothing for DRT", () => {
  const ttml: Snapshot = { ...READY, settings: { ...READY.settings, subtitle_format: "ttml" } };
  const { unmount } = render(
    <JobsPane store={storeWith(ttml, [], false)} snapshot={ttml} onOpenModels={vi.fn()} />,
  );
  expect(screen.getByText("TTML shows no outer edge or shadow. Choose DRT to use them.").tagName).toBe("P");
  unmount();
  const drt: Snapshot = { ...READY, settings: { ...READY.settings, subtitle_format: "drt" } };
  render(<JobsPane store={storeWith(drt, [], false)} snapshot={drt} onOpenModels={vi.fn()} />);
  expect(screen.queryByText("TTML shows no outer edge or shadow. Choose DRT to use them.")).toBe(null);
  expect(
    screen.getByText("DaVinci Resolve timeline of Text+ clips: colours, outlines, outer edges and shadows")
      .tagName,
  ).toBe("P");
});

it("saves the chosen subtitle format", async () => {
  const store: AppStore = storeWith(READY, [], false);
  render(
    <main>
      <JobsPane store={store} snapshot={READY} onOpenModels={vi.fn()} />
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Subtitle file format" }));
  });
  await act(async (): Promise<void> => {
    fireEvent.pointerDown(screen.getByRole("option", { name: "TTML" }));
    fireEvent.click(screen.getByRole("option", { name: "TTML" }));
  });
  expect(vi.mocked(store.updateSettings).mock.calls.at(-1)?.[0].subtitle_format).toBe("ttml");
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
      font_missing: false,
      speakers_missing: false,
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
      font_missing: false,
      speakers_missing: false,
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
      font_missing: false,
      speakers_missing: false,
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
              font_family: "Sans",
              font_ratio: 0.05,
              line_height: 1.4,
              width_ratio: 0.8,
              safe_left_percent: 5,
              safe_right_percent: 5,
              placement: {
                anchor: "video_top",
                offset_x_percent: 0,
                offset_y_percent: 0,
              },
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
      <JobsPane store={storeWith(READY, [], false)} snapshot={READY} onOpenModels={vi.fn()} />
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
  render(<JobsPane store={store} snapshot={snapshot} onOpenModels={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose videos" }));
  expect(vi.mocked(open).mock.calls).toStrictEqual([]);
  expect(failureOf(store)).toStrictEqual({ code: "models_missing", roles: ["transcribe"] });
});

it("rewrites the chosen subtitle files", async () => {
  vi.mocked(open).mockResolvedValue(["/a.srt"]);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} onOpenModels={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose subtitle files" }));
  await waitFor((): void => {
    expect(vi.mocked(store.dropFiles).mock.calls).toStrictEqual([[["/a.srt"]]]);
  });
  expect(vi.mocked(open).mock.calls[0]?.[0]).toStrictEqual({
    multiple: true,
    filters: [{ name: "Subtitle files", extensions: ["drt", "fcpxml", "srt", "ttml", "json"] }],
  });
});

it("rewrites nothing when the subtitle dialog is dismissed", async () => {
  vi.mocked(open).mockResolvedValue(null);
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} onOpenModels={vi.fn()} />);
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("button", { name: "Choose subtitle files" }));
  });
  expect(vi.mocked(store.dropFiles).mock.calls).toStrictEqual([]);
});

it("reports a subtitle dialog that fails", async () => {
  vi.mocked(open).mockRejectedValue(new Error("no dialog"));
  const store: AppStore = storeWith(READY, [], false);
  render(<JobsPane store={store} snapshot={READY} onOpenModels={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Choose subtitle files" }));
  await waitFor((): void => {
    expect(failureOf(store)).toStrictEqual({ code: "internal", detail: "Error: no dialog" });
  });
});

it("saves the chosen number of speakers", async () => {
  const store: AppStore = storeWith(READY, [], false);
  render(
    <main>
      <JobsPane store={store} snapshot={READY} onOpenModels={vi.fn()} />
    </main>,
  );
  await act(async (): Promise<void> => {
    fireEvent.click(screen.getByRole("combobox", { name: "Number of speakers" }));
  });
  await act(async (): Promise<void> => {
    fireEvent.pointerDown(screen.getByRole("option", { name: "3 speakers (Heavy)" }));
    fireEvent.click(screen.getByRole("option", { name: "3 speakers (Heavy)" }));
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
  render(<JobsPane store={storeWith(snapshot, [], false)} snapshot={snapshot} onOpenModels={vi.fn()} />);
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
