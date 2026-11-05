// Runs with bun: rebuilds the .app whenever a source changes, then reopens a running app on the new build once no job is running and nobody is using it.
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

export interface AppActivity {
  jobRunning: boolean;
  frontmost: boolean;
  idleSeconds: number;
}

interface RelaunchState {
  waiting: boolean;
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
const APP_NAME: string = "Transcribe Captions";
const APP_EXECUTABLE: string = `${APP_NAME}.app/Contents/MacOS`;
const BUNDLE_PATH: RegExp = /^.*\.app(?=\/Contents\/MacOS)/;
const FRONT_PID: RegExp = /"pid"=(\d+)/;
const HID_IDLE_NANOSECONDS: RegExp = /"HIDIdleTime" = (\d+)/;
const NANOSECONDS_PER_SECOND: number = 1e9;
const MIN_IDLE_SECONDS: number = 20;
const RECHECK_MS: number = 5000;
const STAGE_GAP_SECONDS: string = "2";
const QUIT_WAIT_SECONDS: string = "4";

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

const run = (command: string, ...args: string[]): string =>
  spawnSync(command, args, { encoding: "utf8" }).stdout.trim();

const appPid = (): string => run("pgrep", "-f", APP_EXECUTABLE).split("\n")[0];

const hasChildren = (pid: string): boolean => run("pgrep", "-P", pid) !== "";

/** Stages start one tool after another, so a job counts as finished only when two looks apart find no tool. */
function jobRunning(pid: string): boolean {
  if (hasChildren(pid)) {
    return true;
  }
  spawnSync("sleep", [STAGE_GAP_SECONDS]);
  return hasChildren(pid);
}

const frontPid = (): string =>
  FRONT_PID.exec(run("lsappinfo", "info", "-only", "pid", run("lsappinfo", "front")))?.[1] ?? "";

const idleSeconds = (): number =>
  Number(HID_IDLE_NANOSECONDS.exec(run("ioreg", "-c", "IOHIDSystem"))?.[1] ?? 0) / NANOSECONDS_PER_SECOND;

/** Never during a job; while the app is in front, only after the person has stopped touching the Mac. */
export const mayRelaunch = ({ jobRunning: running, frontmost, idleSeconds: idle }: AppActivity): boolean =>
  !running && (!frontmost || idle >= MIN_IDLE_SECONDS);

// A reopened app stays behind what the person is doing; only an app already in front comes back in front.
// shortcut: a model download runs inside the app with no child process, so a reopen stops it; this tool only serves the developer's Mac.
function reopen(pid: string, frontmost: boolean): void {
  const bundle: string = BUNDLE_PATH.exec(run("ps", "-o", "command=", "-p", pid))?.[0] ?? "";
  spawnSync("osascript", ["-e", `quit app "${APP_NAME}"`]);
  spawnSync("sleep", [QUIT_WAIT_SECONDS]);
  if (appPid() === "" && bundle !== "") {
    spawnSync("open", frontmost ? [bundle] : ["-g", bundle]);
    console.log(`watch-app: reopened at ${new Date().toISOString()}`);
    return;
  }
  console.error("watch-app: the app did not quit or its path is unknown; leaving it as it is");
}

/** Does nothing when the app is closed, because the next launch uses the new build anyway. */
function reopenWhenFree(state: RelaunchState): void {
  const pid: string = appPid();
  if (pid === "") {
    state.waiting = false;
    return;
  }
  const frontmost: boolean = frontPid() === pid;
  if (mayRelaunch({ jobRunning: jobRunning(pid), frontmost, idleSeconds: idleSeconds() })) {
    state.waiting = false;
    reopen(pid, frontmost);
    return;
  }
  setTimeout((): void => reopenWhenFree(state), RECHECK_MS);
}

function requestReopen(state: RelaunchState): void {
  if (!state.waiting) {
    state.waiting = true;
    reopenWhenFree(state);
  }
}

function rebuild(relaunch: RelaunchState): void {
  const build = spawnSync("bunx", ["tauri", "build", "--bundles", "app"], { cwd: APP_DIR, stdio: "inherit" });
  spawnSync("bash", [join(APP_DIR, "scripts/verify/prune-build.sh")], { stdio: "inherit" });
  if (build.status !== 0) {
    console.error("watch-app: the build failed; keeping the previous app");
    return;
  }
  console.log(`watch-app: rebuilt at ${new Date().toISOString()}`);
  requestReopen(relaunch);
}

/** One rebuild at a time, plus one more if anything changed during it. */
function startWatching(): void {
  const timer: PendingTimer = { pending: null };
  const built: BuiltState = { newest: 0 };
  const relaunch: RelaunchState = { waiting: false };
  // The build itself fires watch events on these folders, so only a newer file means a change.
  const rebuildIfChanged = (): void => {
    const newest: number = newestSource();
    if (newest > built.newest) {
      built.newest = newest;
      rebuild(relaunch);
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
