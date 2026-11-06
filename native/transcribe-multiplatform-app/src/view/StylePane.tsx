// Runs with bun: caption position, size and speaker colours, with a live preview.
import type { VNode } from "preact";
import { useState } from "preact/hooks";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group.tsx";
import type { Anchor, Profile, Settings, SpeakerColor } from "../backend/types.ts";
import { format, type Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import { type FrameSize, type PreviewLayout, previewLayout } from "../layout/preview-layout.ts";
import { type AppStore, reportFailure } from "../state/use-app-store.ts";
import { FaceSilhouette } from "./FaceSilhouette.tsx";
import { FontField } from "./FontField.tsx";
import { AdjustField, SelectField, type SelectOption } from "./fields.tsx";
import { LayeredText } from "./LayeredText.tsx";
import { ProfilesCard } from "./ProfilesCard.tsx";
import { defaultPalette, SpeakerColors } from "./SpeakerColors.tsx";

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

interface CaptionPreviewProps {
  settings: Settings;
  frame: FrameChoice;
  /** The speaker row being edited. */
  slot: number | null;
}

interface SafeZoneBandProps {
  band: { x: number; width: number };
  edge: number;
  size: FrameSize;
}

const PERCENT: number = 100;
const PERCENT_DECIMALS_SCALE: number = 10;
const OFFSET_LIMIT: number = 50;
const OFFSET_STEP: number = 0.5;
const SAFE_ZONE_LIMIT: number = 40;
/** Over the preview's bright and dark halves alike: black under white dashes holds 3:1 on both (WCAG 1.4.11). */
const SAFE_ZONE_FILL: string = "rgb(0 0 0 / 0.3)";
const SAFE_ZONE_LINE_RATIO: number = 0.005;
const SAFE_ZONE_DASH_RATIO: number = 0.02;
const PREVIEW_MAX_WIDTH_PX: number = 560;
const PREVIEW_VERTICAL_WIDTH_PX: number = 200;
const LINE_HEIGHT_MIN: number = 1;
const LINE_HEIGHT_MAX: number = 3;
const LINE_HEIGHT_STEP: number = 0.001;
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

/** One edge's strip and the line at its inner side; a zone of no width draws nothing. */
const SafeZoneBand = ({ band, edge, size }: SafeZoneBandProps): VNode | null => {
  if (band.width <= 0) {
    return null;
  }
  const shortEdge: number = Math.min(size.width, size.height);
  const line = { x1: edge, x2: edge, y1: 0, y2: size.height, strokeWidth: shortEdge * SAFE_ZONE_LINE_RATIO };
  return (
    <g data-slot="safe-zone">
      <rect x={band.x} y={0} width={band.width} height={size.height} fill={SAFE_ZONE_FILL} />
      <line {...line} stroke="#000000" />
      <line {...line} stroke="#ffffff" strokeDasharray={String(shortEdge * SAFE_ZONE_DASH_RATIO)} />
    </g>
  );
};

const CaptionPreview = ({ settings, frame, slot }: CaptionPreviewProps): VNode => {
  const messages: Messages = useMessages();
  const { width, height } = frame.size;
  const layout: PreviewLayout = previewLayout(messages.previewSample, frame.size, settings);
  // Whoever is being edited, or speaker 1 while no row is open.
  const color: SpeakerColor = settings.palette[slot ?? 0] ?? settings.palette[0] ?? defaultPalette()[0];
  const widest: number = width >= height ? PREVIEW_MAX_WIDTH_PX : PREVIEW_VERTICAL_WIDTH_PX;
  return (
    <>
      {/* In a narrow window only the picture stays in view, small enough to leave room for the controls. */}
      <div
        data-slot="caption-preview"
        className="sticky top-0 z-10 bg-(--page) py-1 [--preview-most:30vh] lg:static lg:bg-transparent lg:py-0 lg:[--preview-most:70vh]"
      >
        {/* Half bright, half dark: an outline has to hold up on both, as it must on real video. */}
        <svg
          aria-hidden="true"
          className="mx-auto block rounded-lg bg-[linear-gradient(#a9d8ef_50%,#1f2530_50%)]"
          viewBox={`0 0 ${width} ${height}`}
          style={{ width: `min(${widest}px, 100%, calc(var(--preview-most) * ${width} / ${height}))` }}
        >
          {layout.face === null ? null : <FaceSilhouette face={layout.face} frame={frame.size} />}
          <SafeZoneBand
            band={{ x: 0, width: layout.safeLeftPx }}
            edge={layout.safeLeftPx}
            size={frame.size}
          />
          <SafeZoneBand
            band={{ x: width - layout.safeRightPx, width: layout.safeRightPx }}
            edge={width - layout.safeRightPx}
            size={frame.size}
          />
          <LayeredText
            lines={layout.lines}
            x={layout.x}
            y={layout.y}
            fontFamily={settings.font_family}
            fontPx={layout.renderedFontPx}
            lineHeightPx={layout.lineHeightPx}
            color={color}
            strokeUnitPx={layout.strokeUnitPx}
            frame={frame.size}
          />
        </svg>
      </div>
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
};

export function StylePane({ store, settings, profiles, faceOptions }: StylePaneProps): VNode {
  const messages: Messages = useMessages();
  const [frameId, setFrameId] = useState<FrameId>("landscape");
  // Speaker 1 starts open, so the first look at this card shows what a row holds.
  const [openSlot, setOpenSlot] = useState<number | null>(0);
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
    <div className="grid grid-cols-1 gap-6">
      <ProfilesCard store={store} settings={settings} profiles={profiles} />
      <div className="flex flex-col gap-6 lg:grid lg:grid-cols-2 lg:items-start">
        {/* First in the page so it is at the top of a narrow window; `contents` lets its picture stick to this whole column. */}
        <div className="contents lg:sticky lg:top-4 lg:col-start-2 lg:row-start-1 lg:grid lg:min-w-0 lg:grid-cols-1 lg:gap-4">
          <Card>
            <CardHeader>
              <CardTitle>{messages.previewHeading}</CardTitle>
            </CardHeader>
            <CardContent>
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
            </CardContent>
          </Card>
          <CaptionPreview settings={settings} frame={FRAMES[frameId]} slot={openSlot} />
        </div>
        {/* Keyboard focus scrolls clear of the sticky picture (WCAG 2.4.12). */}
        <div className="grid min-w-0 grid-cols-1 gap-6 [&_*]:scroll-mt-[calc(30vh+1rem)] lg:col-start-1 lg:row-start-1 lg:[&_*]:scroll-mt-4">
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
              <p className="text-muted-foreground">{messages.safeZoneHint}</p>
              <AdjustField
                label={messages.safeLeft}
                value={settings.safe_left_percent}
                min={0}
                max={SAFE_ZONE_LIMIT}
                step={OFFSET_STEP}
                onChange={(safe_left_percent: number): void => update({ safe_left_percent })}
              />
              <AdjustField
                label={messages.safeRight}
                value={settings.safe_right_percent}
                min={0}
                max={SAFE_ZONE_LIMIT}
                step={OFFSET_STEP}
                onChange={(safe_right_percent: number): void => update({ safe_right_percent })}
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
          <SpeakerColors
            palette={settings.palette}
            fontFamily={settings.font_family}
            subtitleFormat={settings.subtitle_format}
            openSlot={openSlot}
            onOpenSlotChange={setOpenSlot}
            onChange={(palette: SpeakerColor[]): void => update({ palette })}
          />
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
                label={messages.captionLineHeight}
                value={settings.line_height}
                min={LINE_HEIGHT_MIN}
                max={LINE_HEIGHT_MAX}
                step={LINE_HEIGHT_STEP}
                onChange={(line_height: number): void => update({ line_height })}
              />
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
