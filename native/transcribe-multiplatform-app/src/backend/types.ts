// Runs with bun: the JSON shapes the Rust backend sends, kept in step with src-tauri by hand.

export type Anchor = "face_above" | "face_below" | "video_top" | "video_bottom";
export type Role = "transcribe" | "diarize" | "face" | "runtime";
export type Stage = "convert_model" | "extract_audio" | "transcribe" | "diarize" | "faces" | "export";

export interface SpeakerColor {
  fill: string;
  outline: string;
}

export interface Placement {
  anchor: Anchor;
  offset_x_percent: number;
  offset_y_percent: number;
}

export interface CaptionShadow {
  enabled: boolean;
  /** 0 to 1. */
  strength: number;
}

export interface Settings {
  palette: SpeakerColor[];
  unassigned: SpeakerColor;
  font_family: string;
  font_ratio: number;
  outline_ratio: number;
  rim_ratio: number;
  line_height: number;
  shadow: CaptionShadow;
  width_ratio: number;
  placement: Placement;
  language: string;
  diarize_model: string;
  speaker_count: number;
  face_model: string;
  model_dirs: string[];
}

export type Backend = "default" | "vulkan" | "cuda";
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

export interface BackendError {
  code: string;
  detail?: string;
  model?: string;
  role?: string;
  required?: number;
  available?: number;
  limit_minutes?: number;
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
  hidden: boolean;
}

export interface Snapshot {
  locale: string;
  hardware: Hardware;
  backend: Backend;
  settings: Settings;
  models: ModelStatus[];
  models_dir: string;
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
  outputs: string[];
  base: string;
  speaker_count: number;
  requested_speakers: number;
  cue_count: number;
  faceless: boolean;
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
