// Runs with bun: shadcn base-nova AlertDialog, kept inside <main> (WCAG 1.3.6).
import { AlertDialog as AlertDialogPrimitive } from "@base-ui/react/alert-dialog";
import type { VNode } from "preact";
import { cn, type PrimitiveProps } from "@/styling/cn.ts";
import { MAIN_LANDMARK } from "./select.tsx";

export const AlertDialog = AlertDialogPrimitive.Root;
export const AlertDialogTrigger = AlertDialogPrimitive.Trigger;
export const AlertDialogClose = AlertDialogPrimitive.Close;

export const AlertDialogContent = ({
  className,
  ...props
}: PrimitiveProps<AlertDialogPrimitive.Popup.Props>): VNode => (
  <AlertDialogPrimitive.Portal container={MAIN_LANDMARK}>
    <AlertDialogPrimitive.Popup
      data-slot="alert-dialog-content"
      className={cn(
        "fixed top-1/2 left-1/2 z-50 grid max-h-[calc(100vh-2rem)] w-[28rem] max-w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 gap-3 overflow-y-auto rounded-xl border border-border bg-popover p-4 text-sm text-popover-foreground shadow-lg",
        className,
      )}
      {...props}
    />
  </AlertDialogPrimitive.Portal>
);

export const AlertDialogTitle = ({
  className,
  ...props
}: PrimitiveProps<AlertDialogPrimitive.Title.Props>): VNode => (
  <AlertDialogPrimitive.Title
    data-slot="alert-dialog-title"
    className={cn("text-base font-semibold", className)}
    {...props}
  />
);

export const AlertDialogDescription = ({
  className,
  ...props
}: PrimitiveProps<AlertDialogPrimitive.Description.Props>): VNode => (
  <AlertDialogPrimitive.Description
    data-slot="alert-dialog-description"
    className={cn("text-muted-foreground", className)}
    {...props}
  />
);
