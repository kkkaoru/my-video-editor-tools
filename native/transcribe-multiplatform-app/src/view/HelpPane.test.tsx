// Runs with bun: the help page quotes real button names, in every language, with a correct outline.
import { cleanup, render, screen } from "@testing-library/preact";
import { afterEach, expect, it } from "vitest";
import { ALL_LANGUAGE_MESSAGES } from "../i18n/language.ts";
import { enMessages } from "../i18n/locale-en.ts";
import { jaMessages } from "../i18n/locale-ja.ts";
import { format, type HelpPart, type HelpSection, type Messages } from "../i18n/messages.ts";
import { MessagesContext } from "../i18n/use-messages.ts";
import { HelpPane, helpLabels, ResolveFigureList } from "./HelpPane.tsx";

const PLACEHOLDER: RegExp = /\{\w+\}/;
const FILE_NAME: RegExp = /Transcribe-Captions-[\w.-]+/g;

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
  expect(jaMessages.help.sections.transcribe.parts[2]?.notes[1]).toBe(
    ".fcpxml: 動画編集ソフトで読める、字幕入りのファイルです。",
  );
});

it("has the seven topics as second-level headings", () => {
  render(<HelpPane />);
  expect(
    screen
      .getAllByRole("heading", { level: 2 })
      .map((heading: HTMLElement): string | null => heading.textContent),
  ).toStrictEqual([
    "First steps",
    "Make subtitles from a video",
    "Open the subtitles in your video editor",
    "Change how subtitles look and where they sit",
    "Give finished subtitles a new look",
    "Settings and updates",
    "If something goes wrong",
  ]);
  expect(screen.getByRole("heading", { level: 3, name: "Bring in an FCPXML file" }).tagName).toBe("H3");
});

it("numbers the steps and quotes the real button names", () => {
  render(<HelpPane />);
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

it("gives the import path without naming any editor", () => {
  render(<HelpPane />);
  expect(
    screen.getByText("Bring in the timeline from the menu (for example, File > Import > Timeline).").tagName,
  ).toBe("LI");
});

it("draws no empty lists for a topic with only a list of notes or steps", () => {
  render(
    <MessagesContext.Provider value={jaMessages}>
      <HelpPane />
    </MessagesContext.Provider>,
  );
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
    new Set(["3/6 3/4 3/1 4/7 0/4 0/5 0/4 5/3 4/1 0/3 4/2 3/0 3/2 3/2 3/2 4/5 3/1 3/2 3/0 0/2 3/2 3/3 2/0"]),
  );
});

it("quotes the place choices as the style tab shows them", () => {
  render(<HelpPane />);
  expect(
    screen.getByText(
      "Under “Caption position”, choose “Above the face”, “Below the face”, “Top of the video” or “Bottom of the video”.",
    ).tagName,
  ).toBe("LI");
});

it("keeps the opening text within the readable line length", () => {
  render(<HelpPane />);
  expect(
    screen.getByText(
      "This app turns the words spoken in a video into subtitles, with a different colour for each person. You can use the subtitles in video editing software.",
    ).className,
  ).toBe("max-w-[min(65ch,40em)] leading-normal");
});

it("says the write-again button keeps the old files", () => {
  render(<HelpPane />);
  expect(
    screen.getByText(
      "The write-again button next to a video's name in the list makes new files with a different name. The old files are kept.",
    ).tagName,
  ).toBe("LI");
});

it("shows each annotated picture lazily, sized, in this language, and described", () => {
  render(
    <MessagesContext.Provider value={jaMessages}>
      <HelpPane />
    </MessagesContext.Provider>,
  );
  const picture: HTMLElement = screen.getByAltText(
    "「モデル」のタブの画面です。1 はタブ、2 はダウンロードのボタン、3 はモデルごとのバーです。",
  );
  expect(picture.getAttribute("src")).toBe("/help/ja/models.webp");
  expect(picture.getAttribute("loading")).toBe("lazy");
  expect(picture.getAttribute("width")).toBe("1100");
  expect(picture.getAttribute("height")).toBe("1000");
});

it("has a picture for each topic with numbered marks", () => {
  render(<HelpPane />);
  expect(
    screen.getAllByRole("img").map((picture: HTMLElement): string | null => picture.getAttribute("src")),
  ).toStrictEqual([
    "/help/en/models.webp",
    "/help/en/transcribe.webp",
    "/help/en/look.webp",
    "/help/en/font.webp",
    "/help/en/profiles.webp",
    "/help/en/rewrite.webp",
  ]);
});

it("says what a picture's marks point at, by the step numbers of the text", () => {
  expect(jaMessages.help.sections.start.parts[1]?.steps.length).toBe(3);
  expect(jaMessages.help.images.models).toBe(
    "「モデル」のタブの画面です。1 はタブ、2 はダウンロードのボタン、3 はモデルごとのバーです。",
  );
});

it("explains why the number of speakers cannot be chosen", () => {
  render(<HelpPane />);
  expect(
    screen.getByRole("heading", { level: 3, name: "Subtitles for 2 or more speakers cannot start" }).tagName,
  ).toBe("H3");
  expect(
    screen.getByText(
      "On the “Models” tab, download the missing models. If they do not fit this computer, press “Run with 1 speaker”.",
    ).tagName,
  ).toBe("LI");
  expect(
    screen.getByText("With 1 speaker, these models are not needed, and everyone gets the same colours.")
      .tagName,
  ).toBe("LI");
});

it("captions a DaVinci Resolve picture in the language, from the one folder", () => {
  const { container } = render(<ResolveFigureList names={["menu"]} />);
  expect(container.querySelector("img")?.getAttribute("src")).toBe("/help/resolve/menu.webp");
  expect(container.querySelector("figcaption")?.textContent).toBe(
    "Choose File, then Import, then Timeline. 1 is File, 2 is Import, 3 is Timeline.",
  );
});

it("shows each DaVinci Resolve picture under the step it belongs to", () => {
  const { container } = render(<HelpPane />);
  const stepsOfFigures = Array.from(
    container.querySelectorAll("figure img"),
    (image: Element): string | null => image.getAttribute("src"),
  );
  expect(stepsOfFigures).toStrictEqual([
    "/help/resolve/menu.webp",
    "/help/resolve/dialog.webp",
    "/help/resolve/timeline.webp",
    "/help/resolve/text.webp",
    "/help/resolve/shading.webp",
  ]);
  expect(container.querySelector('img[src="/help/resolve/text.webp"]')?.className.includes("max-w-sm")).toBe(
    true,
  );
  expect(container.querySelector('img[src="/help/resolve/menu.webp"]')?.className.includes("max-w-sm")).toBe(
    false,
  );
});

it("quotes the speaker figures from their one place", () => {
  render(<HelpPane />);
  expect(
    screen.getByText(
      "Whisper on the CPU took about 14 min on one video of about an hour, with no passages redone. These figures were measured on one Mac, and vary by video and computer. RAM and GPU memory on Windows have not been measured.",
    ).tagName,
  ).toBe("LI");
  expect(
    screen.getByText(
      "Nemotron 3.5 encoder: per hour of audio, about 4 seconds on a Mac's GPU, about 33 seconds on the CPU. Voice separation, measured on one video of about an hour: about 5 min on Core ML, about 14 min on the CPU. It changes with how much of the video it handles.",
    ).tagName,
  ).toBe("LI");
});

it("says Nemotron only tells speakers apart, and Whisper does the transcribing", () => {
  render(<HelpPane />);
  expect(
    screen.getByText(
      "With automatic detection or 2 or more speakers, Whisper turbo, the Nemotron 3.5 encoder and MossFormer2 run. Whisper turbo always does the transcribing; Nemotron only tells speakers apart.",
    ).tagName,
  ).toBe("P");
});
