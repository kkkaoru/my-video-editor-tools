// Runs with bun: how to use the app, in plain words; button names come from the labels the app really shows.
import type { VNode } from "preact";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { format, type HelpPart, type HelpSection, type Messages, type Params } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";

export interface HelpPaneProps {
  version: string;
}

interface HelpPartViewProps {
  part: HelpPart;
  labels: Params;
}

/** AAA line length (WCAG 1.4.8): under 80 Latin characters, or 40 CJK. */
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
});

const HelpPartView = ({ part: { heading, intro, steps, notes }, labels }: HelpPartViewProps): VNode => (
  <div className="grid grid-cols-1 gap-2">
    <h3 className="font-medium">{heading}</h3>
    <p>{format(intro, labels)}</p>
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
      {[
        sections.start,
        sections.transcribe,
        sections.resolve,
        sections.look,
        sections.rewrite,
        sections.trouble,
      ].map(
        ({ heading, parts }: HelpSection): VNode => (
          <Card key={heading}>
            <CardHeader>
              <CardTitle>{heading}</CardTitle>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-4 text-base">
              {parts.map(
                (part: HelpPart): VNode => (
                  <HelpPartView key={part.heading} part={part} labels={labels} />
                ),
              )}
            </CardContent>
          </Card>
        ),
      )}
    </div>
  );
}
