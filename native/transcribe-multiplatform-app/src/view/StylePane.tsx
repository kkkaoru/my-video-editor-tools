// Runs with bun: caption position, size and speaker colours, with a live preview.
import type { VNode } from "preact";
import { useState } from "preact/hooks";
import { Alert } from "@/components/ui/alert.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group.tsx";
import type { Anchor, Profile, Settings, SpeakerColor } from "../backend/types.ts";
import { format, type Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import { type FrameSize, type PreviewLayout, previewLayout } from "../layout/preview-layout.ts";
import { type AppStore, reportFailure } from "../state/use-app-store.ts";
import { contrastRatio, meetsAa } from "../styling/contrast.ts";
import { FontField } from "./FontField.tsx";
import { AdjustField, SelectField, type SelectOption, SwitchField } from "./fields.tsx";
import { LayeredText } from "./LayeredText.tsx";
import { ProfilesCard } from "./ProfilesCard.tsx";

export interface StylePaneProps {
  store: AppStore;
  settings: Settings;
  profiles: Profile[];
  faceOptions: SelectOption[];
}

interface ToggleChoice<Value extends string> {
  value: Value;
  label: string;
}

interface ChoiceGroupProps<Value extends string> {
  label: string;
  value: Value;
  choices: ToggleChoice<Value>[];
  onChange: (value: Value) => void;
}

type FrameId = "landscape" | "vertical" | "uhd";

interface FrameChoice {
  id: FrameId;
  size: FrameSize;
  label: (messages: Messages) => string;
}

interface PaletteProps {
  palette: SpeakerColor[];
  unassigned: SpeakerColor;
  fontFamily: string;
  onChange: (palette: SpeakerColor[]) => void;
  onUnassignedChange: (unassigned: SpeakerColor) => void;
}

interface ColorSlotProps {
  color: SpeakerColor;
  name: string;
  fontFamily: string;
  onChange: (color: SpeakerColor) => void;
}

const PERCENT: number = 100;
const OFFSET_LIMIT: number = 50;
const OFFSET_STEP: number = 0.5;
const PERCENT_DECIMALS_SCALE: number = 10;
const PREVIEW_MAX_WIDTH_PX: number = 560;
const PREVIEW_VERTICAL_WIDTH_PX: number = 200;
const DEFAULT_OUTLINE: string = "#FFFFFF";
const NEW_SLOT_FILL: string = "#FFFFFF";
const FIRST_SPEAKER: SpeakerColor = { fill: "#FFFFFF", outline: "#000000" };
const SAMPLE_FONT_PX: number = 22;
const SAMPLE_SIZE: FrameSize = { width: 64, height: 32 };
const SAMPLE_OUTLINE_RATIO: number = 0.115;
const SAMPLE_RIM_RATIO: number = 0.034;
const LINE_HEIGHT_MIN: number = 1;
const LINE_HEIGHT_MAX: number = 3;
const LINE_HEIGHT_STEP: number = 0.001;
const RIM_PERCENT_MAX: number = 10;
const RIM_PERCENT_STEP: number = 0.5;
const HEAD_WIDTH_RATIO: number = 0.75;
const FACE_WIDTH_RATIO: number = 0.4;
const COLOR_INPUT_CLASS: string = "size-11 cursor-pointer rounded-md border border-border bg-background p-1";
const FIRST_COLOURS: string[] = [
  "#005A9C",
  "#6A1FA8",
  "#0B672C",
  "#A3123A",
  "#8A4500",
  "#7A1D7A",
  "#006262",
  "#3A3AA8",
  "#5C5C00",
];
const SPEECH_LANGUAGES: ToggleChoice<string>[] = [
  { value: "ja", label: "日本語" },
  { value: "en", label: "English" },
  { value: "zh", label: "中文" },
  { value: "ko", label: "한국어" },
  { value: "de", label: "Deutsch" },
  { value: "fr", label: "Français" },
  { value: "es", label: "Español" },
];
const FRAMES: Record<FrameId, FrameChoice> = {
  landscape: {
    id: "landscape",
    size: { width: 1920, height: 1080 },
    label: ({ previewLandscape }: Messages): string => previewLandscape,
  },
  vertical: {
    id: "vertical",
    size: { width: 1080, height: 1920 },
    label: ({ previewVertical }: Messages): string => previewVertical,
  },
  uhd: {
    id: "uhd",
    size: { width: 3840, height: 2160 },
    label: ({ previewUhd }: Messages): string => previewUhd,
  },
};

/** Speaker 1 is white on a black outline; the others keep a colour on a white outline. */
export const defaultPalette = (): SpeakerColor[] => [
  FIRST_SPEAKER,
  ...FIRST_COLOURS.map((fill: string): SpeakerColor => ({ fill, outline: DEFAULT_OUTLINE })),
];

const ChoiceGroup = <Value extends string>({
  label,
  value,
  choices,
  onChange,
}: ChoiceGroupProps<Value>): VNode => (
  <ToggleGroup
    aria-label={label}
    value={[value]}
    onValueChange={(next: string[]): void => {
      // Pressing the active choice empties `next`; one choice always stays selected.
      const chosen: ToggleChoice<Value> | undefined = choices.find(
        ({ value: choiceValue }: ToggleChoice<Value>): boolean => choiceValue === next[0],
      );
      if (chosen !== undefined) {
        onChange(chosen.value);
      }
    }}
  >
    {choices.map(
      ({ value: choiceValue, label: choiceLabel }: ToggleChoice<Value>): VNode => (
        <ToggleGroupItem key={choiceValue} value={choiceValue}>
          {choiceLabel}
        </ToggleGroupItem>
      ),
    )}
  </ToggleGroup>
);

function CaptionPreview({ settings, frame }: { settings: Settings; frame: FrameChoice }): VNode {
  const messages: Messages = useMessages();
  const { width, height } = frame.size;
  const layout: PreviewLayout = previewLayout(messages.previewSample, frame.size, settings);
  const color: SpeakerColor | undefined = settings.palette[0];
  const faceHeight: number = layout.face === null ? 0 : layout.face.bottom - layout.face.top;
  return (
    <>
      {/* Half bright, half dark: an outline has to hold up on both, as it must on real video. */}
      <svg
        aria-hidden="true"
        className="mx-auto block max-w-full rounded-lg bg-[linear-gradient(#a9d8ef_50%,#1f2530_50%)]"
        viewBox={`0 0 ${width} ${height}`}
        style={{ width: `${width >= height ? PREVIEW_MAX_WIDTH_PX : PREVIEW_VERTICAL_WIDTH_PX}px` }}
      >
        {layout.face === null ? null : (
          <>
            <ellipse
              cx={layout.face.centerX}
              cy={(layout.face.headTop + layout.face.bottom) / 2}
              rx={faceHeight * HEAD_WIDTH_RATIO}
              ry={(layout.face.bottom - layout.face.headTop) / 2}
              fill="#3a3f47"
            />
            <ellipse
              cx={layout.face.centerX}
              cy={(layout.face.top + layout.face.bottom) / 2}
              rx={faceHeight * FACE_WIDTH_RATIO}
              ry={faceHeight / 2}
              fill="#e8c9b4"
            />
          </>
        )}
        <LayeredText
          lines={layout.lines}
          x={layout.x}
          y={layout.y}
          fontFamily={settings.font_family}
          fontPx={layout.fontPx}
          lineHeightPx={layout.lineHeightPx}
          color={color ?? FIRST_SPEAKER}
          outlinePx={layout.outlinePx}
          rimPx={layout.rimPx}
          shadow={settings.shadow}
        />
      </svg>
      <p className="text-muted-foreground">
        {format(messages.previewDescription, {
          size: frame.label(messages),
          x: Math.round((layout.x / width) * PERCENT),
          y: Math.round((layout.y / height) * PERCENT),
        })}{" "}
        {format(messages.previewFontPixels, { px: layout.fontPx })}
      </p>
    </>
  );
}

function ColorSlot({ color, name, fontFamily, onChange }: ColorSlotProps): VNode {
  const messages: Messages = useMessages();
  const ratio: number = contrastRatio(color.fill, color.outline);
  return (
    <div className="grid gap-1">
      {/* The name stays on one line: below the row's width it moves down whole instead of breaking. */}
      <div className="flex flex-wrap items-center gap-2">
        <input
          type="color"
          className={COLOR_INPUT_CLASS}
          aria-label={`${name}: ${messages.fillLabel}`}
          value={color.fill}
          onInput={({ currentTarget }): void =>
            onChange({ ...color, fill: currentTarget.value.toUpperCase() })
          }
        />
        <input
          type="color"
          className={COLOR_INPUT_CLASS}
          aria-label={`${name}: ${messages.outlineLabel}`}
          value={color.outline}
          onInput={({ currentTarget }): void =>
            onChange({ ...color, outline: currentTarget.value.toUpperCase() })
          }
        />
        <span className="flex-1 whitespace-nowrap">{name}</span>
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
            outlinePx={SAMPLE_FONT_PX * SAMPLE_OUTLINE_RATIO}
            rimPx={SAMPLE_FONT_PX * SAMPLE_RIM_RATIO}
            shadow={{ enabled: false, strength: 0 }}
          />
        </svg>
      </div>
      {meetsAa(ratio) ? null : (
        <Alert>{format(messages.contrastWarning, { name, ratio: ratio.toFixed(1) })}</Alert>
      )}
    </div>
  );
}

function PaletteEditor({
  palette,
  unassigned,
  fontFamily,
  onChange,
  onUnassignedChange,
}: PaletteProps): VNode {
  const messages: Messages = useMessages();
  return (
    <Card>
      <CardHeader>
        <CardTitle>{format(messages.speakersHeading, { count: palette.length })}</CardTitle>
        <CardDescription>{messages.speakersHint}</CardDescription>
      </CardHeader>
      <CardContent className="grid grid-cols-1 gap-3">
        <div className="grid grid-cols-[repeat(auto-fill,minmax(min(100%,22rem),1fr))] gap-2">
          {palette.map(
            (color: SpeakerColor, index: number): VNode => (
              <ColorSlot
                key={String(index)}
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
          <ColorSlot
            color={unassigned}
            name={messages.unknownSpeaker}
            fontFamily={fontFamily}
            onChange={onUnassignedChange}
          />
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            onClick={(): void => onChange([...palette, { fill: NEW_SLOT_FILL, outline: DEFAULT_OUTLINE }])}
          >
            {messages.addSpeaker}
          </Button>
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

export function StylePane({ store, settings, profiles, faceOptions }: StylePaneProps): VNode {
  const messages: Messages = useMessages();
  const [frameId, setFrameId] = useState<FrameId>("landscape");
  const { placement } = settings;
  const update = (changes: Partial<Settings>): void => store.updateSettings({ ...settings, ...changes });
  const place = (changes: Partial<Settings["placement"]>): void =>
    update({ placement: { ...placement, ...changes } });
  const anchors: ToggleChoice<Anchor>[] = [
    { value: "face_above", label: messages.anchorFaceAbove },
    { value: "face_below", label: messages.anchorFaceBelow },
    { value: "video_top", label: messages.anchorVideoTop },
    { value: "video_bottom", label: messages.anchorVideoBottom },
  ];
  return (
    <div className="grid grid-cols-1 gap-4">
      <ProfilesCard store={store} settings={settings} profiles={profiles} />
      <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-2">
        <div className="grid min-w-0 grid-cols-1 gap-4">
          <Card>
            <CardHeader>
              <CardTitle>{messages.positionHeading}</CardTitle>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-4">
              <ChoiceGroup
                label={messages.positionLabel}
                value={placement.anchor}
                choices={anchors}
                onChange={(anchor: Anchor): void => place({ anchor })}
              />
              <AdjustField
                label={messages.offsetX}
                value={placement.offset_x_percent}
                min={-OFFSET_LIMIT}
                max={OFFSET_LIMIT}
                step={OFFSET_STEP}
                onChange={(offset_x_percent: number): void => place({ offset_x_percent })}
              />
              <AdjustField
                label={messages.offsetY}
                value={placement.offset_y_percent}
                min={-OFFSET_LIMIT}
                max={OFFSET_LIMIT}
                step={OFFSET_STEP}
                onChange={(offset_y_percent: number): void => place({ offset_y_percent })}
              />
              <SelectField
                label={messages.faceModel}
                value={settings.face_model}
                options={faceOptions}
                onChange={(face_model: string): void => update({ face_model })}
              />
              <SelectField
                label={messages.speechLanguage}
                value={settings.language}
                options={[{ value: "auto", label: messages.languageAuto }, ...SPEECH_LANGUAGES].map(
                  (choice: ToggleChoice<string>): SelectOption => ({ ...choice, disabled: false }),
                )}
                onChange={(language: string): void => update({ language })}
              />
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>{messages.sizeHeading}</CardTitle>
              <CardDescription>{messages.sizeHint}</CardDescription>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-4">
              <FontField
                value={settings.font_family}
                onChange={(font_family: string): void => update({ font_family })}
                onFailure={(error: unknown): void => reportFailure(store.setState, error)}
              />
              <AdjustField
                label={messages.fontSize}
                value={
                  Math.round(settings.font_ratio * PERCENT * PERCENT_DECIMALS_SCALE) / PERCENT_DECIMALS_SCALE
                }
                min={2}
                max={12}
                step={0.2}
                onChange={(percent: number): void => update({ font_ratio: percent / PERCENT })}
              />
              <AdjustField
                label={messages.captionWidth}
                value={Math.round(settings.width_ratio * PERCENT)}
                min={30}
                max={100}
                step={1}
                onChange={(percent: number): void => update({ width_ratio: percent / PERCENT })}
              />
              <AdjustField
                label={messages.outlineWidth}
                value={Math.round(settings.outline_ratio * PERCENT)}
                min={2}
                max={30}
                step={1}
                onChange={(percent: number): void => update({ outline_ratio: percent / PERCENT })}
              />
              <AdjustField
                label={messages.captionLineHeight}
                value={settings.line_height}
                min={LINE_HEIGHT_MIN}
                max={LINE_HEIGHT_MAX}
                step={LINE_HEIGHT_STEP}
                onChange={(line_height: number): void => update({ line_height })}
              />
              <AdjustField
                label={messages.rimWidth}
                value={
                  Math.round(settings.rim_ratio * PERCENT * PERCENT_DECIMALS_SCALE) / PERCENT_DECIMALS_SCALE
                }
                min={0}
                max={RIM_PERCENT_MAX}
                step={RIM_PERCENT_STEP}
                onChange={(percent: number): void => update({ rim_ratio: percent / PERCENT })}
              />
              <SwitchField
                label={messages.shadowEnabled}
                checked={settings.shadow.enabled}
                onChange={(enabled: boolean): void => update({ shadow: { ...settings.shadow, enabled } })}
              />
              {settings.shadow.enabled ? (
                <AdjustField
                  label={messages.shadowStrength}
                  value={Math.round(settings.shadow.strength * PERCENT)}
                  min={0}
                  max={100}
                  step={5}
                  onChange={(percent: number): void =>
                    update({ shadow: { ...settings.shadow, strength: percent / PERCENT } })
                  }
                />
              ) : null}
            </CardContent>
          </Card>
        </div>
        <div className="grid min-w-0 grid-cols-1 gap-4">
          <Card>
            <CardHeader>
              <CardTitle>{messages.previewHeading}</CardTitle>
            </CardHeader>
            <CardContent className="grid grid-cols-1 gap-3">
              <ChoiceGroup
                label={messages.previewFrameLabel}
                value={frameId}
                choices={Object.values(FRAMES).map(
                  ({ id, label }: FrameChoice): ToggleChoice<FrameId> => ({
                    value: id,
                    label: label(messages),
                  }),
                )}
                onChange={setFrameId}
              />
              <CaptionPreview settings={settings} frame={FRAMES[frameId]} />
            </CardContent>
          </Card>
          <PaletteEditor
            palette={settings.palette}
            unassigned={settings.unassigned}
            fontFamily={settings.font_family}
            onChange={(palette: SpeakerColor[]): void => update({ palette })}
            onUnassignedChange={(unassigned: SpeakerColor): void => update({ unassigned })}
          />
        </div>
      </div>
    </div>
  );
}
