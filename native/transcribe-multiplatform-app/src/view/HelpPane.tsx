// Runs with bun: how to use the app, in plain words; button names come from the labels the app really shows.
import { ExternalLinkIcon } from "lucide-preact";
import type { VNode } from "preact";
import { Button } from "@/components/ui/button.tsx";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { openHelpLink } from "../backend/backend.ts";
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

interface HelpPartViewProps {
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

/** The address stays on the page, so a browser that does not open still leaves a way there. */
const openInBrowser = (url: string): void => {
  openHelpLink(url).catch((error: unknown): void => {
    console.error(`opening ${url} failed`, error);
  });
};

const HelpPartView = ({
  part: { heading, intro, steps, notes, link },
  labels,
  image,
  figuresOf,
}: HelpPartViewProps): VNode => (
  <div className="grid grid-cols-1 gap-2">
    <h3 className="font-medium">{heading}</h3>
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
    {link === undefined ? null : (
      <p className="grid justify-items-start gap-1">
        <Button variant="outline" onClick={(): void => openInBrowser(link.url)}>
          {link.label}
          <ExternalLinkIcon aria-hidden="true" />
        </Button>
        <span className="text-sm break-all text-muted-foreground">{link.url}</span>
      </p>
    )}
  </div>
);

export function HelpPane(): VNode {
  const messages: Messages = useMessages();
  const labels: Params = helpLabels(messages);
  const { sections } = messages.help;
  return (
    <div className="grid grid-cols-1 gap-4 text-base break-words">
      <p className={LINE_LIMIT}>{messages.help.intro}</p>
      {SECTION_ORDER.map(
        (key: keyof HelpSections): VNode => (
          <Card key={key}>
            <CardHeader>
              <CardTitle>{sections[key].heading}</CardTitle>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-4 text-base">
              {sections[key].parts.map(
                (part: HelpPart, index: number): VNode => (
                  <HelpPartView
                    key={part.heading}
                    part={part}
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
