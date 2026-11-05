// Runs with bun: a colour square that opens the usual colours, "transparent" where that is allowed, and any colour.
import { CheckIcon } from "lucide-preact";
import type { VNode } from "preact";
import { useState } from "preact/hooks";
import { Button } from "@/components/ui/button.tsx";
import { Input } from "@/components/ui/input.tsx";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover.tsx";
import type { Messages, PresetColor } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import { contrastRatio } from "../styling/contrast.ts";

export interface ColorPickerProps {
  label: string;
  /** Null is transparent. */
  value: string | null;
  allowTransparent: boolean;
  onChange: (value: string | null) => void;
}

interface Preset {
  name: PresetColor;
  hex: string;
}

const PRESETS: Preset[] = [
  { name: "white", hex: "#FFFFFF" },
  { name: "black", hex: "#000000" },
  { name: "red", hex: "#D0021B" },
  { name: "orange", hex: "#F5820A" },
  { name: "yellow", hex: "#FFE600" },
  { name: "green", hex: "#0B672C" },
  { name: "blue", hex: "#005A9C" },
  { name: "purple", hex: "#6A1FA8" },
];
const CUSTOM_START: string = "#000000";
const HEX_COLOR: RegExp = /^#[0-9a-f]{6}$/i;
/** One element for every square: `bg-clip-padding` keeps a dark colour from showing through the 1 px border's corners. */
const SWATCH: string = "size-11 min-h-0 min-w-0 shrink-0 rounded-lg border border-border bg-clip-padding p-0";
const CHECKERBOARD: string =
  "bg-[repeating-conic-gradient(#767676_0_25%,#ffffff_0_50%)] bg-[length:12px_12px]";

const colorName = (value: string | null, messages: Messages): string => {
  if (value === null) {
    return messages.colorTransparent;
  }
  const preset: Preset | undefined = PRESETS.find(({ hex }: Preset): boolean => hex === value);
  return preset === undefined ? value : messages.colorNames[preset.name];
};

/** Whichever of black and white reads better on the colour, for the mark on a chosen square. */
const markColor = (hex: string): string =>
  contrastRatio(hex, "#000000") >= contrastRatio(hex, "#FFFFFF") ? "#000000" : "#FFFFFF";

const swatchStyle = (value: string | null): { backgroundColor: string } | undefined =>
  value === null ? undefined : { backgroundColor: value };

export function ColorPicker({ label, value, allowTransparent, onChange }: ColorPickerProps): VNode {
  const messages: Messages = useMessages();
  const [open, setOpen] = useState<boolean>(false);
  // Null shows the colour itself; what is being typed stays until it leaves the field.
  const [typed, setTyped] = useState<string | null>(null);
  const choose = (next: string | null): void => {
    onChange(next);
    setOpen(false);
  };
  const type = (text: string): void => {
    setTyped(text);
    if (HEX_COLOR.test(text)) {
      onChange(text.toUpperCase());
    }
  };
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        data-slot="color-swatch"
        aria-label={`${label}: ${colorName(value, messages)}`}
        className={`rounded-lg bg-clip-padding ${value === null ? CHECKERBOARD : ""}`}
        style={swatchStyle(value)}
      />
      <PopoverContent data-slot="color-picker" aria-label={label} className="grid w-auto gap-2">
        <div className="grid grid-cols-4 gap-2">
          {PRESETS.map(
            ({ name, hex }: Preset): VNode => (
              <Button
                key={name}
                variant="outline"
                aria-label={messages.colorNames[name]}
                aria-pressed={value === hex}
                className={SWATCH}
                style={swatchStyle(hex)}
                onClick={(): void => choose(hex)}
              >
                {value === hex ? (
                  <CheckIcon className="size-5" style={{ color: markColor(hex) }} aria-hidden="true" />
                ) : null}
              </Button>
            ),
          )}
        </div>
        {allowTransparent ? (
          <Button
            variant="outline"
            aria-pressed={value === null}
            className="justify-start px-2"
            onClick={(): void => choose(null)}
          >
            <span
              className={`size-7 shrink-0 rounded-md border border-border ${CHECKERBOARD}`}
              aria-hidden="true"
            />
            {messages.colorTransparent}
            {value === null ? <CheckIcon className="ml-auto size-5" aria-hidden="true" /> : null}
          </Button>
        ) : null}
        <div className="grid gap-2">
          <span className="text-sm font-medium">{messages.colorCustom}</span>
          <div className="grid grid-cols-[2.75rem_minmax(0,1fr)] gap-2">
            {/* The input is the square: the system's chooser, the keyboard and the focus ring stay its own; only its inner swatch is hidden. */}
            <input
              type="color"
              className="size-11 cursor-pointer appearance-none rounded-lg border border-border bg-clip-padding p-0 focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-ring [&::-webkit-color-swatch]:opacity-0 [&::-webkit-color-swatch-wrapper]:p-0"
              style={{ backgroundColor: value ?? CUSTOM_START }}
              aria-label={messages.colorCustom}
              value={value ?? CUSTOM_START}
              onInput={({ currentTarget }): void => onChange(currentTarget.value.toUpperCase())}
            />
            <Input
              className="w-28"
              aria-label={`${messages.colorCustom} (#RRGGBB)`}
              maxLength={7}
              spellcheck={false}
              value={typed ?? value ?? ""}
              onInput={({ currentTarget }): void => type(currentTarget.value)}
              onBlur={(): void => setTyped(null)}
            />
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}
