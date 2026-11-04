// Runs with bun: shadcn base-nova ToggleGroup; the pressed state also changes weight, so it does not rely on colour alone.
import { Toggle as TogglePrimitive } from "@base-ui/react/toggle";
import { ToggleGroup as ToggleGroupPrimitive } from "@base-ui/react/toggle-group";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";

export const ToggleGroup = ({ className, ...props }: PrimitiveProps<ToggleGroupPrimitive.Props>): VNode => (
  <ToggleGroupPrimitive
    data-slot="toggle-group"
    className={cn("flex flex-wrap items-center gap-2", className)}
    {...props}
  />
);

export const ToggleGroupItem = ({ className, ...props }: PrimitiveProps<TogglePrimitive.Props>): VNode => (
  <TogglePrimitive
    data-slot="toggle-group-item"
    className={cn(
      "inline-flex min-h-11 max-w-full min-w-11 items-center justify-center rounded-lg border border-border bg-background px-3 py-2 text-sm font-medium text-foreground transition-colors hover:bg-muted aria-pressed:border-primary aria-pressed:bg-primary aria-pressed:font-semibold aria-pressed:text-primary-foreground",
      className,
    )}
    {...props}
  />
);
