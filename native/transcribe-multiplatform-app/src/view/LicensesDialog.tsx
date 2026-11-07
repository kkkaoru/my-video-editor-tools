// Runs with bun: the licenses of the libraries in the app, read from the generated list when opened.
import type { VNode } from "preact";
import { useState } from "preact/hooks";
import { Alert } from "@/components/ui/alert.tsx";
import {
  AlertDialog,
  AlertDialogClose,
  AlertDialogContent,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible.tsx";
import { type LicenseEntry, type LicenseList, loadLicenses } from "../backend/licenses.ts";
import type { Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";

export interface LicensesDialogProps {
  load?: () => Promise<LicenseList>;
}

function LicenseRow({ entry, texts }: { entry: LicenseEntry; texts: string[] }): VNode {
  const messages: Messages = useMessages();
  return (
    <li>
      <Collapsible>
        <CollapsibleTrigger
          render={
            <Button
              variant="outline"
              className="h-auto w-full flex-wrap justify-between whitespace-normal text-left"
            />
          }
        >
          <span>
            {entry.name} {entry.version}
          </span>
          <span className="font-normal text-muted-foreground">
            {entry.license || messages.licensesUnknown}
          </span>
        </CollapsibleTrigger>
        <CollapsibleContent className="grid grid-cols-1 gap-2 p-2">
          {entry.url === "" ? null : <p className="break-all text-muted-foreground">{entry.url}</p>}
          {entry.text === null ? (
            <p className="text-muted-foreground">{messages.licensesNoText}</p>
          ) : (
            <pre className="font-sans text-sm break-words whitespace-pre-wrap">{texts[entry.text]}</pre>
          )}
        </CollapsibleContent>
      </Collapsible>
    </li>
  );
}

export function LicensesDialog({ load = loadLicenses }: LicensesDialogProps): VNode {
  const messages: Messages = useMessages();
  const [list, setList] = useState<LicenseList | null>(null);
  const [failed, setFailed] = useState<boolean>(false);
  const open = (opening: boolean): void => {
    if (opening && list === null) {
      setFailed(false);
      load()
        .then(setList)
        .catch((): void => setFailed(true));
    }
  };
  return (
    <AlertDialog onOpenChange={open}>
      <AlertDialogTrigger render={<Button variant="outline" />}>{messages.licensesOpen}</AlertDialogTrigger>
      <AlertDialogContent className="h-[calc(100vh-2rem)] w-[48rem] grid-rows-[auto_minmax(0,1fr)_auto]">
        <AlertDialogTitle>{messages.licensesOpen}</AlertDialogTitle>
        {/* The list scrolls on its own; each row is a button, so the keyboard moves through it. */}
        <section aria-label={messages.licensesOpen} className="min-h-0 overflow-y-auto">
          {failed ? <Alert>{messages.licensesFailed}</Alert> : null}
          {list === null && !failed ? <p role="status">{messages.licensesLoading}</p> : null}
          <ul className="grid grid-cols-1 gap-1">
            {(list?.entries ?? []).map(
              (entry: LicenseEntry): VNode => (
                <LicenseRow key={`${entry.name}@${entry.version}`} entry={entry} texts={list?.texts ?? []} />
              ),
            )}
          </ul>
        </section>
        <div className="flex justify-end">
          <AlertDialogClose render={<Button variant="outline" />}>{messages.licensesClose}</AlertDialogClose>
        </div>
      </AlertDialogContent>
    </AlertDialog>
  );
}
