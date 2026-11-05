// Runs with bun: shadcn base-nova Tabs; 44 px triggers and full-contrast inactive text for WCAG AAA.
import { Tabs as TabsPrimitive } from "@base-ui/react/tabs";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";

export const Tabs = ({ className, ...props }: PrimitiveProps<TabsPrimitive.Root.Props>): VNode => (
  <TabsPrimitive.Root data-slot="tabs" className={cn("flex flex-col gap-4", className)} {...props} />
);

export const TabsList = ({ className, ...props }: PrimitiveProps<TabsPrimitive.List.Props>): VNode => (
  <TabsPrimitive.List
    data-slot="tabs-list"
    className={cn(
      "inline-flex w-fit max-w-full flex-wrap items-center gap-1 rounded-lg bg-muted p-1",
      className,
    )}
    {...props}
  />
);

export const TabsTrigger = ({ className, ...props }: PrimitiveProps<TabsPrimitive.Tab.Props>): VNode => (
  <TabsPrimitive.Tab
    data-slot="tabs-trigger"
    className={cn(
      "inline-flex min-h-11 min-w-11 items-center justify-center rounded-md border border-transparent px-3 py-2 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground aria-selected:border-border aria-selected:bg-background aria-selected:font-semibold aria-selected:text-foreground aria-selected:shadow-sm",
      className,
    )}
    {...props}
  />
);

export const TabsContent = ({ className, ...props }: PrimitiveProps<TabsPrimitive.Panel.Props>): VNode => (
  <TabsPrimitive.Panel data-slot="tabs-content" className={cn("flex-1 text-sm", className)} {...props} />
);
