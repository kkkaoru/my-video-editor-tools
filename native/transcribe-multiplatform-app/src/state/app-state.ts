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
} from "../backend/types.ts";

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
}

export const initialState = (snapshot: Snapshot): AppState => ({
  snapshot,
  jobs: [],
  downloads: new Map(),
  hovering: false,
  loadError: null,
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
});

/** A save that answered also ends the banner of an earlier failed one. */
export const withModels = (state: AppState, models: ModelStatus[]): AppState => ({
  ...state,
  snapshot: { ...state.snapshot, models },
  loadError: null,
});

/** One known speaker skips speaker separation, so it needs no model for it. */
export const missingModelRoles = ({ models, settings }: Snapshot): Role[] =>
  (settings.speaker_count === 1
    ? (["transcribe"] satisfies Role[])
    : (["transcribe", "diarize"] satisfies Role[])
  ).filter(
    (role: Role): boolean =>
      !models.some(
        (model: ModelStatus): boolean =>
          model.role === role && model.found_at !== null && model.blocked === null,
      ),
  );

const lookFields = ({
  palette,
  unassigned,
  font_family,
  font_ratio,
  outline_ratio,
  rim_ratio,
  line_height,
  shadow,
  width_ratio,
  placement,
}: CaptionLook): CaptionLook => ({
  palette,
  unassigned,
  font_family,
  font_ratio,
  outline_ratio,
  rim_ratio,
  line_height,
  shadow,
  width_ratio,
  placement,
});

/** Whether the look on screen differs from what the profile saved. */
export const lookChanged = (settings: Settings, saved: CaptionLook): boolean =>
  JSON.stringify(lookFields(settings)) !== JSON.stringify(lookFields(saved));
