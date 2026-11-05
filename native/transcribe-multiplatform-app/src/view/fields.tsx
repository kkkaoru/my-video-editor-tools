// Runs with bun: labelled controls shared by the panes.
import type { ComponentChildren, VNode } from "preact";
import { useId } from "preact/hooks";
import { Input } from "@/components/ui/input.tsx";
import { Label } from "@/components/ui/label.tsx";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select.tsx";
import { Slider } from "@/components/ui/slider.tsx";

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
  /** For a label in another language than the page. */
  lang?: string;
}

export interface LayerAdjustProps {
  label: string;
  value: number;
  min: number;
  max: number;
  scale: number;
  step: number;
  unit: string;
  disabled: boolean;
  /** The row has no colour square, so the slider takes its place in a narrow window. */
  wide?: boolean;
  onChange: (value: number) => void;
}

export interface SelectFieldProps {
  label: string;
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  /** Shown beside the control, so a popup opening sideways does not cover it. */
  help?: ComponentChildren;
}

const SHOWN_DECIMALS: number = 10;

/** A half-typed or out-of-range number leaves the saved value as it is. */
const typedNumber = ({ validity, value }: HTMLInputElement): number | null =>
  validity.valid && value !== "" ? Number(value) : null;

export function whenPresent<Value>(next: Value | null, apply: (value: Value) => void): void {
  if (next !== null) {
    apply(next);
  }
}

/** Typing is the single-pointer alternative to dragging, and the precise way to fine-tune. */
export function AdjustField({ label, value, min, max, step, onChange }: AdjustFieldProps): VNode {
  const inputId: string = useId();
  return (
    <div className="grid grid-cols-1 gap-2">
      <Label htmlFor={inputId}>{label}</Label>
      <div className="flex flex-wrap items-center gap-2">
        <Slider
          className="min-w-40 flex-1"
          thumbLabel={label}
          value={value}
          min={min}
          max={max}
          step={step}
          onValueChange={(next: number): void => onChange(next)}
        />
        <Input
          id={inputId}
          type="number"
          className="w-24"
          value={value}
          min={min}
          max={max}
          step={step}
          onInput={({ currentTarget }): void => whenPresent(typedNumber(currentTarget), onChange)}
        />
      </div>
    </div>
  );
}

/** A slider, a number and its unit, as cells of a layer row; stored in the backend's unit, shown `scale` times as large. */
export const LayerAdjust = ({
  label,
  value,
  min,
  max,
  scale,
  step,
  unit,
  disabled,
  wide,
  onChange,
}: LayerAdjustProps): VNode => (
  <>
    <Slider
      className={wide === true ? "col-span-2 sm:col-span-1" : ""}
      thumbLabel={label}
      disabled={disabled}
      value={value * scale}
      min={min * scale}
      max={max * scale}
      step={step}
      onValueChange={(next: number): void => onChange(next / scale)}
    />
    <Input
      type="number"
      className="disabled:cursor-not-allowed disabled:opacity-50"
      aria-label={label}
      disabled={disabled}
      value={Math.round(value * scale * SHOWN_DECIMALS) / SHOWN_DECIMALS}
      min={min * scale}
      max={max * scale}
      step="any"
      onInput={({ currentTarget }): void =>
        whenPresent(typedNumber(currentTarget), (typed: number): void => onChange(typed / scale))
      }
    />
    <span className="text-sm" aria-hidden="true">
      {unit}
    </span>
  </>
);

export const SelectField = ({ label, value, options, onChange, help }: SelectFieldProps): VNode => (
  <Select
    items={options}
    value={value}
    onValueChange={(next: string | null): void => whenPresent(next, onChange)}
  >
    <div className="grid grid-cols-1 gap-2">
      <span className="text-sm font-medium">{label}</span>
      <div className="flex items-center gap-2">
        <SelectTrigger aria-label={label} className="min-w-0 flex-1">
          <SelectValue />
        </SelectTrigger>
        {help}
      </div>
    </div>
    <SelectContent>
      {options.map(
        ({ value: optionValue, label: optionLabel, disabled, lang }: SelectOption): VNode => (
          <SelectItem key={optionValue} value={optionValue} disabled={disabled} lang={lang}>
            {optionLabel}
          </SelectItem>
        ),
      )}
    </SelectContent>
  </Select>
);

/** The whole row is the 44 px target. */
