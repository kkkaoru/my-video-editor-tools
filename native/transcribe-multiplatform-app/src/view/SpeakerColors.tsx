// Runs with bun: one short row per speaker; opening a row edits that speaker's layers, in the same form for each.
import { ChevronDownIcon } from "lucide-preact";
import type { VNode } from "preact";
import { useState } from "preact/hooks";
import { Accordion, AccordionItem, AccordionPanel, AccordionTrigger } from "@/components/ui/accordion.tsx";
import { Alert } from "@/components/ui/alert.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible.tsx";
import type { SpeakerColor, SubtitleFormat } from "../backend/types.ts";
import { format, type Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import type { FrameSize } from "../layout/preview-layout.ts";
import { contrastRatio, meetsAa } from "../styling/contrast.ts";
import { ColorPicker } from "./ColorPicker.tsx";
import { FormatLimit } from "./FormatLimit.tsx";
import { LayerAdjust, whenPresent } from "./fields.tsx";
import { LayeredText } from "./LayeredText.tsx";

export interface SpeakerColorsProps {
  palette: SpeakerColor[];
  fontFamily: string;
  subtitleFormat: SubtitleFormat;
  /** The row being edited, the unknown speaker's coming after every numbered one. */
  openSlot: number | null;
  onOpenSlotChange: (slot: number | null) => void;
  onChange: (palette: SpeakerColor[]) => void;
}

interface SlotProps {
  slot: number;
  color: SpeakerColor;
  name: string;
  fontFamily: string;
  onChange: (color: SpeakerColor) => void;
}

interface LayerRowProps {
  label: string;
  /** Without one, the colour's place stays empty so every slider lines up. */
  picker?: VNode;
  adjust?: VNode;
}

/** The speaker models tell eight apart; a further colour would never be used. */
const MAX_PALETTE_SIZE: number = 8;
/** Layer limits, as the backend checks them. */
const FILL_WIDTH_MAX: number = 0.2;
const OUTLINE_WIDTH_MAX: number = 0.5;
const RIM_WIDTH_MAX: number = 0.2;
const SHADOW_OFFSET_MAX: number = 0.2;
const SHADOW_BLUR_MAX: number = 50;
const LAYER_WIDTH_SCALE: number = 100;
const LAYER_WIDTH_STEP: number = 0.5;
/** Shown as the percent of the text the shadow actually moves: twice the stored value. */
const SHADOW_OFFSET_SCALE: number = 200;
const SHADOW_OFFSET_STEP: number = 1;
const SHADOW_BLUR_STEP: number = 1;
const PERCENT_SIGN: string = "%";
/** The same columns in every row: the label, the colour, the slider, the number and its unit. */
const LAYER_GRID: string =
  "grid grid-cols-[2.75rem_minmax(0,1fr)_5rem_1rem] items-center gap-2 sm:grid-cols-[8rem_2.75rem_minmax(0,1fr)_5rem_1rem]";
const NO_SHADOW: Pick<SpeakerColor, "shadow" | "shadow_x" | "shadow_y" | "shadow_blur"> = {
  shadow: null,
  shadow_x: 0.03,
  shadow_y: -0.03,
  shadow_blur: 5,
};
const FIRST_SPEAKER: SpeakerColor = {
  fill: "#FFFFFF",
  fill_width: 0.025,
  outline: "#000000",
  outline_width: 0.16,
  rim: null,
  rim_width: 3 / 88,
  ...NO_SHADOW,
};
/** What a speaker after the first starts from: a white outline, and the outer edge transparent. */
const LATER_SPEAKER: Omit<SpeakerColor, "fill"> = {
  fill_width: 0.025,
  outline: "#FFFFFF",
  outline_width: 0.16,
  rim: null,
  rim_width: 3 / 88,
  ...NO_SHADOW,
};
const NEW_SLOT_FILL: string = "#FFFFFF";
const FIRST_COLOURS: string[] = ["#005A9C", "#6A1FA8", "#0B672C", "#A3123A", "#8A4500", "#7A1D7A", "#006262"];
const SAMPLE_FONT_PX: number = 22;
const SAMPLE_SIZE: FrameSize = { width: 64, height: 32 };

/** Speaker 1 is white on a black outline; the others keep a colour on a white outline; nobody starts with an outer edge. */
export const defaultPalette = (): SpeakerColor[] => [
  FIRST_SPEAKER,
  ...FIRST_COLOURS.map((fill: string): SpeakerColor => ({ ...LATER_SPEAKER, fill })),
];

/** Each input names itself; this says at a glance which column is which, at any width. */
const LayerColumnHeads = (): VNode => {
  const messages: Messages = useMessages();
  return (
    <div data-slot="layer-heads" className={`${LAYER_GRID} text-sm text-muted-foreground`} aria-hidden="true">
      <span className="hidden sm:block" />
      <span>{messages.colorColumn}</span>
      <span className="col-span-3">{messages.layerWidth}</span>
    </div>
  );
};

const LayerRow = ({ label, picker, adjust }: LayerRowProps): VNode => (
  <div data-slot="layer-row" className={LAYER_GRID}>
    <span className="col-span-full text-sm font-medium sm:col-span-1">{label}</span>
    {picker ?? <span className="hidden sm:block" />}
    {adjust}
  </div>
);

function SpeakerSlot({ slot, color, name, fontFamily, onChange }: SlotProps): VNode {
  const messages: Messages = useMessages();
  const ratio: number = contrastRatio(color.fill, color.outline);
  const noShadow: boolean = color.shadow === null;
  // Speaker 1 always shows it; a shadow in use is never hidden, wherever it came from.
  const shadowAlwaysShown: boolean = slot === 0 || !noShadow;
  const [shadowExpanded, setShadowExpanded] = useState<boolean>(false);
  const shadowShown: boolean = shadowAlwaysShown || shadowExpanded;
  return (
    <AccordionItem value={String(slot)}>
      <AccordionTrigger>
        {/* The name stays on one line: a narrow row shrinks the space around it instead. */}
        <span data-slot="speaker-name" className="flex-1 whitespace-nowrap">
          {name}
        </span>
        <svg
          aria-hidden="true"
          className="shrink-0 rounded-md bg-[linear-gradient(#a9d8ef_50%,#1f2530_50%)]"
          viewBox={`0 0 ${SAMPLE_SIZE.width} ${SAMPLE_SIZE.height}`}
          width={SAMPLE_SIZE.width}
          height={SAMPLE_SIZE.height}
        >
          <LayeredText
            lines={["Aあ"]}
            x={SAMPLE_SIZE.width / 2}
            y={SAMPLE_SIZE.height / 2}
            fontFamily={fontFamily}
            fontPx={SAMPLE_FONT_PX}
            lineHeightPx={SAMPLE_FONT_PX}
            color={color}
            strokeUnitPx={SAMPLE_FONT_PX}
            frame={SAMPLE_SIZE}
          />
        </svg>
      </AccordionTrigger>
      <AccordionPanel className="grid gap-2 pt-2">
        <LayerColumnHeads />
        <LayerRow
          label={messages.fillLabel}
          picker={
            <ColorPicker
              label={`${name}: ${messages.fillLabel}`}
              value={color.fill}
              allowTransparent={false}
              onChange={(next: string | null): void =>
                whenPresent(next, (fill: string): void => onChange({ ...color, fill }))
              }
            />
          }
          adjust={
            <LayerAdjust
              label={`${name}: ${messages.fillWidth}`}
              value={color.fill_width}
              min={0}
              max={FILL_WIDTH_MAX}
              scale={LAYER_WIDTH_SCALE}
              step={LAYER_WIDTH_STEP}
              unit={PERCENT_SIGN}
              disabled={false}
              onChange={(fill_width: number): void => onChange({ ...color, fill_width })}
            />
          }
        />
        <LayerRow
          label={messages.outlineLabel}
          picker={
            <ColorPicker
              label={`${name}: ${messages.outlineLabel}`}
              value={color.outline}
              allowTransparent={false}
              onChange={(next: string | null): void =>
                whenPresent(next, (outline: string): void => onChange({ ...color, outline }))
              }
            />
          }
          adjust={
            <LayerAdjust
              label={`${name}: ${messages.outlineWidth}`}
              value={color.outline_width}
              min={0}
              max={OUTLINE_WIDTH_MAX}
              scale={LAYER_WIDTH_SCALE}
              step={LAYER_WIDTH_STEP}
              unit={PERCENT_SIGN}
              disabled={false}
              onChange={(outline_width: number): void => onChange({ ...color, outline_width })}
            />
          }
        />
        <LayerRow
          label={messages.rimLabel}
          picker={
            <ColorPicker
              label={`${name}: ${messages.rimLabel}`}
              value={color.rim}
              allowTransparent={true}
              onChange={(rim: string | null): void => onChange({ ...color, rim })}
            />
          }
          adjust={
            <LayerAdjust
              label={`${name}: ${messages.rimWidth}`}
              value={color.rim_width}
              min={0}
              max={RIM_WIDTH_MAX}
              scale={LAYER_WIDTH_SCALE}
              step={LAYER_WIDTH_STEP}
              unit={PERCENT_SIGN}
              disabled={color.rim === null}
              onChange={(rim_width: number): void => onChange({ ...color, rim_width })}
            />
          }
        />
        <Collapsible open={shadowShown} onOpenChange={setShadowExpanded}>
          {shadowAlwaysShown ? (
            <span className="flex min-h-11 items-center text-sm font-medium">{messages.shadowLabel}</span>
          ) : (
            <CollapsibleTrigger
              aria-label={`${name}: ${messages.shadowLabel}`}
              className="group flex min-h-11 cursor-pointer items-center gap-2 rounded-md text-sm font-medium focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-ring"
            >
              {messages.shadowLabel}
              <ChevronDownIcon
                className="size-4 shrink-0 text-muted-foreground group-data-[panel-open]:rotate-180"
                aria-hidden="true"
              />
            </CollapsibleTrigger>
          )}
          <CollapsibleContent
            role="group"
            aria-label={`${name}: ${messages.shadowLabel}`}
            className="-mx-2 grid gap-2 rounded-lg p-2 ring-1 ring-border"
          >
            <LayerRow
              label={messages.shadowColor}
              picker={
                <ColorPicker
                  label={`${name}: ${messages.shadowColor}`}
                  value={color.shadow}
                  allowTransparent={true}
                  onChange={(shadow: string | null): void => {
                    setShadowExpanded(true);
                    onChange({ ...color, shadow });
                  }}
                />
              }
            />
            <LayerRow
              label={messages.shadowXShort}
              adjust={
                <LayerAdjust
                  wide
                  label={`${name}: ${messages.shadowX}`}
                  value={color.shadow_x}
                  min={-SHADOW_OFFSET_MAX}
                  max={SHADOW_OFFSET_MAX}
                  scale={SHADOW_OFFSET_SCALE}
                  step={SHADOW_OFFSET_STEP}
                  unit={PERCENT_SIGN}
                  disabled={noShadow}
                  onChange={(shadow_x: number): void => onChange({ ...color, shadow_x })}
                />
              }
            />
            <LayerRow
              label={messages.shadowYShort}
              adjust={
                <LayerAdjust
                  wide
                  label={`${name}: ${messages.shadowY}`}
                  value={color.shadow_y}
                  min={-SHADOW_OFFSET_MAX}
                  max={SHADOW_OFFSET_MAX}
                  scale={SHADOW_OFFSET_SCALE}
                  step={SHADOW_OFFSET_STEP}
                  unit={PERCENT_SIGN}
                  disabled={noShadow}
                  onChange={(shadow_y: number): void => onChange({ ...color, shadow_y })}
                />
              }
            />
            <LayerRow
              label={messages.shadowBlurShort}
              adjust={
                <LayerAdjust
                  wide
                  label={`${name}: ${messages.shadowBlur}`}
                  value={color.shadow_blur}
                  min={0}
                  max={SHADOW_BLUR_MAX}
                  scale={1}
                  step={SHADOW_BLUR_STEP}
                  unit=""
                  disabled={noShadow}
                  onChange={(shadow_blur: number): void => onChange({ ...color, shadow_blur })}
                />
              }
            />
          </CollapsibleContent>
        </Collapsible>
      </AccordionPanel>
      {meetsAa(ratio) ? null : (
        <Alert className="mb-4">{format(messages.contrastWarning, { name, ratio: ratio.toFixed(1) })}</Alert>
      )}
    </AccordionItem>
  );
}

export function SpeakerColors({
  palette,
  fontFamily,
  subtitleFormat,
  openSlot,
  onOpenSlotChange,
  onChange,
}: SpeakerColorsProps): VNode {
  const messages: Messages = useMessages();
  return (
    <Card>
      <CardHeader>
        <CardTitle>{format(messages.speakersHeading, { count: palette.length })}</CardTitle>
        <CardDescription>{messages.speakersHint}</CardDescription>
      </CardHeader>
      <CardContent className="grid grid-cols-1 gap-4">
        <FormatLimit format={subtitleFormat} />
        <Accordion
          value={openSlot === null ? [] : [String(openSlot)]}
          onValueChange={(open: string[]): void => {
            const [slot] = open;
            onOpenSlotChange(slot === undefined ? null : Number(slot));
          }}
        >
          {palette.map(
            (color: SpeakerColor, index: number): VNode => (
              <SpeakerSlot
                key={String(index)}
                slot={index}
                color={color}
                name={format(messages.speakerName, { number: index + 1 })}
                fontFamily={fontFamily}
                onChange={(next: SpeakerColor): void =>
                  onChange(
                    palette.map(
                      (slot: SpeakerColor, position: number): SpeakerColor =>
                        position === index ? next : slot,
                    ),
                  )
                }
              />
            ),
          )}
        </Accordion>
        <div className="flex flex-wrap gap-2">
          {palette.length < MAX_PALETTE_SIZE ? (
            <Button
              variant="outline"
              onClick={(): void => onChange([...palette, { ...LATER_SPEAKER, fill: NEW_SLOT_FILL }])}
            >
              {messages.addSpeaker}
            </Button>
          ) : null}
          {palette.length > 1 ? (
            <Button variant="outline" onClick={(): void => onChange(palette.slice(0, -1))}>
              {messages.removeSpeaker}
            </Button>
          ) : null}
          <Button variant="outline" onClick={(): void => onChange(defaultPalette())}>
            {messages.resetPalette}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
