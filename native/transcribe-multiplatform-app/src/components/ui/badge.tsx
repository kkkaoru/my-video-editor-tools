// Runs with bun: shadcn base-nova Badge.
import { cva, type VariantProps } from "class-variance-authority";
import type { ComponentProps, VNode } from "preact";
import { cn } from "@/styling/cn.ts";

const badgeVariants = cva(
  "inline-flex min-h-6 w-fit shrink-0 items-center justify-center gap-1 rounded-4xl border px-2 py-0.5 text-xs font-medium whitespace-nowrap",
  {
    variants: {
      variant: {
        default: "border-transparent bg-primary text-primary-foreground",
        outline: "border-border text-foreground",
        success: "border-success text-success",
        destructive: "border-destructive text-destructive",
      },
    },
    defaultVariants: { variant: "outline" },
  },
);

export type BadgeProps = ComponentProps<"span"> & VariantProps<typeof badgeVariants>;

export const Badge = ({ className, variant, ...props }: BadgeProps): VNode => (
  <span data-slot="badge" className={cn(badgeVariants({ variant }), className)} {...props} />
);
