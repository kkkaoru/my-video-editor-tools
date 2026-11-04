// Runs with bun: rebuilds "Transcribe for Resolve.app" whenever a source changes, and restarts it if it was open.
// The build goes to the one shared target directory, and old intermediates are pruned so rebuilds do not pile up.
import { spawnSync } from "node:child_process";
import { watch } from "node:fs";
import { join } from "node:path";

interface PendingTimer {
  pending: ReturnType<typeof setTimeout> | null;
}

const APP_DIR: string = join(import.meta.dirname, "../..");
const WATCHED: string[] = ["src", "src-tauri/src", "src-tauri/resources", "src-tauri/tauri.conf.json", "index.html"];
const SETTLE_MS: number = 5000;
const APP_NAME: string = "Transcribe for Resolve";
const TEST_FILE: RegExp = /\.test\.tsx?$/;

const isRunning = (): boolean => spawnSync("pgrep", ["-x", "transcribe-app"]).status === 0;

function buildAndRestart(): void {
  const wasRunning: boolean = isRunning();
  const build = spawnSync("bunx", ["tauri", "build", "--bundles", "app"], { cwd: APP_DIR, stdio: "inherit" });
  if (build.status !== 0) {
    console.error("watch-app: the build failed; keeping the previous app");
    return;
  }
  spawnSync("bash", [join(APP_DIR, "scripts/verify/prune-build.sh")], { stdio: "inherit" });
  if (wasRunning) {
    spawnSync("pkill", ["-x", "transcribe-app"]);
    spawnSync("open", ["-g", join(process.env.CARGO_TARGET_DIR ?? "src-tauri/target", "release/bundle/macos", `${APP_NAME}.app`)]);
  }
  console.log(`watch-app: rebuilt at ${new Date().toISOString()}`);
}

/** One rebuild at a time, plus one more if anything changed during it. */
function startWatching(): void {
  const timer: PendingTimer = { pending: null };
  const schedule = (filename: string | null): void => {
    if (filename !== null && TEST_FILE.test(filename)) {
      return;
    }
    if (timer.pending !== null) {
      clearTimeout(timer.pending);
    }
    timer.pending = setTimeout(buildAndRestart, SETTLE_MS);
  };
  WATCHED.forEach((path: string): void => {
    watch(join(APP_DIR, path), { recursive: true }, (_event, filename): void => schedule(filename));
  });
  console.log("watch-app: watching for changes");
}

startWatching();
