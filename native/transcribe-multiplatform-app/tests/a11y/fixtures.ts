// Runs with bun: every UI state, generated from the types so a new stage or error code joins the sweep.
import type {
  BackendError,
  DownloadEvent,
  JobEvent,
  ModelStatus,
  Profile,
  Snapshot,
  SpeakerColor,
  Stage,
  UpdateInfo,
} from "../../src/backend/types.ts";
import { enMessages } from "../../src/i18n/locale-en.ts";
import type { ErrorMessages } from "../../src/i18n/messages.ts";

export type Tab = "jobs" | "style" | "models" | "help" | "settings";
export type StateStep =
  | "open-face-select"
  | "open-language-select"
  | "open-subtitle-format"
  | "open-speaker-count"
  | "hover-drop"
  | "open-speaker-help"
  | "press-update-check"
  | "press-update-install"
  | "open-settings-language"
  | "open-second-speaker"
  | "open-color-picker"
  | "open-font-list"
  | "open-reset-dialog"
  | "open-overwrite-dialog"
  | "open-delete-dialog"
  | "open-reset-look-dialog"
  | "open-reset-look-profile-dialog"
  | "drop-video"
  | "drop-subtitles"
  | "drop-other";

/** What the mocked backend answers about updates. */
export interface UpdaterFixture {
  available: UpdateInfo | null;
  checkError: BackendError | null;
  installError: BackendError | null;
}

export interface UiState {
  name: string;
  tab: Tab;
  snapshot: Snapshot;
  jobEvents: JobEvent[];
  downloadEvents: DownloadEvent[];
  anchor: string | null;
  frame: string | null;
  steps: StateStep[];
  updater: UpdaterFixture;
}

const STAGES: Record<Stage, true> = {
  convert_model: true,
  extract_audio: true,
  transcribe: true,
  diarize: true,
  separate: true,
  retranscribe: true,
  faces: true,
  export: true,
};
const ALL_STAGES: Stage[] = ["convert_model", "extract_audio", "transcribe", "diarize", "faces", "export"];
const REMAINING_SECONDS: (number | null)[] = [null, 30, 900];
const ANCHORS: string[] = ["face_above", "face_below", "video_top", "video_bottom"];
const FRAMES: string[] = ["landscape", "vertical", "uhd"];
const LONG_PATH: string =
  "/Volumes/素材/とても長いフォルダ名/2026-11-04 配信の切り抜き 第12回 ゲスト回/recording.mp4";
const FIRST_FILL: string[] = [
  "#FFFFFF",
  "#005A9C",
  "#6A1FA8",
  "#0B672C",
  "#A3123A",
  "#8A4500",
  "#7A1D7A",
  "#006262",
];

const model = (
  id: string,
  role: ModelStatus["role"],
  label: string,
  changes: Partial<ModelStatus>,
): ModelStatus => ({
  id,
  role,
  label,
  source_url: `https://huggingface.co/${id}`,
  download_bytes: 1_700_000_000,
  found_at: null,
  blocked: null,
  recommended: false,
  ...changes,
});

export const ALL_MODELS: ModelStatus[] = [
  model("whisper-large-v3-turbo", "transcribe", "Whisper large-v3-turbo", {
    found_at: "/data/models/ggml.bin",
    recommended: true,
  }),
  model("nemotron-3.5-transcribe-iq4", "diarize", "Nemotron 3.5 Transcribe 30B-A4B (IQ4_NL)", {
    recommended: true,
  }),
  model("nemotron-3.5-transcribe-nvfp4", "diarize", "Nemotron 3.5 Transcribe 30B-A4B (NVFP4)", {
    blocked: { code: "needs_nvidia", model: "nemotron-3.5-transcribe-nvfp4" },
  }),
  model("nemotron-3.5-transcribe-q8", "diarize", "Nemotron 3.5 Transcribe 30B-A4B (Q8_0)", {
    blocked: { code: "needs_memory", model: "nemotron-3.5-transcribe-q8", required: 48, available: 12 },
  }),
  model("mossformer2-ss-16k", "separate", "MossFormer2 SS 16K", {
    download_bytes: 230_156_008,
    recommended: true,
  }),
  model("yunet", "face", "YuNet", { found_at: "/app/yunet.onnx", download_bytes: 0, recommended: true }),
  model("avatar-face-yolo", "face", "Avatar face YOLO", { download_bytes: 0, source_url: "" }),
];
/** Only the GeForce distribution, on a machine with a GeForce, lists its runtime. */
const GEFORCE_RUNTIME: ModelStatus = model("geforce-runtime", "runtime", "GeForce CUDA runtime", {});

export const snapshot = (changes: Partial<Snapshot>): Snapshot => ({
  version: "1.2.3",
  locale: "en",
  backend: "default",
  hardware: { is_mac: true, memory_gib: 48, gpu: null },
  models: ALL_MODELS,
  models_dir: "/data/models",
  settings: {
    palette: FIRST_FILL.map(
      (fill: string, index: number): SpeakerColor =>
        index === 0
          ? {
              fill,
              fill_width: 0,
              outline: "#000000",
              outline_width: 0.115,
              rim: null,
              rim_width: 3 / 88,
              shadow: null,
              shadow_x: 0.03,
              shadow_y: -0.03,
              shadow_blur: 5,
            }
          : {
              fill,
              fill_width: 0,
              outline: "#FFFFFF",
              outline_width: 0.115,
              rim: null,
              rim_width: 3 / 88,
              shadow: null,
              shadow_x: 0.03,
              shadow_y: -0.03,
              shadow_blur: 5,
            },
    ),
    font_family: "Noto Sans JP",
    font_ratio: 0.048,
    line_height: 1.448,
    width_ratio: 0.9,
    safe_left_percent: 5,
    safe_right_percent: 5,
    placement: {
      anchor: "video_bottom",
      offset_x_percent: 0,
      offset_y_percent: 0,
    },
    language: "auto",
    diarize_model: "nemotron-3.5-transcribe-iq4",
    speaker_count: 1,
    subtitle_format: "drt",
    ui_language: "",
    ui_theme: "system",
    face_model: "yunet",
    model_dirs: ["/user/models"],
    active_profile: "",
  },
  profiles: [],
  ...changes,
});

const profile = (name: string): Profile => ({
  name,
  look: {
    palette: snapshot({}).settings.palette,
    font_family: "Noto Sans JP",
    font_ratio: 0.048,
    line_height: 1.448,
    width_ratio: 0.9,
    safe_left_percent: 5,
    safe_right_percent: 5,
    placement: {
      anchor: "video_bottom",
      offset_x_percent: 0,
      offset_y_percent: 0,
    },
  },
});

export const WITH_PROFILES: Snapshot = snapshot({
  profiles: [
    profile("配信用 16:9 白フチ"),
    profile("A long profile name for vertical shorts and interviews"),
  ],
  settings: { ...snapshot({}).settings, active_profile: "配信用 16:9 白フチ" },
});

const NO_UPDATE: UpdaterFixture = { available: null, checkError: null, installError: null };
const NEW_VERSION: UpdateInfo = {
  version: "9.9.9",
  notes:
    "Faster captions.\nA long line of notes from the release, to see that it wraps the way the rest of the window does and stays readable.",
};
const OFFERED: UpdaterFixture = { ...NO_UPDATE, available: NEW_VERSION };
const UPDATE_FAILURE: BackendError = { code: "internal", detail: "The server did not answer" };

const state = (name: string, tab: Tab, changes: Partial<UiState>): UiState => ({
  name,
  tab,
  snapshot: snapshot({}),
  jobEvents: [],
  downloadEvents: [],
  anchor: null,
  frame: null,
  steps: [],
  updater: NO_UPDATE,
  ...changes,
});

const queued = (id: number): JobEvent => ({ state: "queued", id, path: LONG_PATH });

const errorsByCode = (): BackendError[] =>
  Object.keys(enMessages.errors satisfies ErrorMessages).map(
    (code: string): BackendError => ({
      code,
      detail: "ffmpeg failed: exit status 1\nInvalid data found when processing input",
      model: "nemotron-3.5-transcribe-iq4",
      role: "diarize",
      required: 24,
      available: 12,
      limit_minutes: 60,
      file: "recording.fcpxml",
      name: "配信用",
      roles: ["transcribe", "diarize"],
    }),
  );

const stageStates = (): UiState[] =>
  Object.keys(STAGES).flatMap((key: string): UiState[] => {
    const stage: Stage = narrowStage(key);
    return [true, false].flatMap((determinate: boolean): UiState[] =>
      REMAINING_SECONDS.map(
        (remaining: number | null): UiState =>
          state(
            `job running ${stage} ${determinate ? "with percent" : "without percent"} remaining ${remaining}`,
            "jobs",
            {
              jobEvents: [
                queued(1),
                {
                  state: "running",
                  id: 1,
                  progress: {
                    stage,
                    overall: 0.42,
                    stage_fraction: 0.5,
                    determinate,
                    plan: ALL_STAGES,
                    remaining_seconds: remaining,
                  },
                },
              ],
            },
          ),
      ),
    );
  });

const STAGE_SET: ReadonlySet<string> = new Set(Object.keys(STAGES));

function narrowStage(key: string): Stage {
  if (!isStage(key)) {
    throw new Error(`not a stage: ${key}`);
  }
  return key;
}

function isStage(key: string): key is Stage {
  return STAGE_SET.has(key);
}

export const UI_STATES: UiState[] = [
  state("jobs empty", "jobs", {}),
  state("jobs drop hover", "jobs", { steps: ["hover-drop"] }),
  state("jobs on a geforce without its runtime", "jobs", {
    snapshot: snapshot({
      backend: "speakers_on_cpu",
      hardware: {
        is_mac: false,
        memory_gib: 64,
        gpu: { vendor: "nvidia", name: "NVIDIA GeForce RTX 5090", memory_gib: 32 },
      },
    }),
  }),
  state("jobs with no usable gpu", "jobs", {
    snapshot: snapshot({ hardware: { is_mac: false, memory_gib: 12, gpu: null } }),
  }),
  state("jobs speaker help open", "jobs", { steps: ["open-speaker-help"] }),
  state("jobs with a selected profile", "jobs", { snapshot: WITH_PROFILES }),
  state("jobs subtitle format fcpxml", "jobs", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, subtitle_format: "fcpxml" } }),
  }),
  state("jobs subtitle format ttml", "jobs", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, subtitle_format: "ttml" } }),
  }),
  state("jobs subtitle format srt", "jobs", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, subtitle_format: "srt" } }),
  }),
  state("jobs subtitle format list open", "jobs", { steps: ["open-subtitle-format"] }),
  state("jobs heavy with every model", "jobs", {
    snapshot: snapshot({
      settings: { ...snapshot({}).settings, speaker_count: 3 },
      models: ALL_MODELS.map(
        (row: ModelStatus): ModelStatus =>
          row.id === "nemotron-3.5-transcribe-iq4" || row.id === "mossformer2-ss-16k"
            ? { ...row, found_at: "/data/models/found" }
            : row,
      ),
    }),
  }),
  state("jobs heavy missing mossformer2", "jobs", {
    snapshot: snapshot({
      settings: { ...snapshot({}).settings, speaker_count: 2 },
      models: ALL_MODELS.map(
        (row: ModelStatus): ModelStatus =>
          row.id === "nemotron-3.5-transcribe-iq4" ? { ...row, found_at: "/data/models/iq4.gguf" } : row,
      ),
    }),
  }),
  state("jobs automatic missing both speaker models", "jobs", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, speaker_count: 0 } }),
  }),
  state("jobs speaker count list open", "jobs", { steps: ["open-speaker-count"] }),
  state("jobs subtitle file dropped", "jobs", { steps: ["drop-subtitles"] }),
  state("jobs unsupported file dropped", "jobs", { steps: ["drop-other"] }),
  state("jobs video dropped without models", "jobs", {
    snapshot: snapshot({
      settings: { ...snapshot({}).settings, speaker_count: 0 },
      models: ALL_MODELS.map((row: ModelStatus): ModelStatus => ({ ...row, found_at: null })),
    }),
    steps: ["drop-video"],
  }),
  state("jobs missing models", "jobs", {
    snapshot: snapshot({
      settings: { ...snapshot({}).settings, speaker_count: 0 },
      models: ALL_MODELS.map((row: ModelStatus): ModelStatus => ({ ...row, found_at: null })),
    }),
  }),
  state("job queued", "jobs", { jobEvents: [queued(1)] }),
  ...stageStates(),
  state("job finished", "jobs", {
    jobEvents: [
      queued(1),
      {
        state: "finished",
        id: 1,
        outcome: {
          outputs: [`${LONG_PATH}.ttml`, `${LONG_PATH}.srt`],
          base: LONG_PATH,
          speaker_count: 3,
          requested_speakers: 0,
          cue_count: 120,
          faceless: false,
          font_missing: false,
          speakers_missing: false,
        },
      },
    ],
  }),
  state("job finished with fewer speakers than requested", "jobs", {
    jobEvents: [
      queued(1),
      {
        state: "finished",
        id: 1,
        outcome: {
          outputs: [`${LONG_PATH}.ttml`],
          base: LONG_PATH,
          speaker_count: 2,
          requested_speakers: 4,
          cue_count: 20,
          faceless: false,
          font_missing: false,
          speakers_missing: false,
        },
      },
    ],
  }),
  state("jobs with one speaker and no speaker model", "jobs", {
    snapshot: snapshot({
      models: ALL_MODELS.map(
        (row: ModelStatus): ModelStatus => (row.role === "diarize" ? { ...row, found_at: null } : row),
      ),
    }),
  }),
  state("job finished with a missing font", "jobs", {
    jobEvents: [
      queued(1),
      {
        state: "finished",
        id: 1,
        outcome: {
          outputs: [`${LONG_PATH}.fcpxml`],
          base: LONG_PATH,
          speaker_count: 1,
          requested_speakers: 0,
          cue_count: 20,
          faceless: false,
          font_missing: true,
          speakers_missing: false,
        },
      },
    ],
  }),
  state("job finished without faces", "jobs", {
    jobEvents: [
      queued(1),
      {
        state: "finished",
        id: 1,
        outcome: {
          outputs: [`${LONG_PATH}.ttml`],
          base: LONG_PATH,
          speaker_count: 1,
          requested_speakers: 0,
          cue_count: 2,
          faceless: true,
          font_missing: false,
          speakers_missing: false,
        },
      },
    ],
  }),
  state("job failed too long", "jobs", {
    jobEvents: [
      queued(1),
      {
        state: "failed",
        id: 1,
        error: { code: "too_long_for_model", model: "nemotron-3.5-transcribe-iq4", limit_minutes: 60 },
      },
    ],
  }),
  state("job cancelled", "jobs", { jobEvents: [queued(1), { state: "cancelled", id: 1 }] }),
  ...errorsByCode().map(
    (error: BackendError): UiState =>
      state(`job failed ${error.code}`, "jobs", {
        jobEvents: [queued(1), { state: "failed", id: 1, error }],
      }),
  ),
  ...ANCHORS.flatMap((anchor: string): UiState[] =>
    FRAMES.map((frame: string): UiState => state(`style ${anchor} ${frame}`, "style", { anchor, frame })),
  ),
  state("style with every speaker's outer edge on and thick layers", "style", {
    snapshot: snapshot({
      settings: {
        ...snapshot({}).settings,
        palette: snapshot({}).settings.palette.map(
          (color: SpeakerColor): SpeakerColor => ({
            ...color,
            fill_width: 0.02,
            outline_width: 0.2,
            rim: "#000000",
            rim_width: 0.1,
            shadow: null,
            shadow_x: 0.03,
            shadow_y: -0.03,
            shadow_blur: 5,
          }),
        ),
      },
    }),
  }),
  state("style with every speaker's shadow on", "style", {
    snapshot: snapshot({
      settings: {
        ...snapshot({}).settings,
        palette: snapshot({}).settings.palette.map(
          (color: SpeakerColor): SpeakerColor => ({
            ...color,
            shadow: "#112233",
            shadow_y: -0.2,
            shadow_blur: 50,
          }),
        ),
      },
    }),
  }),
  state("style for the srt format", "style", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, subtitle_format: "srt" } }),
  }),
  state("style second speaker open", "style", { steps: ["open-second-speaker"] }),
  state("style colour picker open", "style", { steps: ["open-color-picker"] }),
  state("style colour picker open on a transparent edge", "style", {
    steps: ["open-second-speaker", "open-color-picker"],
  }),
  state("style with wide safe zones", "style", {
    snapshot: snapshot({
      settings: {
        ...snapshot({}).settings,
        safe_left_percent: 30,
        safe_right_percent: 10,
      },
    }),
  }),
  state("style with the widest safe zones", "style", {
    snapshot: snapshot({
      settings: {
        ...snapshot({}).settings,
        safe_left_percent: 40,
        safe_right_percent: 40,
      },
    }),
  }),
  state("style without safe zones", "style", {
    snapshot: snapshot({
      settings: {
        ...snapshot({}).settings,
        safe_left_percent: 0,
        safe_right_percent: 0,
      },
    }),
  }),
  state("style with low contrast colours", "style", {
    snapshot: snapshot({
      settings: {
        ...snapshot({}).settings,
        palette: [
          {
            fill: "#C48CFF",
            fill_width: 0,
            outline: "#FFFFFF",
            outline_width: 0.1,
            rim: null,
            rim_width: 0.034,
            shadow: null,
            shadow_x: 0.03,
            shadow_y: -0.03,
            shadow_blur: 5,
          },
          ...snapshot({}).settings.palette.slice(1),
        ],
      },
    }),
  }),
  state("style with profiles", "style", { snapshot: WITH_PROFILES }),
  state("style with an unsaved change to the profile", "style", {
    snapshot: {
      ...WITH_PROFILES,
      settings: { ...WITH_PROFILES.settings, font_ratio: 0.06 },
    },
  }),
  state("style overwrite profile dialog", "style", {
    snapshot: WITH_PROFILES,
    steps: ["open-overwrite-dialog"],
  }),
  state("style delete profile dialog", "style", { snapshot: WITH_PROFILES, steps: ["open-delete-dialog"] }),
  state("style reset look dialog", "style", { steps: ["open-reset-look-dialog"] }),
  state("style reset look dialog with a profile", "style", {
    snapshot: WITH_PROFILES,
    steps: ["open-reset-look-profile-dialog"],
  }),
  state("style font list open", "style", { steps: ["open-font-list"] }),
  state("style font not installed", "style", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, font_family: "Missing Font Family" } }),
    steps: ["open-font-list"],
  }),
  state("style face select open", "style", { steps: ["open-face-select"] }),
  state("style language select open", "style", { steps: ["open-language-select"] }),
  state("help", "help", {}),
  state("settings", "settings", {}),
  state("settings language list open", "settings", { steps: ["open-settings-language"] }),
  state("settings dark on a light system", "settings", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, ui_theme: "dark" } }),
  }),
  state("settings light on a dark system", "settings", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, ui_theme: "light" } }),
  }),
  state("settings with the language overridden", "settings", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, ui_language: "de" } }),
  }),
  state("style in the dark setting", "style", {
    snapshot: snapshot({ settings: { ...snapshot({}).settings, ui_theme: "dark" } }),
  }),
  state("jobs update offered", "jobs", { updater: OFFERED }),
  state("jobs update offered without notes", "jobs", {
    updater: { ...OFFERED, available: { version: "9.9.9", notes: null } },
  }),
  state("jobs update offered while a video runs", "jobs", { updater: OFFERED, jobEvents: [queued(1)] }),
  state("jobs update installing", "jobs", { updater: OFFERED, steps: ["press-update-install"] }),
  state("jobs update failed", "jobs", {
    updater: { ...OFFERED, installError: UPDATE_FAILURE },
    steps: ["press-update-install"],
  }),
  state("settings up to date", "settings", { steps: ["press-update-check"] }),
  state("settings update found", "settings", { updater: OFFERED, steps: ["press-update-check"] }),
  state("settings update check failed", "settings", {
    updater: { ...NO_UPDATE, checkError: UPDATE_FAILURE },
    steps: ["press-update-check"],
  }),
  state("models idle", "models", {}),
  state("models on geforce without its runtime", "models", {
    snapshot: snapshot({
      backend: "speakers_on_cpu",
      models: [...ALL_MODELS, GEFORCE_RUNTIME],
      hardware: {
        is_mac: false,
        memory_gib: 64,
        gpu: { vendor: "nvidia", name: "NVIDIA GeForce RTX 5090", memory_gib: 32 },
      },
    }),
  }),
  state("models on geforce with its runtime", "models", {
    snapshot: snapshot({
      backend: "cuda",
      models: [...ALL_MODELS, { ...GEFORCE_RUNTIME, found_at: "/data/runtime/geforce" }],
      hardware: {
        is_mac: false,
        memory_gib: 64,
        gpu: { vendor: "nvidia", name: "NVIDIA GeForce RTX 5090", memory_gib: 32 },
      },
    }),
  }),
  state("models on radeon", "models", {
    snapshot: snapshot({
      backend: "vulkan",
      hardware: {
        is_mac: false,
        memory_gib: 32,
        gpu: { vendor: "radeon", name: "AMD Radeon RX 7900 XTX", memory_gib: 24 },
      },
    }),
  }),
  state("models on cpu only", "models", {
    snapshot: snapshot({
      hardware: { is_mac: false, memory_gib: 12, gpu: null },
    }),
  }),
  state("models with no recommended speaker model", "models", {
    snapshot: snapshot({
      models: ALL_MODELS.map(
        (row: ModelStatus): ModelStatus => (row.role === "diarize" ? { ...row, recommended: false } : row),
      ),
    }),
  }),
  state("models with nothing missing", "models", {
    snapshot: snapshot({
      models: ALL_MODELS.map(
        (row: ModelStatus): ModelStatus => (row.recommended ? { ...row, found_at: "/data/models/x" } : row),
      ),
    }),
  }),
  state("models reset dialog", "models", { steps: ["open-reset-dialog"] }),
  state("models downloading all at once", "models", {
    downloadEvents: [
      { state: "running", id: "whisper-large-v3-turbo", done_bytes: 0, total_bytes: 1_700_000_000 },
      {
        state: "running",
        id: "nemotron-3.5-transcribe-iq4",
        done_bytes: 800_000_000,
        total_bytes: 1_700_000_000,
      },
      {
        state: "running",
        id: "nemotron-3.5-transcribe-q8",
        done_bytes: 100_000_000,
        total_bytes: 1_700_000_000,
      },
    ],
  }),
  state("model downloading", "models", {
    downloadEvents: [
      {
        state: "running",
        id: "nemotron-3.5-transcribe-iq4",
        done_bytes: 800_000_000,
        total_bytes: 1_700_000_000,
      },
    ],
  }),
  state("model download failed", "models", {
    downloadEvents: [
      {
        state: "failed",
        id: "nemotron-3.5-transcribe-iq4",
        error: { code: "internal", detail: "download failed: SHA-256 mismatch" },
      },
    ],
  }),
];

// Japanese, English and German stand for all languages; the rest are checked for structure by unit tests.
export const LOCALES: string[] = ["en", "ja", "de"];
export const COLOR_SCHEMES: ("light" | "dark")[] = ["light", "dark"];
/** Hover and focus styling does not depend on language, so those sweeps run in these two scripts. */
export const INTERACTION_LOCALES: ReadonlySet<string> = new Set(["en", "ja"]);
export const NARROW_VIEWPORT: { width: number; height: number } = { width: 320, height: 640 };
