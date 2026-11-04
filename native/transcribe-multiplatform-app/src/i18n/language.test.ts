// Runs with bun: locale resolution, and every translation matching the English shape and placeholders.
import { expect, it } from "vitest";
import { ALL_LANGUAGE_MESSAGES, LANGUAGES, languageFor } from "./language.ts";
import { enMessages } from "./locale-en.ts";
import type { Messages } from "./messages.ts";

const PLACEHOLDER: RegExp = /\{\w+\}/g;

const entriesOf = (messages: object, prefix: string): [string, string][] =>
  Object.entries(messages).flatMap(([key, value]: [string, unknown]): [string, string][] =>
    typeof value === "object" && value !== null
      ? entriesOf(value, `${prefix}${key}.`)
      : [[`${prefix}${key}`, String(value)]],
  );

const placeholdersOf = (text: string): string =>
  Array.from(new Set(text.match(PLACEHOLDER)))
    .sort()
    .join(" ");

const pathsOf = (messages: Messages): string =>
  entriesOf(messages, "")
    .map(([path]: [string, string]): string => path)
    .sort()
    .join(" ");

const placeholderMismatches = (messages: Messages, position: number): string[] =>
  entriesOf(enMessages, "")
    .filter(
      ([path, english]: [string, string]): boolean =>
        placeholdersOf(english) !== placeholdersOf(new Map(entriesOf(messages, "")).get(path) ?? ""),
    )
    .map(([path]: [string, string]): string => `${position}:${path}`);

it("resolves a plain language", () => {
  expect(languageFor("ja").tag).toBe("ja");
});

it("resolves a region with a hyphen", () => {
  expect(languageFor("ja-JP").tag).toBe("ja");
});

it("resolves a POSIX locale", () => {
  expect(languageFor("de_DE.UTF-8").tag).toBe("de");
});

it("ignores case in the language", () => {
  expect(languageFor("FR-fr").tag).toBe("fr");
});

it("resolves simplified Chinese", () => {
  expect(languageFor("zh-CN").tag).toBe("zh-Hans");
});

it("resolves each traditional Chinese form", () => {
  expect(languageFor("zh-Hant-TW").tag).toBe("zh-Hant");
  expect(languageFor("zh-TW").tag).toBe("zh-Hant");
  expect(languageFor("zh-hk").tag).toBe("zh-Hant");
  expect(languageFor("zh-MO").tag).toBe("zh-Hant");
});

it("falls back to English for an unknown language", () => {
  expect(languageFor("sv-SE").tag).toBe("en");
  expect(languageFor("C").tag).toBe("en");
  expect(languageFor("").tag).toBe("en");
});

it("uses the matching messages", () => {
  expect(languageFor("ko").messages.appName).toBe(LANGUAGES.get("ko")?.messages.appName);
  expect(languageFor("en").messages.tabModels).toBe("Models");
});

it("offers eight languages", () => {
  expect(ALL_LANGUAGE_MESSAGES.length).toBe(8);
});

it("reads nested keys when comparing shapes", () => {
  expect(entriesOf({ top: "a", nested: { inner: "b" } }, "")).toStrictEqual([
    ["top", "a"],
    ["nested.inner", "b"],
  ]);
});

it("gives every language the same keys", () => {
  expect(new Set(ALL_LANGUAGE_MESSAGES.map(pathsOf)).size).toBe(1);
});

it("keeps every placeholder of the English text in each language", () => {
  expect(ALL_LANGUAGE_MESSAGES.flatMap(placeholderMismatches)).toStrictEqual([]);
});

it("notices a dropped placeholder", () => {
  expect(placeholderMismatches({ ...enMessages, jobCancel: "Cancel" }, 0)).toStrictEqual(["0:jobCancel"]);
});

it("notices a placeholder the English text does not have", () => {
  expect(placeholderMismatches({ ...enMessages, jobQueued: "{name}" }, 3)).toStrictEqual(["3:jobQueued"]);
});
