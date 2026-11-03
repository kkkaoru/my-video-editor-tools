// Runs with bun (vitest). The deploy's caption font step, on fakes.
import { expect, it } from "vitest";
import { FONT_SOURCE_SHA256, prepareCaptionFont } from "./captionFont.ts";
import { createFakeFs, createFakeRunner, fakeFile } from "./fakes.fake.ts";

const SOURCE = "/fonts/NotoSansJP-VariableFont_wght.ttf";
const TARGET = "/p/public/fonts/NotoSansJP-VariableFont_wght.woff2";
const STAMP = "/p/cache/caption-font.json";

it("builds the WOFF2 from the pinned TTF and records what it was built from", async () => {
  const { fs, files } = createFakeFs({ [SOURCE]: { sha: FONT_SOURCE_SHA256 } });
  const { runner, calls } = createFakeRunner((call) => {
    files.set(call.args.at(-1) ?? "", fakeFile({ sha: "w1" }));
    return {};
  });
  const outcome = await prepareCaptionFont({ fs, runner, projectDirectory: "/p", source: SOURCE });
  expect(outcome).toBe("built");
  expect(
    calls.map((call) => [call.command, call.args.slice(0, 5), call.args.slice(6), call.cwd]),
  ).toStrictEqual([
    ["uv", ["run", "--no-sync", "python", "-I", "-c"], [SOURCE, `${TARGET}.partial`], "/"],
  ]);
  expect(files.get(TARGET)?.sha).toBe("w1");
  expect(JSON.parse(files.get(STAMP)?.text ?? "{}")).toStrictEqual({
    source: FONT_SOURCE_SHA256,
    woff2: "w1",
  });
});

it("reuses a WOFF2 the stamp says was built from the pinned TTF", async () => {
  const { fs } = createFakeFs({
    [SOURCE]: { sha: FONT_SOURCE_SHA256 },
    [TARGET]: { sha: "w1" },
    [STAMP]: { text: `{"source":"${FONT_SOURCE_SHA256}","woff2":"w1"}` },
  });
  const { runner, calls } = createFakeRunner(() => ({}));
  expect(await prepareCaptionFont({ fs, runner, projectDirectory: "/p", source: SOURCE })).toBe(
    "reused",
  );
  expect(calls).toStrictEqual([]);
});

it("rebuilds when the WOFF2 no longer matches its stamp", async () => {
  const { fs } = createFakeFs({
    [SOURCE]: { sha: FONT_SOURCE_SHA256 },
    [TARGET]: { sha: "edited" },
    [STAMP]: { text: `{"source":"${FONT_SOURCE_SHA256}","woff2":"w1"}` },
  });
  const { runner, calls } = createFakeRunner(() => ({}));
  expect(await prepareCaptionFont({ fs, runner, projectDirectory: "/p", source: SOURCE })).toBe(
    "built",
  );
  expect(calls.length).toBe(1);
});

it("fails the deploy without the pinned TTF or when conversion fails", async () => {
  const { runner } = createFakeRunner(() => ({ code: 1, stderr: "no brotli" }));
  const missing = createFakeFs({});
  await expect(
    prepareCaptionFont({ fs: missing.fs, runner, projectDirectory: "/p", source: SOURCE }),
  ).rejects.toThrow("Caption font not found: /fonts/NotoSansJP-VariableFont_wght.ttf");
  const other = createFakeFs({ [SOURCE]: { sha: "other" } });
  await expect(
    prepareCaptionFont({ fs: other.fs, runner, projectDirectory: "/p", source: SOURCE }),
  ).rejects.toThrow("has SHA-256 other");
  const failing = createFakeFs({ [SOURCE]: { sha: FONT_SOURCE_SHA256 } });
  await expect(
    prepareCaptionFont({ fs: failing.fs, runner, projectDirectory: "/p", source: SOURCE }),
  ).rejects.toThrow("Converting the caption font failed: no brotli");
  expect(failing.calls.at(-1)).toBe(`remove ${TARGET}.partial`);
});
