// Runs with bun: shadcn base-nova Combobox; 44 px rows, and the popup stays inside <main> (WCAG 1.3.6).
import { Combobox as ComboboxPrimitive } from "@base-ui/react/combobox";
import { CheckIcon, ChevronDownIcon } from "lucide-preact";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";
import { MAIN_LANDMARK } from "./select.tsx";

export const Combobox = ComboboxPrimitive.Root;
export const ComboboxValue = ComboboxPrimitive.Value;

/** The search box lives inside the popup: with it outside, the library hides the whole page from assistive tech while open. */
export const ComboboxTrigger = ({
  className,
  children,
  ...props
}: PrimitiveProps<ComboboxPrimitive.Trigger.Props>): VNode => (
  <ComboboxPrimitive.Trigger
    data-slot="combobox-trigger"
    className={cn(
      "flex min-h-11 w-full items-center justify-between gap-2 rounded-lg border border-border bg-background px-3 py-2 text-left text-sm text-foreground transition-colors select-none hover:bg-muted",
      className,
    )}
    {...props}
  >
    {children}
    <ChevronDownIcon
      className="pointer-events-none size-4 shrink-0 text-muted-foreground"
      aria-hidden="true"
    />
  </ComboboxPrimitive.Trigger>
);

export const ComboboxInput = ({
  className,
  ...props
}: PrimitiveProps<ComboboxPrimitive.Input.Props>): VNode => (
  <ComboboxPrimitive.Input
    data-slot="combobox-input"
    className={cn(
      "min-h-11 w-full min-w-0 rounded-lg border border-border bg-background px-3 py-1 text-sm text-foreground transition-colors",
      className,
    )}
    {...props}
  />
);

export const ComboboxContent = ({
  className,
  ...props
}: PrimitiveProps<ComboboxPrimitive.Popup.Props>): VNode => (
  <ComboboxPrimitive.Portal container={MAIN_LANDMARK}>
    <ComboboxPrimitive.Positioner sideOffset={4} className="isolate z-50">
      <ComboboxPrimitive.Popup
        data-slot="combobox-content"
        className={cn(
          "max-h-(--available-height) w-(--anchor-width) min-w-36 overflow-y-auto rounded-lg border border-border bg-popover p-1 text-sm text-popover-foreground shadow-md",
          className,
        )}
        {...props}
      />
    </ComboboxPrimitive.Positioner>
  </ComboboxPrimitive.Portal>
);

export const ComboboxList = ({
  className,
  ...props
}: PrimitiveProps<ComboboxPrimitive.List.Props>): VNode => (
  <ComboboxPrimitive.List data-slot="combobox-list" className={cn("max-h-72", className)} {...props} />
);

export const ComboboxItem = ({
  className,
  children,
  ...props
}: PrimitiveProps<ComboboxPrimitive.Item.Props>): VNode => (
  <ComboboxPrimitive.Item
    data-slot="combobox-item"
    className={cn(
      "relative flex min-h-11 w-full cursor-default items-center gap-2 rounded-md py-1 pr-8 pl-2 select-none data-highlighted:bg-accent data-highlighted:text-accent-foreground",
      className,
    )}
    {...props}
  >
    {children}
    <ComboboxPrimitive.ItemIndicator className="pointer-events-none absolute right-2 flex size-4 items-center justify-center">
      <CheckIcon className="size-4" aria-hidden="true" />
    </ComboboxPrimitive.ItemIndicator>
  </ComboboxPrimitive.Item>
);

export const ComboboxEmpty = ({
  className,
  ...props
}: PrimitiveProps<ComboboxPrimitive.Empty.Props>): VNode => (
  <ComboboxPrimitive.Empty
    data-slot="combobox-empty"
    className={cn("px-2 empty:hidden py-2 text-muted-foreground", className)}
    {...props}
  />
);

export const ComboboxStatus = ({
  className,
  ...props
}: PrimitiveProps<ComboboxPrimitive.Status.Props>): VNode => (
  <ComboboxPrimitive.Status
    data-slot="combobox-status"
    className={cn("px-2 py-2 text-muted-foreground", className)}
    {...props}
  />
);
