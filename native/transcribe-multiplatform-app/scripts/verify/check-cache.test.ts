// Runs with bun: vitest for the record of checks that passed.
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, expect, it } from "vitest";
import { digestOf, loadPassed, savePassed } from "./check-cache.ts";

const ROOT: string = join(import.meta.dirname, "../../node_modules/.cache/check-cache-test");
const CHECK = { name: "lint", command: ["biome"], inputs: ["src", "one.txt", "absent"] };

beforeAll(() => {
  rmSync(ROOT, { recursive: true, force: true });
  mkdirSync(join(ROOT, "src/target"), { recursive: true });
  writeFileSync(join(ROOT, "src/a.ts"), "a");
  writeFileSync(join(ROOT, "one.txt"), "1");
});

afterAll(() => {
  rmSync(ROOT, { recursive: true, force: true });
});

it("gives the same digest for the same inputs", () => {
  expect(digestOf(ROOT, CHECK)).toBe(digestOf(ROOT, CHECK));
});

it("changes with the command", () => {
  expect(digestOf(ROOT, { ...CHECK, command: ["tsc"] })).not.toBe(digestOf(ROOT, CHECK));
});

it("changes with a file's contents, and returns when they do", () => {
  const before: string = digestOf(ROOT, CHECK);
  writeFileSync(join(ROOT, "src/a.ts"), "b");
  const changed: string = digestOf(ROOT, CHECK);
  writeFileSync(join(ROOT, "src/a.ts"), "a");
  expect(changed).not.toBe(before);
  expect(digestOf(ROOT, CHECK)).toBe(before);
});

it("changes with a new file", () => {
  const before: string = digestOf(ROOT, CHECK);
  writeFileSync(join(ROOT, "src/b.ts"), "");
  const changed: string = digestOf(ROOT, CHECK);
  rmSync(join(ROOT, "src/b.ts"));
  expect(changed).not.toBe(before);
});

it("ignores build folders", () => {
  const before: string = digestOf(ROOT, CHECK);
  writeFileSync(join(ROOT, "src/target/out"), "x");
  expect(digestOf(ROOT, CHECK)).toBe(before);
});

it("reads back what it saved, in a folder it makes", () => {
  const file: string = join(ROOT, "made/passed.json");
  savePassed(file, { lint: "abc" });
  expect(loadPassed(file)).toStrictEqual({ lint: "abc" });
});

it("treats a missing or broken record as nothing passed", () => {
  expect(loadPassed(join(ROOT, "absent.json"))).toStrictEqual({});
  writeFileSync(join(ROOT, "broken.json"), "{");
  expect(loadPassed(join(ROOT, "broken.json"))).toStrictEqual({});
});
