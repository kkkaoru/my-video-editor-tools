// Runs with bun: shadcn base-nova Accordion; a row opens below its 44 px header, one at a time unless asked otherwise.
import { Accordion as AccordionPrimitive } from "@base-ui/react/accordion";
import { ChevronDownIcon } from "lucide-preact";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";

export const Accordion = AccordionPrimitive.Root;

export const AccordionItem = ({
  className,
  ...props
}: PrimitiveProps<AccordionPrimitive.Item.Props>): VNode => (
  <AccordionPrimitive.Item
    data-slot="accordion-item"
    className={cn("border-border not-last:border-b", className)}
    {...props}
  />
);

export const AccordionTrigger = ({
  className,
  children,
  ...props
}: PrimitiveProps<AccordionPrimitive.Trigger.Props>): VNode => (
  <AccordionPrimitive.Header className="flex">
    <AccordionPrimitive.Trigger
      data-slot="accordion-trigger"
      className={cn(
        "group flex min-h-11 flex-1 cursor-pointer items-center gap-3 rounded-md py-2 text-left text-sm font-medium focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-ring",
        className,
      )}
      {...props}
    >
      {children}
      <ChevronDownIcon
        className="size-4 shrink-0 text-muted-foreground group-data-[panel-open]:rotate-180"
        aria-hidden="true"
      />
    </AccordionPrimitive.Trigger>
  </AccordionPrimitive.Header>
);

export const AccordionPanel = ({
  className,
  ...props
}: PrimitiveProps<AccordionPrimitive.Panel.Props>): VNode => (
  <AccordionPrimitive.Panel data-slot="accordion-panel" className={cn("pb-4", className)} {...props} />
);
