// Runs with bun: shadcn base-nova Input with a 44 px target.
import { Input as InputPrimitive } from "@base-ui/react/input";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";

export const Input = ({ className, ...props }: PrimitiveProps<InputPrimitive.Props>): VNode => (
  <InputPrimitive
    data-slot="input"
    className={cn(
      "min-h-11 w-full min-w-0 rounded-lg border border-border bg-background px-3 py-1 text-sm text-foreground transition-colors",
      className,
    )}
    {...props}
  />
);
