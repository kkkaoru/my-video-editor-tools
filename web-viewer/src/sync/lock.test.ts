// Runs with bun (vitest). The sync lock, on the fake file system.
import { expect, it } from "vitest";
import { createFakeFs, type FakeFsHandle } from "./fakes.fake.ts";
import { type LockServices, withSyncLock } from "./lock.ts";

const LOCK = "/p/.sync.lock";
const HELD = '{"pid":7,"startedAt":"2026-11-03T09:00:00.000Z"}\n';

interface Harness {
  handle: FakeFsHandle;
  services: LockServices;
  logs: string[];
  slept: number[];
}

function harness(options: {
  initial: Record<string, { text: string }>;
  alive: (pid: number) => boolean;
  onSleep: (handle: FakeFsHandle) => void;
}): Harness {
  const handle = createFakeFs(options.initial);
  const logs: string[] = [];
  const slept: number[] = [];
  const clock = { now: Date.parse("2026-11-03T10:00:00.000Z") };
  const services: LockServices = {
    fs: handle.fs,
    logger: { info: (message) => logs.push(message) },
    sleep: async (milliseconds) => {
      slept.push(milliseconds);
      clock.now += milliseconds;
      options.onSleep(handle);
    },
    now: () => clock.now,
    pid: 42,
    alive: options.alive,
  };
  return { handle, services, logs, slept };
}

it("holds the lock while the work runs and releases it afterwards, even on failure", async () => {
  const { handle, services } = harness({ initial: {}, alive: () => true, onSleep: () => {} });
  const seen = await withSyncLock(
    { services, path: LOCK, waitMinutes: 60 },
    async () => handle.files.get(LOCK)?.text,
  );
  expect(seen).toBe('{"pid":42,"startedAt":"2026-11-03T10:00:00.000Z"}\n');
  expect(handle.files.has(LOCK)).toBe(false);
  await expect(
    withSyncLock({ services, path: LOCK, waitMinutes: 60 }, async () => {
      throw new Error("sync failed");
    }),
  ).rejects.toThrow("sync failed");
  expect(handle.files.has(LOCK)).toBe(false);
});

it("makes a second sync wait for a running one, then go ahead", async () => {
  const sleeps = { count: 0 };
  const { services, logs, slept } = harness({
    initial: { [LOCK]: { text: HELD } },
    alive: () => true,
    // The running sync finishes during the second wait.
    onSleep: (fake) => {
      sleeps.count += 1;
      if (sleeps.count === 2) fake.files.delete(LOCK);
    },
  });
  const ran = await withSyncLock({ services, path: LOCK, waitMinutes: 60 }, async () => "ran");
  expect([ran, slept, logs]).toStrictEqual([
    "ran",
    [15000, 15000],
    ["sync lock: waiting for pid 7 (since 2026-11-03T09:00:00.000Z)"],
  ]);
});

it("takes over a lock whose process is gone, or that names no holder", async () => {
  const dead = harness({
    initial: { [LOCK]: { text: HELD } },
    alive: () => false,
    onSleep: () => {},
  });
  expect(
    await withSyncLock({ services: dead.services, path: LOCK, waitMinutes: 60 }, async () => "ran"),
  ).toBe("ran");
  expect(dead.logs).toStrictEqual([
    `sync lock: taking over ${LOCK} from a process that is gone ({"pid":7,"startedAt":"2026-11-03T09:00:00.000Z"})`,
  ]);
  const garbled = harness({
    initial: { [LOCK]: { text: "not json" } },
    alive: () => true,
    onSleep: () => {},
  });
  expect(
    await withSyncLock(
      { services: garbled.services, path: LOCK, waitMinutes: 60 },
      async () => "ran",
    ),
  ).toBe("ran");
  const empty = harness({
    initial: { [LOCK]: { text: "null" } },
    alive: () => true,
    onSleep: () => {},
  });
  expect(
    await withSyncLock(
      { services: empty.services, path: LOCK, waitMinutes: 60 },
      async () => "ran",
    ),
  ).toBe("ran");
});

it("gives up after the wait, naming the holder", async () => {
  const { services, slept } = harness({
    initial: { [LOCK]: { text: HELD } },
    alive: () => true,
    onSleep: () => {},
  });
  await expect(
    withSyncLock({ services, path: LOCK, waitMinutes: 1 }, async () => "ran"),
  ).rejects.toThrow(
    "Another sync holds /p/.sync.lock (pid 7, since 2026-11-03T09:00:00.000Z); gave up after 1 min",
  );
  expect(slept).toStrictEqual([15000, 15000, 15000, 15000]);
});

it("leaves a lock that another waiter took over before the stale one was removed", async () => {
  const { handle, services } = harness({
    initial: { [LOCK]: { text: HELD } },
    alive: (pid) => pid !== 7,
    onSleep: () => {},
  });
  // Between this waiter reading the dead holder and removing it, another took the lock.
  const read = handle.fs.readText;
  const reads = { count: 0 };
  handle.fs.readText = async (path) => {
    reads.count += 1;
    if (reads.count === 2)
      handle.files.set(LOCK, {
        ...handle.files.get(LOCK),
        size: 1,
        mtimeNs: 1n,
        sha: "",
        text: '{"pid":9,"startedAt":"x"}\n',
      });
    return read(path);
  };
  await expect(
    withSyncLock({ services, path: LOCK, waitMinutes: 0 }, async () => "ran"),
  ).rejects.toThrow("(pid 9, since x)");
});

it("tries again at once when the holder's lock vanishes before it is read", async () => {
  const { handle, services } = harness({
    initial: { [LOCK]: { text: HELD } },
    alive: () => true,
    onSleep: () => {},
  });
  const stat = handle.fs.stat;
  handle.fs.stat = async (path) => {
    handle.files.delete(LOCK);
    return stat(path);
  };
  expect(await withSyncLock({ services, path: LOCK, waitMinutes: 60 }, async () => "ran")).toBe(
    "ran",
  );
});
