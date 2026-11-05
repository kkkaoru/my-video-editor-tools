// Runs with bun: rebuilds the .app whenever a source changes. A running app is left alone, so a job is never killed; the next launch uses the new build.
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
const SETTLE_MS: number = 5000;
const TEST_FILE: RegExp = /\.test\.tsx?$/;

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

function rebuild(): void {
  const build = spawnSync("bunx", ["tauri", "build", "--bundles", "app"], { cwd: APP_DIR, stdio: "inherit" });
  spawnSync("bash", [join(APP_DIR, "scripts/verify/prune-build.sh")], { stdio: "inherit" });
  if (build.status !== 0) {
    console.error("watch-app: the build failed; keeping the previous app");
    return;
  }
  console.log(`watch-app: rebuilt at ${new Date().toISOString()}`);
}

/** One rebuild at a time, plus one more if anything changed during it. */
function startWatching(): void {
  const timer: PendingTimer = { pending: null };
  const built: BuiltState = { newest: 0 };
  // The build itself fires watch events on these folders, so only a newer file means a change.
  const rebuildIfChanged = (): void => {
    const newest: number = newestSource();
    if (newest > built.newest) {
      built.newest = newest;
      rebuild();
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

startWatching();
