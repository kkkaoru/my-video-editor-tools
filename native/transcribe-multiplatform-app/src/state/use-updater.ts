// Runs with bun: asks once at startup for a new version, and checks and installs when asked.
import { useCallback, useEffect, useState } from "preact/hooks";
import { checkUpdate, installUpdate, onUpdateProgress } from "../backend/backend.ts";
import type { UpdateInfo } from "../backend/types.ts";
import {
  checkAnswered,
  checkFailed,
  checkStarted,
  foundAtStartup,
  INITIAL_UPDATE,
  installFailed,
  installProgressed,
  installStarted,
  type UpdateState,
} from "./update-state.ts";
import { asBackendError } from "./use-app-store.ts";

export interface Updater {
  state: UpdateState;
  check: () => void;
  install: () => void;
}

export function useUpdater(): Updater {
  const [state, setState] = useState<UpdateState>(INITIAL_UPDATE);
  useEffect(() => {
    checkUpdate()
      .then((available: UpdateInfo | null): void => setState((now) => foundAtStartup(now, available)))
      // Offline is normal: the person did not ask, so a failure here only goes to the log.
      .catch((error: unknown): void => console.warn("update check at startup failed", error));
    const stop = onUpdateProgress((progress: number): void =>
      setState((now) => installProgressed(now, progress)),
    );
    return (): void => {
      stop
        .then((unlisten): void => unlisten())
        .catch((error: unknown): void => {
          console.warn("stopping the update progress failed", error);
        });
    };
  }, []);
  const check = useCallback((): void => {
    setState(checkStarted);
    checkUpdate()
      .then((available: UpdateInfo | null): void => setState((now) => checkAnswered(now, available)))
      .catch((error: unknown): void => setState((now) => checkFailed(now, asBackendError(error))));
  }, []);
  // Success restarts the app, so only a failure comes back here.
  const install = useCallback((): void => {
    setState(installStarted);
    installUpdate().catch((error: unknown): void =>
      setState((now) => installFailed(now, asBackendError(error))),
    );
  }, []);
  return { state, check, install };
}
