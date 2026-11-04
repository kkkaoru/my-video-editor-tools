// Runs with bun: caption position, size and speaker colours, with a live preview.
import { Toggle } from "@base-ui/react/toggle";
import { ToggleGroup } from "@base-ui/react/toggle-group";
import type { VNode } from "preact";
import { useState } from "preact/hooks";
import type { Anchor, Settings, SpeakerColor } from "../backend/types.ts";
import { format, type Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import { type FrameSize, type PreviewLayout, previewLayout } from "../layout/preview-layout.ts";
import type { AppStore } from "../state/use-app-store.ts";
import { ActionButton, AdjustField, buttonClass, SelectField, type SelectOption } from "./controls.tsx";

export interface StylePaneProps {
  store: AppStore;
  settings: Settings;
  faceOptions: SelectOption[];
}

interface ToggleChoice {
  value: string;
  label: string;
}

interface ChoiceGroupProps {
  label: string;
  value: string;
  choices: ToggleChoice[];
  onChange: (value: string) => void;
}

interface FrameChoice {
  id: string;
  size: FrameSize;
  label: (messages: Messages) => string;
}

interface PaletteProps {
  palette: SpeakerColor[];
  onChange: (palette: SpeakerColor[]) => void;
}

const PERCENT: number = 100;
const OFFSET_LIMIT: number = 50;
const OFFSET_STEP: number = 0.5;
const PREVIEW_MAX_WIDTH_PX: number = 560;
const PREVIEW_VERTICAL_WIDTH_PX: number = 200;
const PREVIEW_STROKE_PER_SIDE: number = 2;
const DEFAULT_OUTLINE: string = "#FFFFFF";
const NEW_SLOT_FILL: string = "#FFFFFF";
const FIRST_COLOURS: string[] = [
  "#C48CFF",
  "#7CDC8C",
  "#6CD4FF",
  "#FF9E5E",
  "#FF7FA8",
  "#F2D64B",
  "#5EE0C8",
  "#8FA8FF",
  "#E08CE0",
  "#B5E05E",
];
const SPEECH_LANGUAGES: ToggleChoice[] = [
  { value: "ja", label: "日本語" },
  { value: "en", label: "English" },
  { value: "zh", label: "中文" },
  { value: "ko", label: "한국어" },
  { value: "de", label: "Deutsch" },
  { value: "fr", label: "Français" },
  { value: "es", label: "Español" },
];
const LANDSCAPE: FrameChoice = {
  id: "landscape",
  size: { width: 1920, height: 1080 },
  label: ({ previewLandscape }: Messages): string => previewLandscape,
};
const FRAMES: FrameChoice[] = [
  LANDSCAPE,
  {
    id: "vertical",
    size: { width: 1080, height: 1920 },
    label: ({ previewVertical }: Messages): string => previewVertical,
  },
  { id: "uhd", size: { width: 3840, height: 2160 }, label: ({ previewUhd }: Messages): string => previewUhd },
];
const ANCHOR_VALUES: ReadonlySet<string> = new Set(["face_above", "face_below", "video_top", "video_bottom"]);

const isAnchor = (value: string): value is Anchor => ANCHOR_VALUES.has(value);

export const defaultPalette = (): SpeakerColor[] =>
  FIRST_COLOURS.map((fill: string): SpeakerColor => ({ fill, outline: DEFAULT_OUTLINE }));

const ChoiceGroup = ({ label, value, choices, onChange }: ChoiceGroupProps): VNode => (
  <ToggleGroup
    aria-label={label}
    className="my-2 flex flex-wrap gap-2"
    value={[value]}
    onValueChange={(next: string[]): void => {
      const chosen: string | undefined = next[0];
      if (chosen !== undefined) {
        onChange(chosen);
      }
    }}
  >
    {choices.map(
      ({ value: choiceValue, label: choiceLabel }: ToggleChoice): VNode => (
        <Toggle key={choiceValue} value={choiceValue} className={buttonClass(choiceValue === value)}>
          {choiceLabel}
        </Toggle>
      ),
    )}
  </ToggleGroup>
);

function CaptionPreview({ settings, frame }: { settings: Settings; frame: FrameChoice }): VNode {
  const messages: Messages = useMessages();
  const { width, height } = frame.size;
  const layout: PreviewLayout = previewLayout(messages.previewSample, frame.size, settings);
  const displayWidth: number = width >= height ? PREVIEW_MAX_WIDTH_PX : PREVIEW_VERTICAL_WIDTH_PX;
  const scale: number = displayWidth / width;
  const color: SpeakerColor | undefined = settings.palette[0];
  return (
    <>
      <div
        aria-hidden="true"
        className="relative max-w-full overflow-hidden rounded-md bg-neutral-800"
        style={{ width: `${displayWidth}px`, aspectRatio: `${width} / ${height}` }}
      >
        {layout.usesFace ? (
          <div className="absolute top-[36%] left-1/2 aspect-[3/4] w-[18%] -translate-x-1/2 -translate-y-1/2 rounded-full bg-neutral-400" />
        ) : null}
        <div
          className="outlined-text absolute -translate-x-1/2 -translate-y-1/2 text-center font-bold whitespace-pre-wrap"
          style={{
            left: `${layout.x * scale}px`,
            top: `${layout.y * scale}px`,
            fontSize: `${layout.fontPx * scale}px`,
            lineHeight: 1.25,
            color: color?.fill,
            WebkitTextStroke: `${PREVIEW_STROKE_PER_SIDE * layout.outlinePx * scale}px ${color?.outline}`,
          }}
        >
          {layout.lines.join("\n")}
        </div>
      </div>
      <p className="mt-2 text-sm text-muted">
        {format(messages.previewDescription, {
          size: frame.label(messages),
          x: Math.round((layout.x / width) * PERCENT),
          y: Math.round((layout.y / height) * PERCENT),
        })}
      </p>
    </>
  );
}

function ColorSlot({
  color,
  index,
  onChange,
}: {
  color: SpeakerColor;
  index: number;
  onChange: (color: SpeakerColor) => void;
}): VNode {
  const messages: Messages = useMessages();
  const number: number = index + 1;
  return (
    <div className="flex items-center gap-2">
      <input
        type="color"
        className="size-11 cursor-pointer rounded border border-line"
        aria-label={format(messages.fillColor, { number })}
        value={color.fill}
        onInput={({ currentTarget }): void => onChange({ ...color, fill: currentTarget.value.toUpperCase() })}
      />
      <input
        type="color"
        className="size-11 cursor-pointer rounded border border-line"
        aria-label={format(messages.outlineColor, { number })}
        value={color.outline}
        onInput={({ currentTarget }): void =>
          onChange({ ...color, outline: currentTarget.value.toUpperCase() })
        }
      />
      <span>{format(messages.speakerName, { number })}</span>
      <span
        aria-hidden="true"
        className="outlined-text rounded bg-neutral-800 px-2 font-bold"
        style={{ color: color.fill, WebkitTextStroke: `3px ${color.outline}` }}
      >
        Aあ
      </span>
    </div>
  );
}

function PaletteEditor({ palette, onChange }: PaletteProps): VNode {
  const messages: Messages = useMessages();
  return (
    <>
      <h3 className="mt-4 font-semibold">{format(messages.speakersHeading, { count: palette.length })}</h3>
      <p className="text-sm text-muted">{messages.speakersHint}</p>
      <div className="my-2 grid gap-2 sm:grid-cols-2">
        {palette.map(
          (color: SpeakerColor, index: number): VNode => (
            <ColorSlot
              key={String(index)}
              color={color}
              index={index}
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
      </div>
      <div className="flex flex-wrap gap-2">
        <ActionButton
          onClick={(): void => onChange([...palette, { fill: NEW_SLOT_FILL, outline: DEFAULT_OUTLINE }])}
        >
          {messages.addSpeaker}
        </ActionButton>
        {palette.length > 1 ? (
          <ActionButton onClick={(): void => onChange(palette.slice(0, -1))}>
            {messages.removeSpeaker}
          </ActionButton>
        ) : null}
        <ActionButton onClick={(): void => onChange(defaultPalette())}>{messages.resetPalette}</ActionButton>
      </div>
    </>
  );
}

export function StylePane({ store, settings, faceOptions }: StylePaneProps): VNode {
  const messages: Messages = useMessages();
  const [frameId, setFrameId] = useState<string>("landscape");
  const frame: FrameChoice = FRAMES.find(({ id }: FrameChoice): boolean => id === frameId) ?? LANDSCAPE;
  const { placement } = settings;
  const update = (changes: Partial<Settings>): void => store.updateSettings({ ...settings, ...changes });
  const place = (changes: Partial<Settings["placement"]>): void =>
    update({ placement: { ...placement, ...changes } });
  const anchors: ToggleChoice[] = [
    { value: "face_above", label: messages.anchorFaceAbove },
    { value: "face_below", label: messages.anchorFaceBelow },
    { value: "video_top", label: messages.anchorVideoTop },
    { value: "video_bottom", label: messages.anchorVideoBottom },
  ];
  return (
    <div className="grid gap-6 lg:grid-cols-2">
      <section aria-labelledby="position-heading">
        <h2 id="position-heading" className="font-semibold">
          {messages.positionHeading}
        </h2>
        <ChoiceGroup
          label={messages.positionLabel}
          value={placement.anchor}
          choices={anchors}
          onChange={(anchor: string): void => {
            if (isAnchor(anchor)) {
              place({ anchor });
            }
          }}
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
            (choice: ToggleChoice): SelectOption => ({ ...choice, disabled: false }),
          )}
          onChange={(language: string): void => update({ language })}
        />
        <h2 className="mt-6 font-semibold">{messages.sizeHeading}</h2>
        <p className="text-sm text-muted">{messages.sizeHint}</p>
        <AdjustField
          label={messages.fontSize}
          value={Math.round(settings.font_ratio * PERCENT * 10) / 10}
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
      </section>
      <section aria-labelledby="preview-heading">
        <h2 id="preview-heading" className="font-semibold">
          {messages.previewHeading}
        </h2>
        <ChoiceGroup
          label={messages.previewFrameLabel}
          value={frameId}
          choices={FRAMES.map(
            ({ id, label }: FrameChoice): ToggleChoice => ({ value: id, label: label(messages) }),
          )}
          onChange={setFrameId}
        />
        <CaptionPreview settings={settings} frame={frame} />
        <PaletteEditor
          palette={settings.palette}
          onChange={(palette: SpeakerColor[]): void => update({ palette })}
        />
      </section>
    </div>
  );
}
