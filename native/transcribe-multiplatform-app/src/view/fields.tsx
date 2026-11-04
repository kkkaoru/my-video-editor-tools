// Runs with bun: labelled controls shared by the panes.
import type { ComponentChildren, VNode } from "preact";
import { useId } from "preact/hooks";
import { Input } from "@/components/ui/input.tsx";
import { Label } from "@/components/ui/label.tsx";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select.tsx";
import { Slider } from "@/components/ui/slider.tsx";
import { Switch } from "@/components/ui/switch.tsx";

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
  /** Shown beside the label. */
  help?: ComponentChildren;
}

export interface SwitchFieldProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

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
    <div className="grid grid-cols-1 gap-1">
      <Label htmlFor={inputId}>{label}</Label>
      <div className="flex flex-wrap items-center gap-3">
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

export const SelectField = ({ label, value, options, onChange, help }: SelectFieldProps): VNode => (
  <Select
    items={options}
    value={value}
    onValueChange={(next: string | null): void => whenPresent(next, onChange)}
  >
    <div className="grid grid-cols-1 gap-1">
      <div className="flex items-center gap-2">
        <span className="text-sm font-medium">{label}</span>
        {help}
      </div>
      <SelectTrigger aria-label={label}>
        <SelectValue />
      </SelectTrigger>
    </div>
    <SelectContent>
      {options.map(
        ({ value: optionValue, label: optionLabel, disabled }: SelectOption): VNode => (
          <SelectItem key={optionValue} value={optionValue} disabled={disabled}>
            {optionLabel}
          </SelectItem>
        ),
      )}
    </SelectContent>
  </Select>
);

/** The whole row is the 44 px target. */
export function SwitchField({ label, checked, onChange }: SwitchFieldProps): VNode {
  const labelId: string = useId();
  return (
    <div className="flex min-h-11 items-center justify-between gap-3">
      <span id={labelId} className="text-sm font-medium">
        {label}
      </span>
      <Switch aria-labelledby={labelId} checked={checked} onCheckedChange={onChange} />
    </div>
  );
}
