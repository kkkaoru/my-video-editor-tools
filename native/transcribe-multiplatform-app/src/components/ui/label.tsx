// Runs with bun: shadcn base-nova Label.
import type { ComponentProps, VNode } from "preact";
import { cn } from "@/styling/cn.ts";

export const Label = ({ className, ...props }: ComponentProps<"label">): VNode => (
  // biome-ignore lint/a11y/noLabelWithoutControl: every caller passes htmlFor.
  <label data-slot="label" className={cn("block text-sm font-medium", className)} {...props} />
);
