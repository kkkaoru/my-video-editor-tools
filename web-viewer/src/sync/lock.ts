// Runs with bun. One sync at a time: every sync (dry runs included) builds into the shared
// data.next and records sync-state.json, so a second one waits for the lock file, which names
// its holder. A lock whose process is gone is stale and taken over.
import { z } from "zod";
import type { FileSystem, Logger } from "./ports.ts";

export const LOCK_FILE = ".sync.lock";
export const DEFAULT_LOCK_WAIT_MINUTES = 60;
const POLL_MS = 15_000;
const MINUTE_MS = 60_000;

export interface LockServices {
  fs: FileSystem;
  logger: Logger;
  sleep: (milliseconds: number) => Promise<void>;
  // Wall-clock milliseconds, this process's id, and whether a process id is still running.
  now: () => number;
  pid: number;
  alive: (pid: number) => boolean;
}

export interface LockRequest {
  services: LockServices;
  path: string;
  waitMinutes: number;
}

const holderSchema = z.object({ pid: z.number().int(), startedAt: z.string() });
type Holder = z.infer<typeof holderSchema>;

// The live holder a lock names; null when it names none (unreadable), which makes it stale.
function holderOf(text: string): Holder | null {
  try {
    const parsed = holderSchema.safeParse(JSON.parse(text));
    return parsed.success ? parsed.data : null;
  } catch {
    return null;
  }
}

// Removes a stale lock only while it still holds what was read, so a lock that another waiter
// took over in the meantime stays. shortcut: the re-read and the removal are two steps; a
// takeover landing exactly between them is not excluded (two waiters on one dead holder).
async function removeStale(request: LockRequest, text: string): Promise<void> {
  const { fs, logger } = request.services;
  if ((await fs.stat(request.path)) === null || (await fs.readText(request.path)) !== text) {
    return;
  }
  logger.info(
    `sync lock: taking over ${request.path} from a process that is gone (${text.trim()})`,
  );
  await fs.removeTree(request.path);
}

async function acquire(request: LockRequest): Promise<void> {
  const { fs, logger, sleep, now, pid, alive } = request.services;
  const deadline = now() + request.waitMinutes * MINUTE_MS;
  const record = `${JSON.stringify({ pid, startedAt: new Date(now()).toISOString() })}\n`;
  const reported = new Set<string>();
  for (;;) {
    if (await fs.createExclusive({ path: request.path, text: record, mode: 0o600 })) {
      return;
    }
    // A lock that vanished since the attempt is simply tried again.
    if ((await fs.stat(request.path)) === null) {
      continue;
    }
    const text = await fs.readText(request.path);
    const holder = holderOf(text);
    if (holder === null || !alive(holder.pid)) {
      await removeStale(request, text);
      continue;
    }
    if (now() >= deadline) {
      throw new Error(
        `Another sync holds ${request.path} (pid ${holder.pid}, since ${holder.startedAt}); gave up after ${request.waitMinutes} min`,
      );
    }
    if (!reported.has(text)) {
      reported.add(text);
      logger.info(`sync lock: waiting for pid ${holder.pid} (since ${holder.startedAt})`);
    }
    await sleep(POLL_MS);
  }
}

// Runs `work` holding the lock and releases it afterwards, whether `work` succeeds or fails.
export async function withSyncLock<Result>(
  request: LockRequest,
  work: () => Promise<Result>,
): Promise<Result> {
  await acquire(request);
  try {
    return await work();
  } finally {
    await request.services.fs.removeTree(request.path);
  }
}
