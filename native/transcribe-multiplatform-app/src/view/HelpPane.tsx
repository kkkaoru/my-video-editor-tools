// Runs with bun: how to use the app, in plain words; button names come from the labels the app really shows.
import type { VNode } from "preact";
import { useState } from "preact/hooks";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { Input } from "@/components/ui/input.tsx";
import { format, type HelpPart, type HelpSections, type Messages, type Params } from "../i18n/messages.ts";
import { SPEAKER_FIGURES } from "../i18n/speaker-figures.ts";
import { useMessages } from "../i18n/use-messages.ts";
import {
  figuresOfStep,
  HELP_IMAGE_SIZES,
  type HelpImageName,
  imageOfPart,
  RESOLVE_FIGURE_SIZES,
  type ResolveFigureName,
} from "./help-images.ts";
import { type FoundSection, findHelp, foundPartCount } from "./help-search.ts";

interface HelpPartViewProps {
  id: string;
  part: HelpPart;
  labels: Params;
  image: HelpImageName | undefined;
  figuresOf: (step: number) => ResolveFigureName[];
}

/** AAA line length (WCAG 1.4.8): under 80 Latin characters, or 40 CJK. */
const SECTION_ORDER: (keyof HelpSections)[] = [
  "start",
  "transcribe",
  "editor",
  "look",
  "rewrite",
  "settings",
  "trouble",
];
const LINE_LIMIT: string = "max-w-[min(65ch,40em)] leading-normal";

/** The on-screen names the text quotes, so a renamed button cannot leave the help behind. */
export const helpLabels = ({
  tabJobs,
  tabStyle,
  tabModels,
  pickVideos,
  pickSubtitles,
  speakerCount,
  speakerCountAuto,
  subtitleFormat,
  addModelFolder,
  downloadAllLabel,
  resetModels,
  profileSave,
  fontFamily,
  badgePresent,
  speechLanguage,
  faceModel,
  positionHeading,
  anchorFaceAbove,
  anchorFaceBelow,
  anchorVideoTop,
  anchorVideoBottom,
  profileImport,
  resetLook,
  tabSettings,
  uiLanguageLabel,
  uiThemeLabel,
  uiThemeSystem,
  uiThemeLight,
  uiThemeDark,
  speakerFiguresNote,
  licensesOpen,
  separationBackend,
  memoryUnified,
  updateCheck,
  runWithOneSpeaker,
  updateInstall,
  captionRowSpacing,
  captionRowsLeft,
}: Messages): Params => ({
  ...SPEAKER_FIGURES,
  tabJobs,
  tabStyle,
  tabModels,
  pickVideos,
  pickSubtitles,
  speakerCount,
  speakerCountAuto,
  subtitleFormat,
  addModelFolder,
  downloadAllLabel,
  resetModels,
  profileSave,
  fontFamily,
  badgePresent,
  speechLanguage,
  faceModel,
  positionHeading,
  anchorFaceAbove,
  anchorFaceBelow,
  anchorVideoTop,
  anchorVideoBottom,
  profileImport,
  resetLook,
  tabSettings,
  uiLanguageLabel,
  uiThemeLabel,
  uiThemeSystem,
  uiThemeLight,
  uiThemeDark,
  speakerFiguresNote: format(speakerFiguresNote, SPEAKER_FIGURES),
  licensesOpen,
  separationBackend,
  memoryUnified,
  updateCheck,
  runWithOneSpeaker,
  updateInstall,
  captionRowSpacing,
  captionRowsLeft,
});

/** Lazy, and sized up front: a picture in a closed tab costs nothing, and arriving moves nothing. */
const HelpImage = ({ name }: { name: HelpImageName }): VNode => {
  const { help } = useMessages();
  return (
    <img
      src={`/help/${help.imageFolder}/${name}.webp`}
      alt={help.images[name]}
      width={HELP_IMAGE_SIZES[name].width}
      height={HELP_IMAGE_SIZES[name].height}
      loading="lazy"
      decoding="async"
      className="h-auto w-full rounded-lg border border-border"
    />
  );
};

/** One picture of DaVinci Resolve under its step, the same for every language; its caption says what the numbers mark. */
const ResolveFigure = ({ name }: { name: ResolveFigureName }): VNode => {
  const { help } = useMessages();
  const { width, height } = RESOLVE_FIGURE_SIZES[name];
  return (
    <figure className="mt-2 grid gap-2">
      <img
        src={`/help/resolve/${name}.webp`}
        alt=""
        width={width}
        height={height}
        loading="lazy"
        decoding="async"
        className={`h-auto w-full rounded-lg border border-border ${height > width ? "max-w-sm" : ""}`}
      />
      <figcaption className="text-sm text-muted-foreground">{help.resolveFigures[name]}</figcaption>
    </figure>
  );
};

export const ResolveFigureList = ({ names }: { names: ResolveFigureName[] }): VNode[] =>
  names.map((name: ResolveFigureName): VNode => <ResolveFigure key={name} name={name} />);

const HelpPartView = ({
  id,
  part: { heading, intro, steps, notes },
  labels,
  image,
  figuresOf,
}: HelpPartViewProps): VNode => (
  <div className="grid grid-cols-1 gap-2">
    <h3 id={id} tabIndex={-1} className="scroll-mt-20 font-medium">
      {heading}
    </h3>
    <p>{format(intro, labels)}</p>
    {image === undefined ? null : <HelpImage name={image} />}
    {steps.length === 0 ? null : (
      <ol className={`${LINE_LIMIT} list-decimal pl-6`}>
        {steps.map(
          (step: string, index: number): VNode => (
            <li key={step}>
              {format(step, labels)}
              <ResolveFigureList names={figuresOf(index)} />
            </li>
          ),
        )}
      </ol>
    )}
    {notes.length === 0 ? null : (
      <ul className={`${LINE_LIMIT} list-disc pl-6`}>
        {notes.map(
          (note: string): VNode => (
            <li key={note}>{format(note, labels)}</li>
          ),
        )}
      </ul>
    )}
  </div>
);

const sectionId = (key: keyof HelpSections): string => `help-${key}`;

const partId = (key: keyof HelpSections, index: number): string => `help-${key}-${index}`;

/** Moves there and the keyboard with it; no hash, which would mean nothing to the app. */
const goTo = (id: string, event: Event): void => {
  event.preventDefault();
  const target: HTMLElement | null = document.getElementById(id);
  target?.scrollIntoView();
  target?.focus();
};

const ContentsLink = ({ id, text }: { id: string; text: string }): VNode => (
  <a
    href={`#${id}`}
    onClick={(event: Event): void => goTo(id, event)}
    className="inline-block py-1 underline underline-offset-2"
  >
    {text}
  </a>
);

const HelpContents = ({
  found,
  sections,
  label,
}: {
  found: FoundSection[];
  sections: HelpSections;
  label: string;
}): VNode => (
  <nav aria-label={label}>
    <p className="font-medium">{label}</p>
    <ul className="grid grid-cols-1 gap-x-6 sm:grid-cols-2">
      {found.map(
        ({ key, partIndexes }: FoundSection): VNode => (
          <li key={key}>
            <ContentsLink id={sectionId(key)} text={sections[key].heading} />
            <ul className="pl-4">
              {partIndexes.map(
                (index: number): VNode => (
                  <li key={index}>
                    <ContentsLink id={partId(key, index)} text={sections[key].parts[index]?.heading ?? ""} />
                  </li>
                ),
              )}
            </ul>
          </li>
        ),
      )}
    </ul>
  </nav>
);

export function HelpPane(): VNode {
  const messages: Messages = useMessages();
  const labels: Params = helpLabels(messages);
  const { sections } = messages.help;
  const [query, setQuery] = useState("");
  const found: FoundSection[] = findHelp(query, SECTION_ORDER, sections, labels);
  const searching: boolean = query.trim().length > 0;
  return (
    <div className="grid grid-cols-1 gap-4 text-base break-words">
      <p className={LINE_LIMIT}>{messages.help.intro}</p>
      <div data-slot="help-search" className="sticky top-0 z-10 bg-(--page) py-1">
        <Input
          type="search"
          aria-label={messages.helpSearch}
          placeholder={messages.helpSearch}
          value={query}
          onInput={(event: Event): void => setQuery((event.currentTarget as HTMLInputElement).value)}
        />
      </div>
      <p role="status" className="text-muted-foreground">
        {searching && found.length > 0
          ? format(messages.helpSearchFound, { count: foundPartCount(found) })
          : ""}
        {searching && found.length === 0 ? format(messages.helpSearchNone, { query }) : ""}
      </p>
      {found.length === 0 ? null : (
        <HelpContents found={found} sections={sections} label={messages.helpContents} />
      )}
      {found.map(
        ({ key, partIndexes }: FoundSection): VNode => (
          <Card key={key}>
            <CardHeader>
              <CardTitle id={sectionId(key)} tabIndex={-1} className="scroll-mt-20">
                {sections[key].heading}
              </CardTitle>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-4 text-base">
              {partIndexes.map(
                (index: number): VNode => (
                  <HelpPartView
                    key={index}
                    id={partId(key, index)}
                    part={sections[key].parts[index] as HelpPart}
                    labels={labels}
                    image={imageOfPart(key, index)}
                    figuresOf={(step: number): ResolveFigureName[] => figuresOfStep(key, index, step)}
                  />
                ),
              )}
            </CardContent>
          </Card>
        ),
      )}
    </div>
  );
}
