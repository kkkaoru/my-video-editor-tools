// Runs with bun: shadcn base-nova Select; 44 px rows, and the popup stays inside <main> (WCAG 1.3.6).
import { Select as SelectPrimitive } from "@base-ui/react/select";
import { CheckIcon, ChevronDownIcon } from "lucide-preact";
import type { RefObject, VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";

// Read when the popup opens: <main> does not exist yet while this component first renders.
export const MAIN_LANDMARK: RefObject<HTMLElement | null> = {
  get current(): HTMLElement | null {
    return document.querySelector("main");
  },
};

export const Select = SelectPrimitive.Root;

export const SelectValue = ({ className, ...props }: PrimitiveProps<SelectPrimitive.Value.Props>): VNode => (
  <SelectPrimitive.Value
    data-slot="select-value"
    className={cn("flex flex-1 text-left", className)}
    {...props}
  />
);

export const SelectTrigger = ({
  className,
  children,
  ...props
}: PrimitiveProps<SelectPrimitive.Trigger.Props>): VNode => (
  <SelectPrimitive.Trigger
    data-slot="select-trigger"
    className={cn(
      "flex min-h-11 w-full items-center justify-between gap-2 rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground transition-colors select-none hover:bg-muted",
      className,
    )}
    {...props}
  >
    {children}
    <SelectPrimitive.Icon>
      <ChevronDownIcon className="pointer-events-none size-4 text-muted-foreground" aria-hidden="true" />
    </SelectPrimitive.Icon>
  </SelectPrimitive.Trigger>
);

export const SelectContent = ({
  className,
  children,
  ...props
}: PrimitiveProps<SelectPrimitive.Popup.Props>): VNode => (
  <SelectPrimitive.Portal container={MAIN_LANDMARK}>
    <SelectPrimitive.Positioner
      positionMethod="fixed"
      sideOffset={4}
      alignItemWithTrigger={false}
      className="isolate z-50"
    >
      <SelectPrimitive.Popup
        data-slot="select-content"
        className={cn(
          "max-h-(--available-height) w-(--anchor-width) min-w-36 overflow-y-auto rounded-lg border border-border bg-popover p-1 text-popover-foreground shadow-md",
          className,
        )}
        {...props}
      >
        <SelectPrimitive.List>{children}</SelectPrimitive.List>
      </SelectPrimitive.Popup>
    </SelectPrimitive.Positioner>
  </SelectPrimitive.Portal>
);

export const SelectItem = ({
  className,
  children,
  ...props
}: PrimitiveProps<SelectPrimitive.Item.Props>): VNode => (
  <SelectPrimitive.Item
    data-slot="select-item"
    className={cn(
      "relative flex min-h-11 w-full cursor-default items-center gap-2 rounded-md py-1 pr-8 pl-2 text-sm select-none data-disabled:line-through data-highlighted:bg-accent data-highlighted:text-accent-foreground",
      className,
    )}
    {...props}
  >
    <SelectPrimitive.ItemText className="flex flex-1 gap-2">{children}</SelectPrimitive.ItemText>
    <SelectPrimitive.ItemIndicator className="pointer-events-none absolute right-2 flex size-4 items-center justify-center">
      <CheckIcon className="size-4" aria-hidden="true" />
    </SelectPrimitive.ItemIndicator>
  </SelectPrimitive.Item>
);
