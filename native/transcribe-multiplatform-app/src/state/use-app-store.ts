// Runs with bun: connects the pure state to the backend and keeps settings saved.
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import {
  addVideos,
  fetchSnapshot,
  onDownloadEvent,
  onFileDrop,
  onJobEvent,
  reformatFiles,
  saveSettings,
} from "../backend/backend.ts";
import type { BackendError, Settings, Snapshot } from "../backend/types.ts";
import {
  type AppState,
  applyDownloadEvent,
  applyJobEvent,
  initialState,
  videoRefusal,
  withSaveAnswer,
  withSettings,
} from "./app-state.ts";

export interface AppStore {
  state: AppState;
  setState: (update: (state: AppState) => AppState) => void;
  updateSettings: (settings: Settings) => void;
  applySnapshot: (snapshot: Snapshot) => void;
  /** For a snapshot that is the newest word on the settings: a save still waiting would undo it. */
  replaceSnapshot: (snapshot: Snapshot) => void;
  /** Videos are transcribed and subtitle files written again; anything else is refused out loud. */
  dropFiles: (paths: string[]) => void;
}

const SAVE_DELAY_MS: number = 250;
const VIDEO_PATTERN: RegExp = /\.(mp4|mov|mkv|m4v|webm|avi|mts|mxf|ts)$/i;
const SUBTITLE_PATTERN: RegExp = /\.(drt|fcpxml|srt|ttml|transcript\.json)$/i;

export const VIDEO_EXTENSIONS: string[] = ["mp4", "mov", "mkv", "m4v", "webm", "avi", "mts", "mxf", "ts"];
export const SUBTITLE_EXTENSIONS: string[] = ["drt", "fcpxml", "srt", "ttml", "ass", "json"];

const isBackendError = (error: unknown): error is BackendError =>
  typeof error === "object" && error !== null && "code" in error && typeof error.code === "string";

const failureText = (error: unknown): string =>
  typeof error === "object" && !(error instanceof Error) ? JSON.stringify(error) : String(error);

/** The backend rejects with a translatable code; anything else is shown as readable text. */
export const asBackendError = (error: unknown): BackendError =>
  isBackendError(error) ? error : { code: "internal", detail: failureText(error) };

/** Failures of background calls surface in the UI instead of vanishing. */
export function reportFailure(setState: AppStore["setState"], error: unknown): void {
  setState((state: AppState): AppState => ({ ...state, loadError: asBackendError(error) }));
}

export const onlyVideos = (paths: string[]): string[] =>
  paths.filter((path: string): boolean => VIDEO_PATTERN.test(path));

export const onlySubtitles = (paths: string[]): string[] =>
  paths.filter((path: string): boolean => SUBTITLE_PATTERN.test(path));

/** A drop with no paths at all (dragged text, say) brought no file to refuse. */
const isAllUnsupported = (paths: string[], videos: string[], subtitles: string[]): boolean =>
  paths.length > 0 && videos.length === 0 && subtitles.length === 0;

export function useAppStore(initial: Snapshot): AppStore {
  const [state, setState] = useState<AppState>((): AppState => initialState(initial));
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latestSnapshot = useRef<Snapshot>(initial);
  latestSnapshot.current = state.snapshot;
  const latestState = useRef<AppState>(state);
  latestState.current = state;

  const applySnapshot = useCallback((snapshot: Snapshot): void => {
    setState(
      (current: AppState): AppState => ({ ...current, snapshot, loadError: null, planPending: false }),
    );
  }, []);

  const replaceSnapshot = useCallback(
    (snapshot: Snapshot): void => {
      if (saveTimer.current !== null) {
        clearTimeout(saveTimer.current);
      }
      applySnapshot(snapshot);
    },
    [applySnapshot],
  );

  /** Nothing starts without its models: the refusal says what is missing and what to do. */
  const dropVideos = useCallback((videos: string[]): void => {
    const refusal: BackendError | null = videoRefusal(latestState.current);
    if (refusal !== null) {
      reportFailure(setState, refusal);
      return;
    }
    addVideos(videos).catch((error: unknown): void => reportFailure(setState, error));
  }, []);

  const dropFiles = useCallback(
    (paths: string[]): void => {
      const videos: string[] = onlyVideos(paths);
      const subtitles: string[] = onlySubtitles(paths);
      if (isAllUnsupported(paths, videos, subtitles)) {
        reportFailure(setState, { code: "no_supported_files" });
      }
      if (videos.length > 0) {
        dropVideos(videos);
      }
      if (subtitles.length > 0) {
        reformatFiles(subtitles).catch((error: unknown): void => reportFailure(setState, error));
      }
    },
    [dropVideos],
  );

  const updateSettings = useCallback((settings: Settings): void => {
    setState((current: AppState): AppState => withSettings(current, settings));
    if (saveTimer.current !== null) {
      clearTimeout(saveTimer.current);
    }
    saveTimer.current = setTimeout((): void => {
      saveSettings(settings)
        // Not the settings: newer ones may have been edited while this save was in flight.
        .then((answer: Snapshot): void =>
          setState((current: AppState): AppState => withSaveAnswer(current, answer)),
        )
        .catch((error: unknown): void => reportFailure(setState, error));
    }, SAVE_DELAY_MS);
  }, []);

  useEffect((): (() => void) => {
    const report = (error: unknown): void => reportFailure(setState, error);
    const subscriptions = [
      onJobEvent((event): void =>
        setState((current: AppState): AppState => applyJobEvent(current, event, Date.now())),
      ),
      onDownloadEvent((event): void => {
        setState((current: AppState): AppState => applyDownloadEvent(current, event));
        // Progress events arrive four times a second; rescanning the model folders is only for the end.
        if (event.state !== "running") {
          fetchSnapshot().then(applySnapshot).catch(report);
        }
      }),
      onFileDrop({
        onHover: (hovering: boolean): void =>
          setState((current: AppState): AppState => ({ ...current, hovering })),
        onDrop: (paths: string[]): void => {
          setState((current: AppState): AppState => ({ ...current, hovering: false }));
          dropFiles(paths);
        },
      }),
    ];
    // Attached now: a subscription that cannot start must show while the app is open, not at unmount.
    subscriptions.map((subscription) => subscription.catch(report));
    return (): void => {
      subscriptions.map((subscription) => subscription.then((stop) => stop()).catch(report));
    };
  }, [applySnapshot, dropFiles]);

  return { state, setState, updateSettings, applySnapshot, replaceSnapshot, dropFiles };
}
