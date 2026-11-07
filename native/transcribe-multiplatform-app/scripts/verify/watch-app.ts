// Runs with bun: rebuilds the .app after a source has settled, and only while the app is closed.
// It never quits or opens the app: replacing a bundle in use can crash it, and the next launch uses the new build.
// The build goes to the one shared target directory, and old intermediates are pruned so rebuilds do not pile up.
import { spawnSync } from "node:child_process";
import { readdirSync, statSync, watch } from "node:fs";
import { join } from "node:path";

interface PendingTimer {
  pending: ReturnType<typeof setTimeout> | null;
}

interface BuiltState {
  newest: number;
}

const APP_DIR: string = join(import.meta.dirname, "../..");
const WATCHED: string[] = [
  "src",
  "public",
  "src-tauri/src",
  "src-tauri/resources",
  "src-tauri/tauri.conf.json",
  "index.html",
];
const SETTLE_MS: number = 30_000;
const TEST_FILE: RegExp = /\.test\.tsx?$/;
// The name, not the whole command line: a shell that mentions the path would count as the app.
const APP_PROCESS: string = "transcribe-app";
const RECHECK_MS: number = 30_000;

const modifiedAt = (path: string): number => statSync(join(APP_DIR, path)).mtimeMs;

const newestIn = (path: string): number =>
  statSync(join(APP_DIR, path)).isDirectory()
    ? Math.max(
        ...readdirSync(join(APP_DIR, path), { recursive: true, encoding: "utf8" }).map(
          (file: string): number => modifiedAt(join(path, file)),
        ),
      )
    : modifiedAt(path);

const newestSource = (): number => Math.max(...WATCHED.map(newestIn));

const appIsOpen = (): boolean =>
  spawnSync("pgrep", ["-x", APP_PROCESS], { encoding: "utf8" }).stdout.trim() !== "";

/** Only a newer source is a change, because the build itself fires watch events on these folders. */
export const mayBuild = (newest: number, built: number, appOpen: boolean): boolean =>
  newest > built && !appOpen;

function rebuild(): void {
  const build = spawnSync("bunx", ["tauri", "build", "--bundles", "app"], { cwd: APP_DIR, stdio: "inherit" });
  spawnSync("bash", [join(APP_DIR, "scripts/verify/prune-build.sh")], { stdio: "inherit" });
  console.log(
    build.status === 0
      ? `watch-app: rebuilt at ${new Date().toISOString()}`
      : "watch-app: the build failed; keeping the previous app",
  );
}

/** One rebuild at a time, plus one more if anything changed during it. */
function startWatching(): void {
  const timer: PendingTimer = { pending: null };
  const built: BuiltState = { newest: 0 };
  const rebuildIfChanged = (): void => {
    const newest: number = newestSource();
    if (mayBuild(newest, built.newest, appIsOpen())) {
      built.newest = newest;
      rebuild();
    } else if (newest > built.newest) {
      timer.pending = setTimeout(rebuildIfChanged, RECHECK_MS);
    }
  };
  const schedule = (filename: string | null): void => {
    if (filename !== null && TEST_FILE.test(filename)) {
      return;
    }
    if (timer.pending !== null) {
      clearTimeout(timer.pending);
    }
    timer.pending = setTimeout(rebuildIfChanged, SETTLE_MS);
  };
  WATCHED.forEach((path: string): void => {
    watch(join(APP_DIR, path), { recursive: true }, (_event, filename): void => schedule(filename));
  });
  console.log("watch-app: watching for changes");
}

if (import.meta.main) {
  startWatching();
}
