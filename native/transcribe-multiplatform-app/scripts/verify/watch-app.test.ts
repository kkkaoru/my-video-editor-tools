// Runs with bun: when the watcher may reopen the running app on a new build.
import { expect, it } from "vitest";
import { mayRelaunch } from "./watch-app.ts";

it("never reopens during a job, even with nobody at the Mac", () => {
  expect(mayRelaunch({ jobRunning: true, frontmost: false, idleSeconds: 600 })).toBe(false);
});

it("waits while the person is using the app", () => {
  expect(mayRelaunch({ jobRunning: false, frontmost: true, idleSeconds: 3 })).toBe(false);
});

it("reopens the front app once the person has stopped", () => {
  expect(mayRelaunch({ jobRunning: false, frontmost: true, idleSeconds: 20 })).toBe(true);
});

it("reopens at once when the app is in the background", () => {
  expect(mayRelaunch({ jobRunning: false, frontmost: false, idleSeconds: 0 })).toBe(true);
});
