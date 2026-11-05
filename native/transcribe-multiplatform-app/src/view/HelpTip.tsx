// Runs with bun: a "?" that explains on hover and on click; focus, Enter and Escape work too (WCAG 1.4.13, 2.1.1).
import type { ComponentChildren, VNode } from "preact";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover.tsx";

export interface HelpTipProps {
  label: string;
  children: ComponentChildren;
}

export const HelpTip = ({ label, children }: HelpTipProps): VNode => (
  <Popover>
    <PopoverTrigger aria-label={label} openOnHover>
      ?
    </PopoverTrigger>
    <PopoverContent aria-label={label}>{children}</PopoverContent>
  </Popover>
);
