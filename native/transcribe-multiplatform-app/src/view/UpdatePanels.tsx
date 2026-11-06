// Runs with bun: the update offered at the top of the window, and the check on the settings tab.
import type { VNode } from "preact";
import { Alert } from "@/components/ui/alert.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { Progress } from "@/components/ui/progress.tsx";
import { errorText, format, type Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import type { Updater } from "../state/use-updater.ts";

export interface UpdateOfferProps {
  updater: Updater;
  /** Videos or model downloads in progress: restarting would stop them. */
  busy: boolean;
}

const PERCENT: number = 100;

export function UpdateOffer({ updater: { state, install }, busy }: UpdateOfferProps): VNode | null {
  const messages: Messages = useMessages();
  if (state.available === null) {
    return null;
  }
  const share: number | null = state.progress;
  return (
    <Card data-slot="update-offer">
      <CardHeader>
        <CardTitle>{format(messages.updateAvailable, { version: state.available.version })}</CardTitle>
      </CardHeader>
      <CardContent className="grid grid-cols-1 gap-2">
        {state.available.notes === null || state.available.notes === "" ? null : (
          <p className="max-w-[min(65ch,40em)] whitespace-pre-line text-muted-foreground">
            {state.available.notes}
          </p>
        )}
        {state.installing ? (
          <>
            <p role="status">{messages.updateInstalling}</p>
            <Progress
              value={share}
              min={0}
              max={1}
              aria-label={messages.updateProgressLabel}
              getAriaValueText={(): string => (share === null ? "" : `${Math.round(share * PERCENT)}%`)}
            />
          </>
        ) : (
          <>
            {state.installError === null ? null : (
              <Alert>
                {format(messages.updateInstallFailed, {
                  detail: errorText(messages, state.installError, ""),
                })}
              </Alert>
            )}
            <div className="flex flex-wrap items-center gap-2">
              <Button variant="outline" disabled={busy} onClick={install}>
                {state.installError === null ? messages.updateInstall : messages.updateRetry}
              </Button>
              {busy ? <p className="text-muted-foreground">{messages.updateBusy}</p> : null}
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}

export function UpdateCheck({ updater: { state, check } }: { updater: Updater }): VNode {
  const messages: Messages = useMessages();
  return (
    <div data-slot="update-check" className="grid grid-cols-1 justify-items-start gap-2">
      <Button variant="outline" disabled={state.checking || state.installing} onClick={check}>
        {messages.updateCheck}
      </Button>
      {state.checking ? <p role="status">{messages.updateChecking}</p> : null}
      {state.upToDate ? <p role="status">{messages.updateUpToDate}</p> : null}
      {state.available === null || state.checking ? null : (
        <p role="status">{format(messages.updateAvailable, { version: state.available.version })}</p>
      )}
      {state.checkError === null ? null : (
        <Alert>
          {format(messages.updateCheckFailed, { detail: errorText(messages, state.checkError, "") })}
        </Alert>
      )}
    </div>
  );
}
