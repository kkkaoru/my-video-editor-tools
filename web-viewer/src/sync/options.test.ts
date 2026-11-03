import { expect, it } from "vitest";
import { parseOptions } from "./options.ts";

it("defaults to a full sync", () => {
  expect(parseOptions([])).toStrictEqual({ dryRun: false, dataOnly: false, originalsOnly: false });
});

it("reads every flag", () => {
  expect(parseOptions(["--dry-run", "--data-only", "--originals-only"])).toStrictEqual({
    dryRun: true,
    dataOnly: true,
    originalsOnly: true,
  });
});

it("reads the video ID of --only", () => {
  expect(parseOptions(["--only", "a-review", "--dry-run"])).toStrictEqual({
    dryRun: true,
    dataOnly: false,
    originalsOnly: false,
    only: "a-review",
  });
});

it("rejects unknown flags", () => {
  expect(() => parseOptions(["--nope"])).toThrow();
});

it("reads the volume wait in minutes", () => {
  expect(parseOptions(["--volume-wait-minutes", "15"])).toStrictEqual({
    dryRun: false,
    dataOnly: false,
    originalsOnly: false,
    volumeWaitMinutes: 15,
  });
});

it("rejects a volume wait that is not a number", () => {
  expect(() => parseOptions(["--volume-wait-minutes", "soon"])).toThrow(
    "--volume-wait-minutes must be a non-negative number, got soon",
  );
});

it("rejects a negative volume wait", () => {
  expect(() => parseOptions(["--volume-wait-minutes=-1"])).toThrow(
    "--volume-wait-minutes must be a non-negative number, got -1",
  );
});

it("reads how long to wait for another sync's lock", () => {
  expect(parseOptions(["--lock-wait-minutes", "5"]).lockWaitMinutes).toBe(5);
  expect(parseOptions([]).lockWaitMinutes).toBeUndefined();
  expect(() => parseOptions(["--lock-wait-minutes=-1"])).toThrow(
    "--lock-wait-minutes must be a non-negative number, got -1",
  );
});
