// Runs with bun: shadcn base-nova Popover, kept inside <main> (WCAG 1.3.6).
import { Popover as PopoverPrimitive } from "@base-ui/react/popover";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";
import { MAIN_LANDMARK } from "./select.tsx";

export const Popover = PopoverPrimitive.Root;

export const PopoverTrigger = ({
  className,
  ...props
}: PrimitiveProps<PopoverPrimitive.Trigger.Props>): VNode => (
  <PopoverPrimitive.Trigger
    data-slot="popover-trigger"
    className={cn(
      "inline-flex size-11 shrink-0 items-center justify-center rounded-full border border-border bg-background text-sm font-semibold text-foreground transition-colors hover:bg-muted",
      className,
    )}
    {...props}
  />
);

export const PopoverContent = ({
  className,
  ...props
}: PrimitiveProps<PopoverPrimitive.Popup.Props>): VNode => (
  <PopoverPrimitive.Portal container={MAIN_LANDMARK}>
    <PopoverPrimitive.Positioner
      // Fixed: WebKit keeps counting where an absolute popup used to be in the page's scroll width.
      positionMethod="fixed"
      side="right"
      align="start"
      sideOffset={4}
      collisionPadding={8}
      className="isolate z-50"
    >
      <PopoverPrimitive.Popup
        data-slot="popover-content"
        className={cn(
          "w-80 max-w-[calc(100vw-1rem)] rounded-lg border border-border bg-popover p-3 text-sm text-popover-foreground shadow-md",
          className,
        )}
        {...props}
      />
    </PopoverPrimitive.Positioner>
  </PopoverPrimitive.Portal>
);
