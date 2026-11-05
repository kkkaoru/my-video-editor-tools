// Runs with bun: how to use the app, in plain words; button names come from the labels the app really shows.
import type { VNode } from "preact";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { format, type HelpPart, type HelpSections, type Messages, type Params } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import { HELP_IMAGE_SIZES, type HelpImageName, imageOfPart } from "./help-images.ts";

export interface HelpPaneProps {
  version: string;
}

interface HelpPartViewProps {
  part: HelpPart;
  labels: Params;
  image: HelpImageName | undefined;
}

/** AAA line length (WCAG 1.4.8): under 80 Latin characters, or 40 CJK. */
const SECTION_ORDER: (keyof HelpSections)[] = ["start", "transcribe", "editor", "look", "rewrite", "trouble"];
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
}: Messages): Params => ({
  tabJobs,
  tabStyle,
  tabModels,
  pickVideos,
  pickSubtitles,
  speakerCount,
  speakerCountAuto,
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

const HelpPartView = ({
  part: { heading, intro, steps, notes },
  labels,
  image,
}: HelpPartViewProps): VNode => (
  <div className="grid grid-cols-1 gap-2">
    <h3 className="font-medium">{heading}</h3>
    <p>{format(intro, labels)}</p>
    {image === undefined ? null : <HelpImage name={image} />}
    {steps.length === 0 ? null : (
      <ol className={`${LINE_LIMIT} list-decimal pl-6`}>
        {steps.map(
          (step: string): VNode => (
            <li key={step}>{format(step, labels)}</li>
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

export function HelpPane({ version }: HelpPaneProps): VNode {
  const messages: Messages = useMessages();
  const labels: Params = helpLabels(messages);
  const { sections } = messages.help;
  return (
    <div className="grid grid-cols-1 gap-4 text-base break-words">
      <p className="text-muted-foreground">{format(messages.help.version, { version })}</p>
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
