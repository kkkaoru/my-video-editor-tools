// Runs with bun: connects the pure state to the backend and keeps settings saved.
import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import {
  addVideos,
  fetchSnapshot,
  onDownloadEvent,
  onFileDrop,
  onJobEvent,
  saveSettings,
} from "../backend/backend.ts";
import type { BackendError, Settings, Snapshot } from "../backend/types.ts";
import { type AppState, applyDownloadEvent, applyJobEvent, initialState, withSettings } from "./app-state.ts";

export interface AppStore {
  state: AppState;
  setState: (update: (state: AppState) => AppState) => void;
  updateSettings: (settings: Settings) => void;
  applySnapshot: (snapshot: Snapshot) => void;
  dropVideos: (paths: string[]) => void;
}

const SAVE_DELAY_MS: number = 250;
const VIDEO_PATTERN: RegExp = /\.(mp4|mov|mkv|m4v|webm|avi|mts|mxf|ts)$/i;

export const VIDEO_EXTENSIONS: string[] = ["mp4", "mov", "mkv", "m4v", "webm", "avi", "mts", "mxf", "ts"];

const asBackendError = (error: unknown): BackendError => ({ code: "internal", detail: String(error) });

/** Failures of background calls surface in the UI instead of vanishing. */
export function reportFailure(setState: AppStore["setState"], error: unknown): void {
  setState((state: AppState): AppState => ({ ...state, loadError: asBackendError(error) }));
}

export const onlyVideos = (paths: string[]): string[] =>
  paths.filter((path: string): boolean => VIDEO_PATTERN.test(path));

export function useAppStore(initial: Snapshot): AppStore {
  const [state, setState] = useState<AppState>((): AppState => initialState(initial));
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const applySnapshot = useCallback((snapshot: Snapshot): void => {
    setState((current: AppState): AppState => ({ ...current, snapshot, loadError: null }));
  }, []);

  const dropVideos = useCallback((paths: string[]): void => {
    addVideos(onlyVideos(paths)).catch((error: unknown): void => reportFailure(setState, error));
  }, []);

  const updateSettings = useCallback(
    (settings: Settings): void => {
      setState((current: AppState): AppState => withSettings(current, settings));
      if (saveTimer.current !== null) {
        clearTimeout(saveTimer.current);
      }
      saveTimer.current = setTimeout((): void => {
        saveSettings(settings)
          .then(applySnapshot)
          .catch((error: unknown): void => reportFailure(setState, error));
      }, SAVE_DELAY_MS);
    },
    [applySnapshot],
  );

  useEffect((): (() => void) => {
    const report = (error: unknown): void => reportFailure(setState, error);
    const subscriptions = [
      onJobEvent((event): void =>
        setState((current: AppState): AppState => applyJobEvent(current, event, Date.now())),
      ),
      onDownloadEvent((event): void => {
        setState((current: AppState): AppState => applyDownloadEvent(current, event));
        fetchSnapshot().then(applySnapshot).catch(report);
      }),
      onFileDrop({
        onHover: (hovering: boolean): void =>
          setState((current: AppState): AppState => ({ ...current, hovering })),
        onDrop: (paths: string[]): void => {
          setState((current: AppState): AppState => ({ ...current, hovering: false }));
          dropVideos(paths);
        },
      }),
    ];
    // Attached now: a subscription that cannot start must show while the app is open, not at unmount.
    subscriptions.map((subscription) => subscription.catch(report));
    return (): void => {
      subscriptions.map((subscription) => subscription.then((stop) => stop()).catch(report));
    };
  }, [applySnapshot, dropVideos]);

  return { state, setState, updateSettings, applySnapshot, dropVideos };
}
