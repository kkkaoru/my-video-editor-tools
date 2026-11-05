// Runs with bun: shadcn base-nova Switch; a 44 px target around the track, and a checkmark so state is not colour alone.
import { Switch as SwitchPrimitive } from "@base-ui/react/switch";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";

export const Switch = ({ className, ...props }: PrimitiveProps<SwitchPrimitive.Root.Props>): VNode => (
  <SwitchPrimitive.Root
    data-slot="switch"
    className={cn(
      "group inline-flex min-h-11 min-w-11 shrink-0 cursor-pointer items-center justify-center rounded-full bg-transparent",
      className,
    )}
    {...props}
  >
    <span className="relative inline-block h-6 w-10 rounded-full border border-border bg-muted transition-colors group-aria-checked:border-primary group-aria-checked:bg-primary">
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="absolute top-0.5 left-0.5 flex size-[1.125rem] items-center justify-center rounded-full bg-foreground transition-transform group-aria-checked:translate-x-4 group-aria-checked:bg-primary-foreground"
      />
    </span>
  </SwitchPrimitive.Root>
);
