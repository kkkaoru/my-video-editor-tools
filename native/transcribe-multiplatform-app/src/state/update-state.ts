// Runs with bun: pure transitions of the app update; side effects live in use-updater.ts.
import type { BackendError, UpdateInfo } from "../backend/types.ts";

export interface UpdateState {
  available: UpdateInfo | null;
  /** The check the person asked for; the one at startup stays out of sight. */
  checking: boolean;
  upToDate: boolean;
  checkError: BackendError | null;
  installing: boolean;
  /** The share of the download, 0 to 1; null until it reports, and while its size is unknown. */
  progress: number | null;
  installError: BackendError | null;
}

export const INITIAL_UPDATE: UpdateState = {
  available: null,
  checking: false,
  upToDate: false,
  checkError: null,
  installing: false,
  progress: null,
  installError: null,
};

export const checkStarted = (state: UpdateState): UpdateState => ({
  ...state,
  checking: true,
  upToDate: false,
  checkError: null,
});

/** Null is "nothing newer". */
export const checkAnswered = (state: UpdateState, available: UpdateInfo | null): UpdateState => ({
  ...state,
  available,
  checking: false,
  upToDate: available === null,
});

export const checkFailed = (state: UpdateState, checkError: BackendError): UpdateState => ({
  ...state,
  checking: false,
  checkError,
});

export const foundAtStartup = (state: UpdateState, available: UpdateInfo | null): UpdateState =>
  available === null ? state : { ...state, available };

export const installStarted = (state: UpdateState): UpdateState => ({
  ...state,
  installing: true,
  progress: null,
  installError: null,
});

export const installProgressed = (state: UpdateState, progress: number): UpdateState => ({
  ...state,
  progress,
});

export const installFailed = (state: UpdateState, installError: BackendError): UpdateState => ({
  ...state,
  installing: false,
  progress: null,
  installError,
});
