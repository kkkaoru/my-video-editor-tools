// Runs with bun: shadcn base-nova Collapsible.
import { Collapsible as CollapsiblePrimitive } from "@base-ui/react/collapsible";
import type { VNode } from "preact";
import type { PrimitiveProps } from "@/styling/cn.ts";

export const Collapsible = (props: PrimitiveProps<CollapsiblePrimitive.Root.Props>): VNode => (
  <CollapsiblePrimitive.Root data-slot="collapsible" {...props} />
);

export const CollapsibleTrigger = (props: PrimitiveProps<CollapsiblePrimitive.Trigger.Props>): VNode => (
  <CollapsiblePrimitive.Trigger data-slot="collapsible-trigger" {...props} />
);

export const CollapsibleContent = (props: PrimitiveProps<CollapsiblePrimitive.Panel.Props>): VNode => (
  <CollapsiblePrimitive.Panel data-slot="collapsible-content" {...props} />
);
