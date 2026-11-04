// Runs with bun: shadcn base-nova Button; 44 px targets and solid hover colours for WCAG AAA.
import { Button as ButtonPrimitive } from "@base-ui/react/button";
import { cva, type VariantProps } from "class-variance-authority";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";

const buttonVariants = cva(
  "inline-flex min-h-11 max-w-full min-w-11 items-center justify-center gap-2 rounded-lg border border-transparent px-4 py-2 text-left text-sm font-medium break-words transition-colors select-none disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground hover:bg-primary-hover",
        outline: "border-border bg-background text-foreground hover:bg-muted",
      },
    },
    defaultVariants: { variant: "default" },
  },
);

export type ButtonProps = PrimitiveProps<ButtonPrimitive.Props> & VariantProps<typeof buttonVariants>;

export const Button = ({ className, variant, ...props }: ButtonProps): VNode => (
  <ButtonPrimitive data-slot="button" className={cn(buttonVariants({ variant, className }))} {...props} />
);
