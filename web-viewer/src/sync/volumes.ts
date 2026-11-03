// Runs with bun. External-volume guard for the sync: a configured library path that cannot be read
// (its SSD under /Volumes/<name> is unplugged, directly or through a symlink such as
// ../youtube-originals) stalls the sync until it is back, so a missing volume is never published
// as "those videos were removed". build_catalog.py fails on a missing path anyway, so guarding
// every configured path only turns that failure into a bounded wait.
import { resolve } from "node:path";
import type { FileSystem, Logger } from "./ports.ts";

export interface RootsWait {
  fs: FileSystem;
  logger: Logger;
  sleep: (milliseconds: number) => Promise<void>;
  roots: string[];
  budgetMs: number;
}

interface Poll {
  wait: RootsWait;
  waitedMs: number;
  reported: string;
}

const MILLISECONDS_PER_SECOND = 1000;
export const VOLUME_POLL_MS = 30_000;

// The configured paths resolved against the project, without duplicates.
export const libraryRoots = (projectDirectory: string, paths: string[]): string[] => [
  ...new Set(paths.map((path) => resolve(projectDirectory, path))),
];

// Why a root cannot be read now, or null when it can. A vanishing volume answers stat with EIO
// (or similar) instead of ENOENT, which counts as missing too.
async function rootProblem(fs: FileSystem, root: string): Promise<string | null> {
  try {
    const stat = await fs.stat(root);
    return stat === null || stat.isFile ? `${root} (missing)` : null;
  } catch (error) {
    return `${root} (${error instanceof Error ? error.message : String(error)})`;
  }
}

export async function missingRoots(fs: FileSystem, roots: string[]): Promise<string[]> {
  const problems = await Promise.all(roots.map((root) => rootProblem(fs, root)));
  return problems.filter((problem) => problem !== null);
}

export async function requireRoots(fs: FileSystem, roots: string[]): Promise<void> {
  const missing = await missingRoots(fs, roots);
  if (missing.length > 0) {
    throw new Error(`library path(s) missing: ${missing.join(", ")}`);
  }
}

const waitedText = (milliseconds: number): string =>
  `${Math.round(milliseconds / MILLISECONDS_PER_SECOND)} s`;

function reportChange({ wait, waitedMs, reported }: Poll, summary: string): void {
  if (summary === reported) {
    return;
  }
  wait.logger.info(
    summary === ""
      ? `library path(s) back after ${waitedText(waitedMs)}`
      : `waiting for library path(s): ${summary}`,
  );
}

async function poll(state: Poll): Promise<number> {
  const summary = (await missingRoots(state.wait.fs, state.wait.roots)).join(", ");
  reportChange(state, summary);
  if (summary === "") {
    return state.waitedMs;
  }
  if (state.waitedMs >= state.wait.budgetMs) {
    throw new Error(
      `library path(s) still missing after ${waitedText(state.waitedMs)}: ${summary}`,
    );
  }
  await state.wait.sleep(VOLUME_POLL_MS);
  return poll({ wait: state.wait, waitedMs: state.waitedMs + VOLUME_POLL_MS, reported: summary });
}

// Polls until every root is readable; resolves with the milliseconds spent waiting, rejects once
// the budget is used up. Logs only when the set of missing roots changes.
export const waitForRoots = (wait: RootsWait): Promise<number> =>
  poll({ wait, waitedMs: 0, reported: "" });
