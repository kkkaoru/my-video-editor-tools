// Runs with bun: every UI state, generated from the types so a new stage or error code joins the sweep.
import type {
  BackendError,
  DownloadEvent,
  JobEvent,
  ModelStatus,
  Snapshot,
  Stage,
} from "../../src/backend/types.ts";
import { enMessages } from "../../src/i18n/locale-en.ts";
import type { ErrorMessages } from "../../src/i18n/messages.ts";

export type Tab = "jobs" | "style" | "models";
export type StateStep = "open-face-select" | "open-language-select" | "hover-drop";

export interface UiState {
  name: string;
  tab: Tab;
  snapshot: Snapshot;
  jobEvents: JobEvent[];
  downloadEvents: DownloadEvent[];
  anchor: string | null;
  frame: string | null;
  steps: StateStep[];
}

const STAGES: Record<Stage, true> = {
  convert_model: true,
  extract_audio: true,
  transcribe: true,
  diarize: true,
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
  "#3A3AA8",
  "#5C5C00",
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
  hidden: false,
  ...changes,
});

const ALL_MODELS: ModelStatus[] = [
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
  model("nemotron-3-diarization", "diarize", "Nemotron 3 Diarization (Q8)", {
    found_at: "/user/models/nemotron.gguf",
    download_bytes: 107_000_000,
  }),
  model("yunet", "face", "YuNet", { found_at: "/app/yunet.onnx", download_bytes: 0, recommended: true }),
  model("avatar-face-yolo", "face", "Avatar face YOLO", { download_bytes: 0, source_url: "" }),
  model("geforce-runtime", "runtime", "GeForce CUDA runtime", {}),
];

const snapshot = (changes: Partial<Snapshot>): Snapshot => ({
  locale: "en",
  backend: "default",
  hardware: { is_mac: true, memory_gib: 48, gpu: null },
  models: ALL_MODELS,
  models_dir: "/data/models",
  settings: {
    palette: FIRST_FILL.map((fill: string, index: number) => ({
      fill,
      outline: index === 0 ? "#000000" : "#FFFFFF",
    })),
    unassigned: { fill: "#595959", outline: "#FFFFFF" },
    font_family: "Noto Sans JP",
    font_ratio: 0.048,
    outline_ratio: 0.115,
    rim_ratio: 0.034,
    line_height: 1.448,
    shadow: { enabled: true, strength: 0.8 },
    width_ratio: 0.9,
    placement: { anchor: "video_bottom", offset_x_percent: 0, offset_y_percent: 0 },
    language: "auto",
    diarize_model: "nemotron-3-diarization",
    speaker_count: 0,
    face_model: "yunet",
    model_dirs: ["/user/models"],
  },
  ...changes,
});

const state = (name: string, tab: Tab, changes: Partial<UiState>): UiState => ({
  name,
  tab,
  snapshot: snapshot({}),
  jobEvents: [],
  downloadEvents: [],
  anchor: null,
  frame: null,
  steps: [],
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
  state("jobs missing models", "jobs", {
    snapshot: snapshot({
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
        },
      },
    ],
  }),
  state("jobs with one speaker and no speaker model", "jobs", {
    snapshot: snapshot({
      settings: { ...snapshot({}).settings, speaker_count: 1 },
      models: ALL_MODELS.map(
        (row: ModelStatus): ModelStatus => (row.role === "diarize" ? { ...row, found_at: null } : row),
      ),
    }),
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
  state("style without shadow", "style", {
    snapshot: snapshot({
      settings: { ...snapshot({}).settings, shadow: { enabled: false, strength: 0.5 } },
    }),
  }),
  state("style with low contrast colours", "style", {
    snapshot: snapshot({
      settings: {
        ...snapshot({}).settings,
        palette: [{ fill: "#C48CFF", outline: "#FFFFFF" }, ...snapshot({}).settings.palette.slice(1)],
        unassigned: { fill: "#C8C8C8", outline: "#FFFFFF" },
      },
    }),
  }),
  state("style face select open", "style", { steps: ["open-face-select"] }),
  state("style language select open", "style", { steps: ["open-language-select"] }),
  state("models idle", "models", {}),
  state("models on geforce", "models", {
    snapshot: snapshot({
      hardware: {
        is_mac: false,
        memory_gib: 64,
        gpu: { vendor: "nvidia", name: "NVIDIA GeForce RTX 5090", memory_gib: 32 },
      },
    }),
  }),
  state("models on radeon", "models", {
    snapshot: snapshot({
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

export const LOCALES: string[] = ["en", "ja", "zh-CN", "zh-TW", "ko", "de", "fr", "es"];
export const COLOR_SCHEMES: ("light" | "dark")[] = ["light", "dark"];
/** Hover and focus styling does not depend on language, so those sweeps run in these two scripts. */
export const INTERACTION_LOCALES: ReadonlySet<string> = new Set(["en", "ja"]);
export const NARROW_VIEWPORT: { width: number; height: number } = { width: 320, height: 640 };
