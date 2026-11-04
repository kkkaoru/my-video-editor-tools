// Runs with bun: the text every language must supply, and the formatter that fills it in.
import type { BackendError, Stage } from "../backend/types.ts";

export interface ModelNotes {
  "whisper-large-v3-turbo": string;
  "nemotron-3.5-transcribe-iq4": string;
  "nemotron-3.5-transcribe-nvfp4": string;
  "nemotron-3.5-transcribe-q2m": string;
  "nemotron-3.5-transcribe-q2l": string;
  "nemotron-3.5-transcribe-q8": string;
  "nemotron-3-diarization": string;
  yunet: string;
  "avatar-face-yolo": string;
  "geforce-runtime": string;
}

export interface ErrorMessages {
  no_video_stream: string;
  no_audio_stream: string;
  unreadable_size: string;
  unreadable_duration: string;
  no_words: string;
  model_missing: string;
  no_model_for_role: string;
  needs_nvidia: string;
  needs_memory: string;
  internal: string;
  cancelled: string;
}

export interface Messages {
  appName: string;
  tabJobs: string;
  tabStyle: string;
  tabModels: string;
  dropTitle: string;
  dropHint: string;
  dropHover: string;
  pickVideos: string;
  videoFilterName: string;
  readinessMissing: string;
  roleTranscribe: string;
  roleDiarize: string;
  roleFace: string;
  roleRuntime: string;
  jobQueued: string;
  jobCancelled: string;
  jobCancel: string;
  jobRelayout: string;
  jobDone: string;
  jobFaceless: string;
  jobNoPercent: string;
  jobElapsed: string;
  jobProgressLabel: string;
  jobOutputs: string;
  stages: Record<Stage, string>;
  positionHeading: string;
  positionLabel: string;
  anchorFaceAbove: string;
  anchorFaceBelow: string;
  anchorVideoTop: string;
  anchorVideoBottom: string;
  offsetX: string;
  offsetY: string;
  faceModel: string;
  speechLanguage: string;
  languageAuto: string;
  sizeHeading: string;
  sizeHint: string;
  fontSize: string;
  captionWidth: string;
  outlineWidth: string;
  previewHeading: string;
  previewFrameLabel: string;
  previewDescription: string;
  previewSample: string;
  previewLandscape: string;
  previewVertical: string;
  previewUhd: string;
  speakersHeading: string;
  speakersHint: string;
  speakerName: string;
  fillColor: string;
  outlineColor: string;
  addSpeaker: string;
  removeSpeaker: string;
  resetPalette: string;
  modelsHeading: string;
  modelsHint: string;
  addModelFolder: string;
  modelFolders: string;
  badgeRecommended: string;
  badgePresent: string;
  useModel: string;
  inUseModel: string;
  download: string;
  downloadCancel: string;
  downloadProgress: string;
  downloadProgressLabel: string;
  deleteModel: string;
  sourceLabel: string;
  locationLabel: string;
  hardwareCpu: string;
  hardwareMetal: string;
  hardwareMemory: string;
  loadFailed: string;
  errors: ErrorMessages;
  modelNotes: ModelNotes;
}

export type Params = Readonly<Record<string, string | number>>;

const PLACEHOLDER: RegExp = /\{(\w+)\}/g;

export const format = (template: string, params: Params): string =>
  template.replace(PLACEHOLDER, (whole: string, name: string): string => String(params[name] ?? whole));

const isErrorCode = (messages: Messages, code: string): code is keyof ErrorMessages =>
  code in messages.errors;

/** Unknown codes still show their technical detail rather than nothing. */
export const errorText = (messages: Messages, error: BackendError, modelLabel: string): string =>
  isErrorCode(messages, error.code)
    ? format(messages.errors[error.code], {
        model: modelLabel,
        role: error.role === "diarize" ? messages.roleDiarize : messages.roleTranscribe,
        required: error.required ?? 0,
        available: error.available ?? 0,
        detail: error.detail ?? "",
      })
    : (error.detail ?? error.code);
