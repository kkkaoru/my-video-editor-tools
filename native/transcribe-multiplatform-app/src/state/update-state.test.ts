// Runs with bun: the update's pure transitions.
import { expect, it } from "vitest";
import type { BackendError, UpdateInfo } from "../backend/types.ts";
import {
  checkAnswered,
  checkFailed,
  checkStarted,
  foundAtStartup,
  INITIAL_UPDATE,
  installFailed,
  installProgressed,
  installStarted,
  type UpdateState,
} from "./update-state.ts";

const INFO: UpdateInfo = { version: "2.0.0", notes: "New" };
const FAILURE: BackendError = { code: "internal", detail: "x" };

it("clears the last answer when a check starts", () => {
  const failed: UpdateState = { ...INITIAL_UPDATE, upToDate: true, checkError: FAILURE };
  expect(checkStarted(failed)).toStrictEqual({ ...INITIAL_UPDATE, checking: true });
});

it("says up to date only for no newer version", () => {
  expect(checkAnswered(checkStarted(INITIAL_UPDATE), null)).toStrictEqual({
    ...INITIAL_UPDATE,
    upToDate: true,
  });
  expect(checkAnswered(checkStarted(INITIAL_UPDATE), INFO)).toStrictEqual({
    ...INITIAL_UPDATE,
    available: INFO,
  });
});

it("takes back an offer when a later check finds nothing newer", () => {
  expect(checkAnswered({ ...INITIAL_UPDATE, available: INFO }, null).available).toBe(null);
});

it("keeps a failed check's error and any offer", () => {
  expect(checkFailed({ ...INITIAL_UPDATE, available: INFO, checking: true }, FAILURE)).toStrictEqual({
    ...INITIAL_UPDATE,
    available: INFO,
    checkError: FAILURE,
  });
});

it("offers what the startup check found, and says nothing otherwise", () => {
  expect(foundAtStartup(INITIAL_UPDATE, INFO).available).toStrictEqual(INFO);
  expect(foundAtStartup(INITIAL_UPDATE, null)).toStrictEqual(INITIAL_UPDATE);
});

it("clears the last failure when an install starts, and follows its progress", () => {
  const started: UpdateState = installStarted({ ...INITIAL_UPDATE, installError: FAILURE });
  expect(started).toStrictEqual({ ...INITIAL_UPDATE, installing: true });
  expect(installProgressed(started, 0.5).progress).toBe(0.5);
});

it("stops installing on a failure, ready to try again", () => {
  const running: UpdateState = installProgressed(installStarted(INITIAL_UPDATE), 0.5);
  expect(installFailed(running, FAILURE)).toStrictEqual({ ...INITIAL_UPDATE, installError: FAILURE });
});
