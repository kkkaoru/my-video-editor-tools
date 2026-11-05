// Runs with bun: shadcn base-nova Alert (destructive only, the one variant this app shows).
import type { ComponentProps, VNode } from "preact";
import { cn } from "@/styling/cn.ts";

export const Alert = ({ className, ...props }: ComponentProps<"div">): VNode => (
  <div
    data-slot="alert"
    role="alert"
    className={cn(
      "flex w-full gap-2 rounded-lg border border-destructive bg-card px-3 py-2 text-left text-sm break-words whitespace-pre-wrap text-destructive [&_svg]:mt-0.5 [&_svg]:size-4 [&_svg]:shrink-0",
      className,
    )}
    {...props}
  />
);
