// Runs with bun: shared accessible controls; every target is at least 44 px (WCAG 2.5.5).
import { Button } from "@base-ui/react/button";
import { Field } from "@base-ui/react/field";
import { NumberField } from "@base-ui/react/number-field";
import { Progress } from "@base-ui/react/progress";
import { Select } from "@base-ui/react/select";
import { Slider } from "@base-ui/react/slider";
import type { ComponentChildren, VNode } from "preact";

export interface ActionButtonProps {
  onClick: () => void;
  children: ComponentChildren;
  primary?: boolean;
  pressed?: boolean;
}

export interface AdjustFieldProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
}

export interface SelectOption {
  value: string;
  label: string;
  disabled: boolean;
}

export interface SelectFieldProps {
  label: string;
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
}

export interface ProgressBarProps {
  label: string;
  /** `null` while the tool reports no percentage. */
  fraction: number | null;
  valueText: string;
}

const BASE_BUTTON: string =
  "min-h-11 min-w-11 rounded-md border border-line bg-panel px-3 py-2 text-left text-ink hover:bg-canvas";
const SELECTED_BUTTON: string =
  "min-h-11 min-w-11 rounded-md border border-accent bg-accent px-3 py-2 text-left text-accent-ink";

export const buttonClass = (selected: boolean): string => (selected ? SELECTED_BUTTON : BASE_BUTTON);

export const ActionButton = ({ onClick, children, primary, pressed }: ActionButtonProps): VNode => (
  <Button
    className={buttonClass(primary === true || pressed === true)}
    onClick={onClick}
    aria-pressed={pressed}
  >
    {children}
  </Button>
);

const firstValue = (value: number | readonly number[]): number =>
  typeof value === "number" ? value : (value[0] ?? 0);

export const AdjustField = ({ label, value, min, max, step, onChange }: AdjustFieldProps): VNode => (
  <Field.Root className="my-3">
    <Field.Label className="block">{label}</Field.Label>
    <div className="flex flex-wrap items-center gap-3">
      <Slider.Root
        className="min-w-40 flex-1"
        value={value}
        min={min}
        max={max}
        step={step}
        onValueChange={(next: number | readonly number[]): void => onChange(firstValue(next))}
      >
        <Slider.Control className="flex min-h-11 items-center">
          <Slider.Track className="h-1.5 w-full rounded bg-line">
            <Slider.Indicator className="rounded bg-accent" />
            <Slider.Thumb
              aria-label={label}
              className="size-11 rounded-full border-4 border-panel bg-accent has-focus-visible:outline-3 has-focus-visible:outline-offset-2 has-focus-visible:outline-ink"
            />
          </Slider.Track>
        </Slider.Control>
      </Slider.Root>
      <NumberField.Root
        value={value}
        min={min}
        max={max}
        step={step}
        onValueChange={(next: number | null): void => {
          if (next !== null) {
            onChange(next);
          }
        }}
      >
        <NumberField.Input
          aria-label={label}
          className="min-h-11 w-24 rounded-md border border-line bg-panel px-2 text-ink"
        />
      </NumberField.Root>
    </div>
  </Field.Root>
);

export const SelectField = ({ label, value, options, onChange }: SelectFieldProps): VNode => (
  <Select.Root
    items={options}
    value={value}
    onValueChange={(next: string | null): void => {
      if (next !== null) {
        onChange(next);
      }
    }}
  >
    <Select.Label className="mt-3 block">{label}</Select.Label>
    <Select.Trigger className={`${BASE_BUTTON} flex w-full items-center justify-between gap-2`}>
      <Select.Value />
      <Select.Icon aria-hidden="true">▾</Select.Icon>
    </Select.Trigger>
    <Select.Portal>
      <Select.Positioner sideOffset={4} className="z-10">
        <Select.Popup className="rounded-md border border-line bg-panel text-ink shadow-lg">
          <Select.List>
            {options.map(
              ({ value: optionValue, label: optionLabel, disabled }: SelectOption): VNode => (
                <Select.Item
                  key={optionValue}
                  value={optionValue}
                  disabled={disabled}
                  className="flex min-h-11 cursor-default items-center gap-2 px-3 py-2 text-ink data-disabled:text-muted data-highlighted:bg-accent data-highlighted:text-accent-ink"
                >
                  <Select.ItemIndicator aria-hidden="true">✓</Select.ItemIndicator>
                  <Select.ItemText>{optionLabel}</Select.ItemText>
                </Select.Item>
              ),
            )}
          </Select.List>
        </Select.Popup>
      </Select.Positioner>
    </Select.Portal>
  </Select.Root>
);

export const ProgressBar = ({ label, fraction, valueText }: ProgressBarProps): VNode => (
  <Progress.Root
    value={fraction}
    min={0}
    max={1}
    aria-label={label}
    getAriaValueText={(): string => valueText}
    className="mt-2"
  >
    <Progress.Track className="h-3 w-full overflow-hidden rounded border border-line bg-panel">
      <Progress.Indicator className="h-full bg-accent data-indeterminate:w-full" />
    </Progress.Track>
  </Progress.Root>
);
