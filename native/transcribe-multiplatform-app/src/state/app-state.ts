// Runs with bun: pure state transitions; side effects live in use-app-store.ts.
import type {
  BackendError,
  CaptionLook,
  DownloadEvent,
  JobEvent,
  JobOutcome,
  JobQueuedEvent,
  ModelStatus,
  Progress,
  Role,
  Settings,
  Snapshot,
  SpeakerMissing,
} from "../backend/types.ts";

const SYSTEM_LANGUAGE: string = "";

interface JobBase {
  id: number;
  path: string;
  startedAt: number;
}

export interface QueuedJobRow extends JobBase {
  state: "queued";
}

export interface RunningJobRow extends JobBase {
  state: "running";
  progress: Progress;
}

export interface FinishedJobRow extends JobBase {
  state: "finished";
  outcome: JobOutcome;
}

export interface FailedJobRow extends JobBase {
  state: "failed";
  error: BackendError;
}

export interface CancelledJobRow extends JobBase {
  state: "cancelled";
}

export type JobRow = QueuedJobRow | RunningJobRow | FinishedJobRow | FailedJobRow | CancelledJobRow;

export interface DownloadRow {
  running: boolean;
  doneBytes: number;
  totalBytes: number;
  error: BackendError | null;
}

export interface AppState {
  snapshot: Snapshot;
  jobs: JobRow[];
  downloads: ReadonlyMap<string, DownloadRow>;
  hovering: boolean;
  loadError: BackendError | null;
  /** Settings changed and unanswered: the speaker plan in the snapshot is the old one. */
  planPending: boolean;
}

export const initialState = (snapshot: Snapshot): AppState => ({
  snapshot,
  jobs: [],
  downloads: new Map(),
  hovering: false,
  loadError: null,
  planPending: false,
});

const newJob = (id: number, path: string, startedAt: number): JobRow => ({
  id,
  path,
  state: "queued",
  startedAt,
});

const withEvent = ({ id, path, startedAt }: JobRow, event: Exclude<JobEvent, JobQueuedEvent>): JobRow => {
  switch (event.state) {
    case "running":
      return { id, path, startedAt, state: "running", progress: event.progress };
    case "finished":
      return { id, path, startedAt, state: "finished", outcome: event.outcome };
    case "failed":
      return { id, path, startedAt, state: "failed", error: event.error };
    case "cancelled":
      return { id, path, startedAt, state: "cancelled" };
  }
};

/** Newest first; events for an unknown job are dropped because the path arrives with `queued`. */
export const applyJobEvent = (state: AppState, event: JobEvent, now: number): AppState =>
  event.state === "queued"
    ? { ...state, jobs: [newJob(event.id, event.path, now), ...state.jobs] }
    : {
        ...state,
        jobs: state.jobs.map((job: JobRow): JobRow => (job.id === event.id ? withEvent(job, event) : job)),
      };

const withDownload = (state: AppState, id: string, row: DownloadRow | null): AppState => {
  const downloads: Map<string, DownloadRow> = new Map(state.downloads);
  if (row === null) {
    downloads.delete(id);
  } else {
    downloads.set(id, row);
  }
  return { ...state, downloads };
};

export const startedDownload = (state: AppState, id: string, totalBytes: number): AppState =>
  withDownload(state, id, { running: true, doneBytes: 0, totalBytes, error: null });

export const applyDownloadEvent = (state: AppState, event: DownloadEvent): AppState => {
  switch (event.state) {
    case "running":
      return withDownload(state, event.id, {
        running: true,
        doneBytes: event.done_bytes,
        totalBytes: event.total_bytes,
        error: null,
      });
    case "failed":
      return withDownload(state, event.id, {
        running: false,
        doneBytes: 0,
        totalBytes: 0,
        error: event.error,
      });
    default:
      return withDownload(state, event.id, null);
  }
};

export const withSettings = (state: AppState, settings: Settings): AppState => ({
  ...state,
  snapshot: { ...state.snapshot, settings },
  planPending: true,
});

/** A save that answered also ends the banner of an earlier failed one. */
export const withModels = (state: AppState, models: ModelStatus[]): AppState => ({
  ...state,
  snapshot: { ...state.snapshot, models },
  loadError: null,
});

/** A save answers with the models and the language it now means; newer settings on screen stay. */
export const withSaveAnswer = (
  state: AppState,
  { models, locale, speaker_plan, speaker_missing }: Snapshot,
): AppState => {
  const updated: AppState = withModels(state, models);
  return {
    ...updated,
    snapshot: { ...updated.snapshot, locale, speaker_plan, speaker_missing },
    planPending: false,
  };
};

const isJobActive = ({ state }: JobRow): boolean => state === "queued" || state === "running";

/** What the backend refuses to restart under. */
export const isWorking = ({ jobs, downloads }: Pick<AppState, "jobs" | "downloads">): boolean =>
  jobs.some(isJobActive) ||
  Array.from(downloads.values()).some(({ running }: DownloadRow): boolean => running);

/** The language to show: the one chosen in the settings, or the system's. */
export const uiLocale = ({ locale, settings }: Pick<Snapshot, "locale" | "settings">): string =>
  settings.ui_language === SYSTEM_LANGUAGE ? locale : settings.ui_language;

/** One speaker runs Whisper alone; automatic detection and more speakers run the whole chain. */
export const isHeavySpeakerCount = ({ speaker_count: count }: Pick<Settings, "speaker_count">): boolean =>
  count !== 1;

/** Speaker separation has its own refusal: see lacksSpeakerModels. */
export const missingModelRoles = ({ models }: Pick<Snapshot, "models">): Role[] =>
  (["transcribe"] satisfies Role[]).filter(
    (role: Role): boolean =>
      !models.some(
        (model: ModelStatus): boolean =>
          model.role === role && model.found_at !== null && model.blocked === null,
      ),
  );

/** What the backend says is missing, unless settings it has not answered for may have changed that. */
export const speakerModelsMissing = ({
  snapshot,
  planPending,
}: Pick<AppState, "snapshot" | "planPending">): SpeakerMissing | null =>
  planPending ? null : snapshot.speaker_missing;

/** Nothing starts without its models; null lets a video through. */
export const videoRefusal = (state: Pick<AppState, "snapshot" | "planPending">): BackendError | null => {
  const missing: Role[] = missingModelRoles(state.snapshot);
  if (missing.length > 0) {
    return { code: "models_missing", roles: missing };
  }
  const speakers: SpeakerMissing | null = speakerModelsMissing(state);
  return speakers === null ? null : { code: "speaker_models_missing", ...speakers };
};

const lookFields = ({
  palette,
  font_family,
  font_ratio,
  line_height,
  width_ratio,
  safe_left_percent,
  safe_right_percent,
  placement,
}: CaptionLook): CaptionLook => ({
  palette,
  font_family,
  font_ratio,
  line_height,
  width_ratio,
  safe_left_percent,
  safe_right_percent,
  placement,
});

/** Whether the look on screen differs from what the profile saved. */
export const lookChanged = (settings: Settings, saved: CaptionLook): boolean =>
  JSON.stringify(lookFields(settings)) !== JSON.stringify(lookFields(saved));
