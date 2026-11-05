// Runs with bun: shadcn base-nova Card.
import type { ComponentProps, VNode } from "preact";
import { cn } from "@/styling/cn.ts";

export const Card = ({ className, ...props }: ComponentProps<"div">): VNode => (
  <div
    data-slot="card"
    className={cn(
      "flex min-w-0 flex-col gap-4 rounded-xl border border-border bg-card py-4 text-sm text-card-foreground shadow-xs",
      className,
    )}
    {...props}
  />
);

export const CardHeader = ({ className, ...props }: ComponentProps<"div">): VNode => (
  <div data-slot="card-header" className={cn("grid auto-rows-min gap-2 px-4", className)} {...props} />
);

export const CardTitle = ({ className, children, ...props }: ComponentProps<"h2">): VNode => (
  <h2
    data-slot="card-title"
    className={cn("min-w-0 text-base leading-snug font-medium break-words", className)}
    {...props}
  >
    {children}
  </h2>
);

export const CardDescription = ({ className, ...props }: ComponentProps<"p">): VNode => (
  <p data-slot="card-description" className={cn("text-sm text-muted-foreground", className)} {...props} />
);

export const CardContent = ({ className, ...props }: ComponentProps<"div">): VNode => (
  <div data-slot="card-content" className={cn("min-w-0 px-4", className)} {...props} />
);
