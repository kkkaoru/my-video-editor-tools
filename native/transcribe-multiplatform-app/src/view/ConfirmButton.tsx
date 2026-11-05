// Runs with bun: a button that asks first, for actions that cannot be undone.
import type { ComponentChildren, VNode } from "preact";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog.tsx";
import { Button } from "@/components/ui/button.tsx";

export interface ConfirmButtonProps {
  children: ComponentChildren;
  title: string;
  description: string;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void;
}

export const ConfirmButton = ({
  children,
  title,
  description,
  confirmLabel,
  cancelLabel,
  onConfirm,
}: ConfirmButtonProps): VNode => (
  <AlertDialog>
    <AlertDialogTrigger render={<Button variant="outline" />}>{children}</AlertDialogTrigger>
    <AlertDialogContent>
      <AlertDialogTitle>{title}</AlertDialogTitle>
      <AlertDialogDescription>{description}</AlertDialogDescription>
      <div className="flex flex-wrap justify-end gap-2">
        <AlertDialogClose render={<Button variant="outline" />}>{cancelLabel}</AlertDialogClose>
        <AlertDialogClose render={<Button onClick={onConfirm} />}>{confirmLabel}</AlertDialogClose>
      </div>
    </AlertDialogContent>
  </AlertDialog>
);
