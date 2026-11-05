// Runs with bun: shadcn base-nova Slider; the thumb's hit area is 44 px around a small visible dot (WCAG 2.5.5).
import { Slider as SliderPrimitive } from "@base-ui/react/slider";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";

export interface SliderProps extends PrimitiveProps<SliderPrimitive.Root.Props<number>> {
  thumbLabel: string;
}

export const Slider = ({ className, thumbLabel, ...props }: SliderProps): VNode => (
  <SliderPrimitive.Root
    data-slot="slider"
    thumbAlignment="edge"
    className={cn("w-full data-[disabled]:opacity-50", className)}
    {...props}
  >
    <SliderPrimitive.Control className="relative flex min-h-11 w-full touch-none items-center select-none">
      <SliderPrimitive.Track
        data-slot="slider-track"
        className="relative h-1.5 w-full rounded-full bg-border select-none"
      >
        <SliderPrimitive.Indicator
          data-slot="slider-range"
          className="h-full rounded-full bg-primary select-none"
        />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb
        data-slot="slider-thumb"
        aria-label={thumbLabel}
        className="flex size-11 shrink-0 items-center justify-center rounded-full select-none before:block before:size-4 before:rounded-full before:border-2 before:border-primary before:bg-background has-focus-visible:outline-3 has-focus-visible:outline-offset-2 has-focus-visible:outline-ring"
      />
    </SliderPrimitive.Control>
  </SliderPrimitive.Root>
);
