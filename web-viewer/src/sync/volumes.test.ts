import { expect, it } from "vitest";
import { createFakeFs, fakeFile } from "./fakes.fake.ts";
import { libraryRoots, missingRoots, requireRoots, waitForRoots } from "./volumes.ts";

it("resolves the configured paths against the project, without duplicates", () => {
  expect(
    libraryRoots("/p", ["../edits", "/Volumes/ssd/movie", "/Volumes/ssd/movie/", "/Users/x/lib"]),
  ).toStrictEqual(["/edits", "/Volumes/ssd/movie", "/Users/x/lib"]);
});

it("reports a root that is absent, a file, or unreadable", async () => {
  const { fs } = createFakeFs({ "/Volumes/a/lib/x.mp4": {}, "/Volumes/b/file": {} });
  const failing = {
    ...fs,
    stat: async (path: string) =>
      path === "/Volumes/c/lib" ? Promise.reject(new Error("EIO: i/o error")) : fs.stat(path),
  };
  expect(
    await missingRoots(failing, [
      "/Volumes/a/lib",
      "/Volumes/b/file",
      "/Volumes/c/lib",
      "/Volumes/d",
    ]),
  ).toStrictEqual([
    "/Volumes/b/file (missing)",
    "/Volumes/c/lib (EIO: i/o error)",
    "/Volumes/d (missing)",
  ]);
});

it("reports a thrown value that is not an Error", async () => {
  const { fs } = createFakeFs({});
  const failing = { ...fs, stat: async () => Promise.reject("gone") };
  expect(await missingRoots(failing, ["/Volumes/a"])).toStrictEqual(["/Volumes/a (gone)"]);
});

it("requires every root", async () => {
  const { fs } = createFakeFs({ "/Volumes/a/lib/x.mp4": {} });
  await expect(requireRoots(fs, ["/Volumes/a/lib"])).resolves.toBeUndefined();
  await expect(requireRoots(fs, ["/Volumes/b/lib"])).rejects.toThrow(
    "library path(s) missing: /Volumes/b/lib (missing)",
  );
});

it("returns at once without logging when the roots are there", async () => {
  const { fs } = createFakeFs({ "/Volumes/a/lib/x.mp4": {} });
  const logs: string[] = [];
  const sleeps: number[] = [];
  const waited = await waitForRoots({
    fs,
    logger: { info: (message) => logs.push(message) },
    sleep: async (milliseconds) => {
      sleeps.push(milliseconds);
    },
    roots: ["/Volumes/a/lib"],
    budgetMs: 0,
  });
  expect(waited).toBe(0);
  expect(logs).toStrictEqual([]);
  expect(sleeps).toStrictEqual([]);
});

it("waits until the volume is back, logging once per change", async () => {
  const { fs, files } = createFakeFs({});
  const logs: string[] = [];
  const sleeps: number[] = [];
  const waited = await waitForRoots({
    fs,
    logger: { info: (message) => logs.push(message) },
    sleep: async (milliseconds) => {
      sleeps.push(milliseconds);
      if (sleeps.length === 3) {
        files.set("/Volumes/a/lib/x.mp4", fakeFile({}));
      }
    },
    roots: ["/Volumes/a/lib"],
    budgetMs: 600_000,
  });
  expect(waited).toBe(90_000);
  expect(sleeps).toStrictEqual([30_000, 30_000, 30_000]);
  expect(logs).toStrictEqual([
    "waiting for library path(s): /Volumes/a/lib (missing)",
    "library path(s) back after 90 s",
  ]);
});

it("logs again when the set of missing roots changes", async () => {
  const { fs, files } = createFakeFs({});
  const logs: string[] = [];
  const sleeps: number[] = [];
  await waitForRoots({
    fs,
    logger: { info: (message) => logs.push(message) },
    sleep: async (milliseconds) => {
      sleeps.push(milliseconds);
      files.set(`/Volumes/${sleeps.length === 1 ? "a" : "b"}/lib/x.mp4`, fakeFile({}));
    },
    roots: ["/Volumes/a/lib", "/Volumes/b/lib"],
    budgetMs: 600_000,
  });
  expect(logs).toStrictEqual([
    "waiting for library path(s): /Volumes/a/lib (missing), /Volumes/b/lib (missing)",
    "waiting for library path(s): /Volumes/b/lib (missing)",
    "library path(s) back after 60 s",
  ]);
});

it("fails with the missing roots once the budget is used up", async () => {
  const { fs } = createFakeFs({});
  const sleeps: number[] = [];
  await expect(
    waitForRoots({
      fs,
      logger: { info: () => undefined },
      sleep: async (milliseconds) => {
        sleeps.push(milliseconds);
      },
      roots: ["/Volumes/a/lib"],
      budgetMs: 60_000,
    }),
  ).rejects.toThrow("library path(s) still missing after 60 s: /Volumes/a/lib (missing)");
  expect(sleeps).toStrictEqual([30_000, 30_000]);
});
