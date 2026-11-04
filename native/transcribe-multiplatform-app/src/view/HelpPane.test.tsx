// Runs with bun: the help page quotes real button names, in every language, with a correct outline.
import { cleanup, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { ALL_LANGUAGE_MESSAGES } from "../i18n/language.ts";
import { enMessages } from "../i18n/locale-en.ts";
import { jaMessages } from "../i18n/locale-ja.ts";
import { format, type HelpPart, type HelpSection, type Messages } from "../i18n/messages.ts";
import { MessagesContext } from "../i18n/use-messages.ts";
import { HelpPane, helpLabels } from "./HelpPane.tsx";

const PLACEHOLDER: RegExp = /\{\w+\}/;
const FILE_NAME: RegExp = /Transcribe-for-Resolve-[\w.-]+/g;

const helpTexts = ({ help: { intro, sections } }: Messages): string[] => [
  intro,
  ...Object.values(sections).flatMap(({ heading, parts }: HelpSection): string[] => [
    heading,
    ...parts.flatMap((part: HelpPart): string[] => [part.heading, part.intro, ...part.steps, ...part.notes]),
  ]),
];

const unfilled = (messages: Messages): string[] =>
  helpTexts(messages)
    .map((text: string): string => format(text, helpLabels(messages)))
    .filter((text: string): boolean => PLACEHOLDER.test(text));

/** A long sentence is what makes a page hard for a child to read. */
const tooLong = (messages: Messages, limit: number): string[] =>
  helpTexts(messages)
    // File names are copied, not read as words, so they do not count.
    .flatMap((text: string): string[] =>
      format(text, helpLabels(messages))
        .replace(FILE_NAME, "")
        .split(/(?<=[。!?.!?])\s*/u),
    )
    .filter((sentence: string): boolean => sentence.length > limit);

afterEach(cleanup);

it("fills every quoted button name, in every language", () => {
  expect(ALL_LANGUAGE_MESSAGES.flatMap(unfilled)).toStrictEqual([]);
});

it("keeps Japanese sentences short", () => {
  expect(tooLong(jaMessages, 60)).toStrictEqual([]);
});

it("explains the words a beginner does not know the first time they appear in Japanese", () => {
  expect(jaMessages.help.sections.start.parts[1]?.intro).toBe(
    "モデルは、文字起こしに使うデータです。文字起こしは、話している言葉を文字にすることです。はじめに一度だけ入れます。",
  );
  expect(jaMessages.help.sections.transcribe.parts[2]?.notes[0]).toBe(
    ".fcpxml: DaVinci Resolve で読める、字幕入りのファイルです。",
  );
});

it("opens with the version, then the six topics as second-level headings", () => {
  const { container } = render(<HelpPane version="1.2.3" />);
  expect(container.querySelector("p")?.textContent).toBe("Version 1.2.3");
  expect(
    screen
      .getAllByRole("heading", { level: 2 })
      .map((heading: HTMLElement): string | null => heading.textContent),
  ).toStrictEqual([
    "First steps",
    "Make subtitles from a video",
    "Open the subtitles in DaVinci Resolve",
    "Change how subtitles look and where they sit",
    "Give finished subtitles a new look",
    "If something goes wrong",
  ]);
  expect(screen.getByRole("heading", { level: 3, name: "Start with the FCPXML file" }).tagName).toBe("H3");
});

it("numbers the steps and quotes the real button names", () => {
  render(<HelpPane version="1.2.3" />);
  const steps: HTMLElement | undefined = screen
    .getAllByRole("list")
    .find((list: HTMLElement): boolean => list.tagName === "OL");
  expect(
    Array.from(steps?.children ?? []).map((step: Element): string | null => step.textContent),
  ).toStrictEqual([
    "Choose the file for your computer from the list below.",
    "On a Mac, open the zip and move the app to the Applications folder.",
    "On Windows, open the exe and follow the steps.",
  ]);
  expect(
    Array.from(
      screen.getAllByRole("list").filter((list: HTMLElement): boolean => list.tagName === "OL")[1]
        ?.children ?? [],
    ).map((step: Element): string | null => step.textContent),
  ).toStrictEqual([
    "Open the “Models” tab.",
    "Press “Download what is missing”.",
    "Wait until each model's bar reaches the end.",
  ]);
  expect(enMessages.downloadAllLabel).toBe("Download what is missing");
});

it("gives the import path for DaVinci Resolve", () => {
  render(<HelpPane version="1.2.3" />);
  expect(screen.getByText("Choose File > Import > Timeline in the menu.").tagName).toBe("LI");
});

it("draws no empty lists for a topic with only a list of notes or steps", () => {
  render(
    <MessagesContext.Provider value={jaMessages}>
      <HelpPane version="1.2.3" />
    </MessagesContext.Provider>,
  );
  expect(screen.getByText("バージョン 1.2.3").tagName).toBe("P");
  expect(
    screen.getAllByRole("list").filter((list: HTMLElement): boolean => list.children.length === 0).length,
  ).toBe(0);
});

const listSizes = ({ help: { sections } }: Messages): string =>
  Object.values(sections)
    .flatMap(({ parts }: HelpSection): string[] =>
      parts.map(({ steps, notes }: HelpPart): string => `${steps.length}/${notes.length}`),
    )
    .join(" ");

it("gives every language the same number of topics, steps and notes", () => {
  expect(new Set(ALL_LANGUAGE_MESSAGES.map(listSizes))).toStrictEqual(
    new Set(["3/6 3/4 3/1 3/6 0/4 0/4 4/1 0/3 4/0 3/0 3/2 4/4 3/0 3/2 3/3 2/0"]),
  );
});

it("quotes the place choices as the style tab shows them", () => {
  render(<HelpPane version="1.2.3" />);
  expect(
    screen.getByText(
      "Under “Caption position”, choose “Above the face”, “Below the face”, “Top of the video” or “Bottom of the video”.",
    ).tagName,
  ).toBe("LI");
});

it("keeps the opening text within the readable line length", () => {
  render(<HelpPane version="1.2.3" />);
  expect(
    screen.getByText(
      "This app turns the words spoken in a video into subtitles, with a different colour for each person. You can use the subtitles in DaVinci Resolve, a video editing program.",
    ).className,
  ).toBe("max-w-[min(65ch,40em)] leading-normal");
});
