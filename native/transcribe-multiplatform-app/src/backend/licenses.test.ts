// Runs with bun: the license list loads only when it is what it should be.
import { expect, it, vi } from "vitest";
import { loadLicenses } from "./licenses.ts";

it("loads the generated list from its script", async () => {
  const load = vi.fn().mockResolvedValue({ default: { entries: [], texts: ["MIT"] } });
  await expect(loadLicenses(load)).resolves.toStrictEqual({ entries: [], texts: ["MIT"] });
  expect(load.mock.calls).toStrictEqual([["/licenses.js"]]);
});

it("fails when the script is missing", async () => {
  await expect(loadLicenses()).rejects.toThrow();
});

it("fails on anything else", async () => {
  const wrong = vi.fn().mockResolvedValue({ default: { entries: "none", texts: [] } });
  await expect(loadLicenses(wrong)).rejects.toThrow("/licenses.js: not a license list");
  const empty = vi.fn().mockResolvedValue({ default: null });
  await expect(loadLicenses(empty)).rejects.toThrow("/licenses.js: not a license list");
});
