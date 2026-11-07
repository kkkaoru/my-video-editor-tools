// Runs with bun: the text every language must supply, and the formatter that fills it in.
import type { BackendError, Role, SeparationBackend, Stage, SubtitleFormat } from "../backend/types.ts";

export interface StepStates {
  done: string;
  current: string;
  pending: string;
}

export interface SpeakerFacts {
  runs: string;
  rows: string;
  memory: string;
  time: string;
}

export interface ModelNotes {
  "whisper-large-v3-turbo": string;
  "nemotron-3.5-speaker": string;
  "mossformer2-ss-16k-coreml": string;
  "mossformer2-ss-16k": string;
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
  needs_memory: string;
  transcript_missing: string;
  profile_exists: string;
  profile_missing: string;
  models_busy: string;
  models_missing: string;
  speaker_models_missing: string;
  update_busy: string;
  no_supported_files: string;
  profile_invalid: string;
  internal: string;
  cancelled: string;
}

/** One h3 block of the help page; `steps` is a numbered list, `notes` a plain one. */
export interface HelpPart {
  heading: string;
  intro: string;
  steps: string[];
  notes: string[];
}

export interface HelpSection {
  heading: string;
  parts: HelpPart[];
}

export interface HelpSections {
  start: HelpSection;
  transcribe: HelpSection;
  editor: HelpSection;
  look: HelpSection;
  rewrite: HelpSection;
  settings: HelpSection;
  trouble: HelpSection;
}

/** What each annotated picture shows, in the numbers its steps use. */
export interface HelpImages {
  models: string;
  transcribe: string;
  look: string;
  font: string;
  profiles: string;
  rewrite: string;
}

/** The DaVinci Resolve pictures are the same in every language; only their descriptions change. */
export interface ResolveFigures {
  menu: string;
  dialog: string;
  timeline: string;
  text: string;
  shading: string;
}

export interface HelpMessages {
  version: string;
  intro: string;
  sections: HelpSections;
  /** The folder of this language's pictures under `public/help`. */
  imageFolder: string;
  resolveFigures: ResolveFigures;
  images: HelpImages;
}

/** The colours every speaker colour picker offers. */
export type PresetColor = "white" | "black" | "red" | "orange" | "yellow" | "green" | "blue" | "purple";

export interface Messages {
  appName: string;
  tabJobs: string;
  tabStyle: string;
  tabSettings: string;
  uiLanguageLabel: string;
  uiLanguageSystem: string;
  uiThemeLabel: string;
  uiThemeSystem: string;
  uiThemeLight: string;
  licensesOpen: string;
  licensesLoading: string;
  licensesFailed: string;
  licensesNoText: string;
  licensesUnknown: string;
  licensesClose: string;
  uiThemeDark: string;
  updateCheck: string;
  updateChecking: string;
  updateUpToDate: string;
  updateCheckFailed: string;
  updateAvailable: string;
  updateInstall: string;
  updateRetry: string;
  updateBusy: string;
  updateInstalling: string;
  updateProgressLabel: string;
  updateInstallFailed: string;
  tabHelp: string;
  tabModels: string;
  noticeSpeakersOnCpu: string;
  noticeCpuOnly: string;
  dropTitle: string;
  dropHint: string;
  dropHover: string;
  pickVideos: string;
  pickSubtitles: string;
  subtitleFilterName: string;
  dropProfileHint: string;
  dropCurrentHint: string;
  speakerCountHelpLabel: string;
  speakerCountHelp: string;
  speakerLoadLight: string;
  speakerLoadHeavy: string;
  speakerOptionLoad: string;
  speakerLoadHeavyDetail: string;
  /** The figures in these texts come from speaker-figures.ts. */
  speakerFiguresNote: string;
  speakerFactLabels: SpeakerFacts;
  speakerFacts: SpeakerFacts;
  separationBackend: string;
  separationBackends: Record<SeparationBackend, string>;
  speakerPlanLine: string;
  speakerPlanRuns: string;
  /** Follows the sentence before it, so it starts with the space the language needs. */
  speakerPlanRunsSeparate: string;
  speakerPlanDeep: string;
  memoryUnified: string;
  memoryRam: string;
  memoryGpu: string;
  openModelsTab: string;
  runWithOneSpeaker: string;
  badgeUsedBySteps: string;
  speakerRolesHint: string;
  profilesHeading: string;
  profilesHint: string;
  profileName: string;
  profileSave: string;
  profileEmpty: string;
  profileSelected: string;
  profileChanged: string;
  profileLoad: string;
  profileOverwrite: string;
  profileOverwriteTitle: string;
  profileOverwriteDescription: string;
  profileDelete: string;
  profileDeleteTitle: string;
  profileDeleteDescription: string;
  profileExport: string;
  profileImport: string;
  profileFileFilter: string;
  resetLook: string;
  resetLookTitle: string;
  resetLookCurrent: string;
  resetLookProfile: string;
  resetLookConfirm: string;
  dialogCancel: string;
  fontFamily: string;
  fontSearch: string;
  fontHint: string;
  fontLoading: string;
  fontNoMatch: string;
  fontNotInstalled: string;
  downloadAllLabel: string;
  downloadAll: string;
  resetModels: string;
  resetModelsTitle: string;
  resetModelsDescription: string;
  resetModelsConfirm: string;
  videoFilterName: string;
  readinessMissing: string;
  roleTranscribe: string;
  roleDiarize: string;
  roleSeparate: string;
  roleFace: string;
  roleRuntime: string;
  jobQueued: string;
  jobCancelled: string;
  jobCancel: string;
  jobRelayout: string;
  jobDone: string;
  jobDoneOneSpeaker: string;
  jobFaceless: string;
  jobFontMissing: string;
  jobSpeakersMissing: string;
  jobNoPercent: string;
  speakerCount: string;
  subtitleFormat: string;
  speakerCountAuto: string;
  speakerCountValue: string;
  speakerCountOne: string;
  jobFewerSpeakers: string;
  jobStep: string;
  jobEstimating: string;
  jobRemaining: string;
  jobRemainingUnderMinute: string;
  stepStates: StepStates;
  outputPurposes: Record<string, string>;
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
  safeZoneHint: string;
  safeLeft: string;
  safeRight: string;
  faceModel: string;
  speechLanguage: string;
  languageAuto: string;
  sizeHeading: string;
  sizeHint: string;
  fontSize: string;
  captionWidth: string;
  captionLineHeight: string;
  captionRowSpacing: string;
  captionRowsLeft: string;
  captionRowsLeftOn: string;
  captionRowsLeftOff: string;
  contrastWarning: string;
  previewHeading: string;
  previewFrameLabel: string;
  previewDescription: string;
  previewFontPixels: string;
  previewSample: string;
  previewLandscape: string;
  previewVertical: string;
  previewUhd: string;
  speakersHeading: string;
  speakersHint: string;
  speakerName: string;
  colorColumn: string;
  fillWidth: string;
  outlineWidth: string;
  rimWidth: string;
  layerWidth: string;
  colorTransparent: string;
  colorCustom: string;
  colorNames: Record<PresetColor, string>;
  fillLabel: string;
  outlineLabel: string;
  rimLabel: string;
  /** What each format cannot show; `.drt` shows everything. */
  formatLimits: Record<Exclude<SubtitleFormat, "drt">, string>;
  shadowLabel: string;
  shadowColor: string;
  shadowX: string;
  shadowXShort: string;
  shadowY: string;
  shadowYShort: string;
  shadowBlur: string;
  shadowBlurShort: string;
  addSpeaker: string;
  removeSpeaker: string;
  resetPalette: string;
  modelsHeading: string;
  noRecommendedSpeakerModel: string;
  modelsHint: string;
  addModelFolder: string;
  modelFolders: string;
  badgeRecommended: string;
  badgePresent: string;
  download: string;
  downloadCancel: string;
  downloadProgress: string;
  downloadProgressLabel: string;
  deleteModel: string;
  sourceLabel: string;
  locationLabel: string;
  loadFailed: string;
  errors: ErrorMessages;
  modelNotes: ModelNotes;
  help: HelpMessages;
}

export type Params = Readonly<Record<string, string | number>>;

const PLACEHOLDER: RegExp = /\{(\w+)\}/g;

export const format = (template: string, params: Params): string =>
  template.replace(PLACEHOLDER, (whole: string, name: string): string => String(params[name] ?? whole));

const isErrorCode = (messages: Messages, code: string): code is keyof ErrorMessages =>
  code in messages.errors;

export const roleName = (messages: Messages, role: Role): string =>
  (
    ({
      transcribe: messages.roleTranscribe,
      diarize: messages.roleDiarize,
      separate: messages.roleSeparate,
      face: messages.roleFace,
      runtime: messages.roleRuntime,
    }) satisfies Record<Role, string>
  )[role];

/** Unknown codes still show their technical detail rather than nothing. */
export const errorText = (messages: Messages, error: BackendError, modelLabel: string): string =>
  isErrorCode(messages, error.code)
    ? format(messages.errors[error.code], {
        model: modelLabel,
        memory: { ram: messages.memoryRam, gpu_memory: messages.memoryGpu, unified: messages.memoryUnified }[
          error.kind ?? "ram"
        ],
        role: error.role === "diarize" ? messages.roleDiarize : messages.roleTranscribe,
        required: error.required ?? 0,
        available: error.available ?? 0,
        detail: error.detail ?? "",
        file: error.file ?? "",
        name: error.name ?? "",
        roles: (error.roles ?? error.missing ?? [])
          .map((role: Role): string => roleName(messages, role))
          .join(", "),
      })
    : (error.detail ?? error.code);
