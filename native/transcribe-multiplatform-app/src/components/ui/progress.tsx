// Runs with bun: shadcn base-nova Progress; the indeterminate state is drawn without motion (WCAG 2.2.2).
import { Progress as ProgressPrimitive } from "@base-ui/react/progress";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";

export const Progress = ({ className, ...props }: PrimitiveProps<ProgressPrimitive.Root.Props>): VNode => (
  <ProgressPrimitive.Root data-slot="progress" className={cn("w-full", className)} {...props}>
    <ProgressPrimitive.Track
      data-slot="progress-track"
      className="relative flex h-2 w-full items-center overflow-hidden rounded-full border border-border bg-muted"
    >
      <ProgressPrimitive.Indicator
        data-slot="progress-indicator"
        className="h-full bg-primary transition-all data-indeterminate:w-full data-indeterminate:bg-muted-foreground"
      />
    </ProgressPrimitive.Track>
  </ProgressPrimitive.Root>
);
