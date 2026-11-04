// Runs with bun: pure state transitions; side effects live in use-app-store.ts.
import type {
  BackendError,
  DownloadEvent,
  JobEvent,
  JobOutcome,
  JobQueuedEvent,
  Progress,
  Settings,
  Snapshot,
} from "../backend/types.ts";

export type JobState = "queued" | "running" | "finished" | "failed" | "cancelled";

export interface JobRow {
  id: number;
  path: string;
  state: JobState;
  startedAt: number;
  progress: Progress | null;
  outcome: JobOutcome | null;
  error: BackendError | null;
}

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
  progress: null,
  outcome: null,
  error: null,
});

const withEvent = (job: JobRow, event: Exclude<JobEvent, JobQueuedEvent>): JobRow => {
  switch (event.state) {
    case "running":
      return { ...job, state: "running", progress: event.progress };
    case "finished":
      return { ...job, state: "finished", outcome: event.outcome };
    case "failed":
      return { ...job, state: "failed", error: event.error };
    case "cancelled":
      return { ...job, state: "cancelled" };
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
