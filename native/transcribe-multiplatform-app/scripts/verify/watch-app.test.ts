// Runs with bun: when the watcher may rebuild the app.
import { expect, it } from "vitest";
import { mayBuild } from "./watch-app.ts";

it("builds a newer source while the app is closed", () => {
  expect(mayBuild(2, 1, false)).toBe(true);
});

it("waits while the app is open, because a bundle in use must not be replaced", () => {
  expect(mayBuild(2, 1, true)).toBe(false);
});

it("does nothing when no source is newer, so a build cannot start another", () => {
  expect(mayBuild(1, 1, false)).toBe(false);
});
