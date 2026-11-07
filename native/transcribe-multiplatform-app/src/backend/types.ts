// Runs with bun: the JSON shapes the Rust backend sends, kept in step with src-tauri by hand.

export type Anchor = "face_above" | "face_below" | "video_top" | "video_bottom";
export type Role = "transcribe" | "diarize" | "separate" | "face" | "runtime";
export type Stage =
  | "convert_model"
  | "extract_audio"
  | "transcribe"
  | "diarize"
  | "separate"
  | "retranscribe"
  | "faces"
  | "export";

/** Widths are layer thicknesses, as ratios of the text size; they stack from the glyph outwards. */
export interface SpeakerColor {
  fill: string;
  fill_width: number;
  outline: string;
  outline_width: number;
  /** Null is transparent: no such layer, whatever `rim_width` says. */
  rim: string | null;
  rim_width: number;
  /** Null is no shadow: the rest of these are then ignored. Only the `.drt` format draws it. */
  shadow: string | null;
  /** Half a ratio of the text size each, negative `shadow_y` being down. */
  shadow_x: number;
  shadow_y: number;
  shadow_blur: number;
}

export interface Placement {
  anchor: Anchor;
  offset_x_percent: number;
  offset_y_percent: number;
}

/** What a profile saves and restores. */
export interface CaptionLook {
  palette: SpeakerColor[];
  font_family: string;
  font_ratio: number;
  line_height: number;
  width_ratio: number;
  /** Strips at the sides of the width that captions keep out of. */
  safe_left_percent: number;
  safe_right_percent: number;
  placement: Placement;
}

export interface Profile {
  name: string;
  look: CaptionLook;
}

export type UiTheme = "system" | "light" | "dark";
export type SubtitleFormat = "drt" | "fcpxml" | "ttml" | "srt";

export interface Settings extends CaptionLook {
  language: string;
  /** Mac only; other systems ignore it. */
  separation_backend: SeparationBackend;
  speaker_count: number;
  subtitle_format: SubtitleFormat;
  /** Empty follows the system; a language tag overrides it. */
  ui_language: string;
  ui_theme: UiTheme;
  face_model: string;
  model_dirs: string[];
  /** The profile dropped subtitle files follow; empty for the settings above. */
  active_profile: string;
}

export type Backend = "default" | "vulkan" | "cuda" | "speakers_on_cpu";
export type GpuVendor = "nvidia" | "radeon";

export interface Gpu {
  vendor: GpuVendor;
  name: string;
  memory_gib: number;
}

export interface Hardware {
  is_mac: boolean;
  memory_gib: number;
  gpu: Gpu | null;
}

/** What a job for several speakers runs on. */
export type RunsOn = "gpu" | "cpu";
export type SeparationBackend = "onnx" | "coreml";

export interface SpeakerPlan {
  diarize: string;
  whisper_on: RunsOn;
  diarize_on: RunsOn;
  separate_on: RunsOn;
  /** The model id that separates. */
  separator: string;
  /** Three people at once go through separation twice, for up to four rows. */
  deep: boolean;
}

export interface SpeakerMissing {
  missing: Role[];
  suggested: string | null;
}

export interface BackendError {
  code: string;
  /** `needs_memory`: which memory runs short. */
  kind?: "ram" | "gpu_memory" | "unified";
  /** `speaker_models_missing`: the model to offer for download. */
  suggested?: string | null;
  /** `speaker_models_missing`: the kinds of model a job needs. */
  missing?: Role[];
  detail?: string;
  model?: string;
  role?: string;
  required?: number;
  available?: number;
  file?: string;
  name?: string;
  roles?: Role[];
}

export interface ModelStatus {
  id: string;
  role: Role;
  label: string;
  source_url: string;
  download_bytes: number;
  found_at: string | null;
  blocked: BackendError | null;
  recommended: boolean;
}

/** A newer version on offer. */
export interface UpdateInfo {
  version: string;
  notes: string | null;
}

export interface Snapshot {
  /** The app's version, from Cargo. */
  version: string;
  locale: string;
  hardware: Hardware;
  backend: Backend;
  settings: Settings;
  models: ModelStatus[];
  /** Null for one speaker, and for several when their models are missing. */
  speaker_plan: SpeakerPlan | null;
  /** Why several speakers have no plan, and what to download first. */
  speaker_missing: SpeakerMissing | null;
  models_dir: string;
  profiles: Profile[];
}

export interface Progress {
  stage: Stage;
  overall: number;
  /** Progress inside this stage, 0 to 1; an estimate when `determinate` is false. */
  stage_fraction: number;
  determinate: boolean;
  /** The stages this job runs, in order. */
  plan: Stage[];
  /** Estimated seconds left for the whole job, from measured speeds; `null` until one is known. */
  remaining_seconds: number | null;
}

export interface JobOutcome {
  /** What actually ran; null for one speaker. */
  speaker_plan: SpeakerPlan | null;
  outputs: string[];
  base: string;
  speaker_count: number;
  requested_speakers: number;
  cue_count: number;
  faceless: boolean;
  /** The chosen font is not installed, so caption widths were estimated. */
  font_missing: boolean;
  /** Speakers were asked for and the model heard none, so every caption has one colour. */
  speakers_missing: boolean;
}

export interface JobQueuedEvent {
  state: "queued";
  id: number;
  path: string;
}

export interface JobRunningEvent {
  state: "running";
  id: number;
  progress: Progress;
}

export interface JobFinishedEvent {
  state: "finished";
  id: number;
  outcome: JobOutcome;
}

export interface JobFailedEvent {
  state: "failed";
  id: number;
  error: BackendError;
}

export interface JobCancelledEvent {
  state: "cancelled";
  id: number;
}

export type JobEvent =
  | JobQueuedEvent
  | JobRunningEvent
  | JobFinishedEvent
  | JobFailedEvent
  | JobCancelledEvent;

export interface DownloadRunningEvent {
  state: "running";
  id: string;
  done_bytes: number;
  total_bytes: number;
}

export interface DownloadFinishedEvent {
  state: "finished";
  id: string;
}

export interface DownloadFailedEvent {
  state: "failed";
  id: string;
  error: BackendError;
}

export type DownloadEvent = DownloadRunningEvent | DownloadFinishedEvent | DownloadFailedEvent;
