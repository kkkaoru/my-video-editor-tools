// Runs with bun: what a search of the help page finds.
import { expect, it } from "vitest";
import type { HelpPart, HelpSections } from "../i18n/messages.ts";
import { type FoundSection, findHelp, foundPartCount } from "./help-search.ts";

const part = (heading: string, intro: string, steps: string[] = [], notes: string[] = []): HelpPart => ({
  heading,
  intro,
  steps,
  notes,
});

const ORDER: (keyof HelpSections)[] = ["start", "editor"];
const SECTIONS = {
  start: { heading: "First", parts: [part("Install", "Get the {tab} file"), part("Models", "Download")] },
  editor: { heading: "Editors", parts: [part("CapCut", "Open it", ["Press ＯＫ"], ["No shadow"])] },
} as unknown as HelpSections;
const LABELS = { tab: "Jobs" };

const find = (query: string): FoundSection[] => findHelp(query, ORDER, SECTIONS, LABELS);

it("finds everything for an empty search", () => {
  expect(find("  ")).toStrictEqual([
    { key: "start", partIndexes: [0, 1] },
    { key: "editor", partIndexes: [0] },
  ]);
});

it("finds a part by any of its texts, whatever the case", () => {
  expect(find("capcut")).toStrictEqual([{ key: "editor", partIndexes: [0] }]);
  expect(find("SHADOW")).toStrictEqual([{ key: "editor", partIndexes: [0] }]);
  expect(find("download")).toStrictEqual([{ key: "start", partIndexes: [1] }]);
});

it("searches the words the page shows, with its labels filled in", () => {
  expect(find("jobs")).toStrictEqual([{ key: "start", partIndexes: [0] }]);
  expect(find("{tab}")).toStrictEqual([]);
});

it("treats full-width and half-width letters alike", () => {
  expect(find("ok")).toStrictEqual([{ key: "editor", partIndexes: [0] }]);
});

it("needs every word in one part", () => {
  expect(find("open shadow")).toStrictEqual([{ key: "editor", partIndexes: [0] }]);
  expect(find("open download")).toStrictEqual([]);
});

it("finds all of a section's parts by its heading", () => {
  expect(find("first")).toStrictEqual([{ key: "start", partIndexes: [0, 1] }]);
});

it("counts the parts found", () => {
  expect(foundPartCount(find(""))).toBe(3);
  expect(foundPartCount([])).toBe(0);
});
