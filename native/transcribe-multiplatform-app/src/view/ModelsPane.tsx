// Runs with bun: download, reuse and remove speech and face models.
import { DownloadIcon, Trash2Icon, XIcon } from "lucide-preact";
import type { VNode } from "preact";
import { Alert } from "@/components/ui/alert.tsx";
import { Badge } from "@/components/ui/badge.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { Progress } from "@/components/ui/progress.tsx";
import {
  cancelDownload,
  deleteModel,
  pickFolder,
  resetModels,
  saveSettings,
  startDownload,
} from "../backend/backend.ts";
import type { ModelStatus, Role, Settings, Snapshot } from "../backend/types.ts";
import { errorText, format, type Messages, type ModelNotes, roleName } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import { type AppState, type DownloadRow, startedDownload } from "../state/app-state.ts";
import { type AppStore, reportFailure } from "../state/use-app-store.ts";
import { ConfirmButton } from "./ConfirmButton.tsx";

export interface ModelsPaneProps {
  store: AppStore;
  snapshot: Snapshot;
}

interface ModelRowProps {
  model: ModelStatus;
  store: AppStore;
  snapshot: Snapshot;
  download: DownloadRow | undefined;
}

/** Decimal, as the published sizes and the system's own file sizes are. */
const GB: number = 1_000_000_000;
const MB: number = 1_000_000;
const ROLE_ORDER: Role[] = ["transcribe", "diarize", "separate", "face", "runtime"];

export const formatBytes = (bytes: number): string =>
  bytes >= GB ? `${(bytes / GB).toFixed(1)} GB` : `${Math.round(bytes / MB) || Math.ceil(bytes / MB)} MB`;

const isOwnCopy = (model: ModelStatus, snapshot: Snapshot): boolean =>
  // The runtime lives outside the models folder, and may be a broken download that has to go.
  model.role === "runtime" || model.found_at?.startsWith(snapshot.models_dir) === true;

const hasNote = (notes: ModelNotes, id: string): id is keyof ModelNotes => id in notes;

const noteFor = (notes: ModelNotes, id: string): string => (hasNote(notes, id) ? notes[id] : "");

const canDownload = (model: ModelStatus): boolean =>
  model.download_bytes > 0 && model.found_at === null && model.blocked === null;

const totalBytes = ([first, ...rest]: ModelStatus[]): number =>
  first === undefined ? 0 : first.download_bytes + totalBytes(rest);

/** What this computer lacks of the models recommended for it. A machine with no recommended speaker model chooses one itself. */
export const missingDownloads = ({ models }: Snapshot): ModelStatus[] =>
  models.filter((model: ModelStatus): boolean => model.recommended && canDownload(model));

export const hasRecommendedSpeakerModel = ({ models }: Snapshot): boolean =>
  models.some(({ role, recommended }: ModelStatus): boolean => role === "diarize" && recommended);

function startDownloads(store: AppStore, models: ModelStatus[]): void {
  models.forEach(({ id, download_bytes: bytes }: ModelStatus): void => {
    store.setState((state: AppState): AppState => startedDownload(state, id, bytes));
    startDownload(id).catch((error: unknown): void => reportFailure(store.setState, error));
  });
}

function DownloadButtons({ model, store, snapshot, download }: ModelRowProps): VNode | null {
  const messages: Messages = useMessages();
  const name: string = model.label;
  const fail = (error: unknown): void => reportFailure(store.setState, error);
  if (download?.running === true) {
    return (
      <Button variant="outline" onClick={(): void => void cancelDownload(model.id).catch(fail)}>
        <XIcon aria-hidden="true" />
        {format(messages.downloadCancel, { name })}
      </Button>
    );
  }
  if (canDownload(model)) {
    return (
      <Button onClick={(): void => startDownloads(store, [model])}>
        <DownloadIcon aria-hidden="true" />
        {format(messages.download, { name, size: formatBytes(model.download_bytes) })}
      </Button>
    );
  }
  return isOwnCopy(model, snapshot) ? (
    <Button
      variant="outline"
      onClick={(): void => void deleteModel(model.id).then(store.applySnapshot).catch(fail)}
    >
      <Trash2Icon aria-hidden="true" />
      {format(messages.deleteModel, { name })}
    </Button>
  ) : null;
}

function DownloadState({
  model,
  download,
}: {
  model: ModelStatus;
  download: DownloadRow | undefined;
}): VNode | null {
  const messages: Messages = useMessages();
  if (download === undefined) {
    return null;
  }
  if (download.error !== null) {
    return <Alert>{errorText(messages, download.error, model.label)}</Alert>;
  }
  const text: string = format(messages.downloadProgress, {
    done: formatBytes(download.doneBytes),
    total: formatBytes(download.totalBytes),
  });
  return (
    <div className="grid grid-cols-1 gap-1">
      <p className="text-muted-foreground tabular-nums">{text}</p>
      <Progress
        value={download.totalBytes > 0 ? download.doneBytes / download.totalBytes : null}
        min={0}
        max={1}
        aria-label={format(messages.downloadProgressLabel, { name: model.label })}
        getAriaValueText={(): string => text}
      />
    </div>
  );
}

/** Offers one model where it is missed, with the same download controls as its row in the models tab. */
export function ModelDownload({
  id,
  store,
  snapshot,
}: {
  id: string;
  store: AppStore;
  snapshot: Snapshot;
}): VNode | null {
  const model: ModelStatus | undefined = snapshot.models.find((row: ModelStatus): boolean => row.id === id);
  if (model === undefined) {
    return null;
  }
  const download: DownloadRow | undefined = store.state.downloads.get(id);
  return (
    <div data-slot="model-download" className="grid grid-cols-1 justify-items-start gap-2">
      <DownloadButtons model={model} store={store} snapshot={snapshot} download={download} />
      <DownloadState model={model} download={download} />
    </div>
  );
}

function ModelRow({ model, store, snapshot, download }: ModelRowProps): VNode {
  const messages: Messages = useMessages();
  return (
    <li className="grid gap-2 border-t border-border pt-3 first:border-t-0 first:pt-0">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="font-medium">{model.label}</h3>
        {model.recommended ? <Badge variant="default">{messages.badgeRecommended}</Badge> : null}
        {model.role === "separate" ? <Badge>{messages.badgeUsedBySteps}</Badge> : null}
        {model.found_at === null ? null : <Badge variant="success">{messages.badgePresent}</Badge>}
      </div>
      <p className="text-muted-foreground">{noteFor(messages.modelNotes, model.id)}</p>
      {model.blocked === null ? null : (
        <p className="text-destructive">{errorText(messages, model.blocked, model.label)}</p>
      )}
      {model.found_at === null ? null : (
        <p className="break-all text-muted-foreground">
          {format(messages.locationLabel, { path: model.found_at })}
        </p>
      )}
      {model.found_at === null && model.source_url !== "" ? (
        <p className="break-all text-muted-foreground">
          {format(messages.sourceLabel, { url: model.source_url })}
        </p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <DownloadButtons model={model} store={store} snapshot={snapshot} download={download} />
      </div>
      <DownloadState model={model} download={download} />
    </li>
  );
}

async function addFolder({ applySnapshot }: AppStore, settings: Settings): Promise<void> {
  const folder: string | null = await pickFolder();
  if (folder !== null) {
    applySnapshot(await saveSettings({ ...settings, model_dirs: [...settings.model_dirs, folder] }));
  }
}

export function ModelsPane({ store, snapshot }: ModelsPaneProps): VNode {
  const messages: Messages = useMessages();
  const fail = (error: unknown): void => reportFailure(store.setState, error);
  const needed: ModelStatus[] = missingDownloads(snapshot).filter(
    ({ id }: ModelStatus): boolean => store.state.downloads.get(id)?.running !== true,
  );
  return (
    <div className="grid grid-cols-1 gap-4">
      <Card>
        <CardHeader>
          <CardTitle>{messages.modelsHeading}</CardTitle>
          <CardDescription>{messages.modelsHint}</CardDescription>
        </CardHeader>
        <CardContent className="grid grid-cols-1 gap-2">
          {hasRecommendedSpeakerModel(snapshot) ? null : <p>{messages.noRecommendedSpeakerModel}</p>}
          <div className="flex flex-wrap gap-2">
            {needed.length === 0 ? null : (
              <Button onClick={(): void => startDownloads(store, needed)}>
                <DownloadIcon aria-hidden="true" />
                {format(messages.downloadAll, {
                  label: messages.downloadAllLabel,
                  count: needed.length,
                  size: formatBytes(totalBytes(needed)),
                })}
              </Button>
            )}
            <Button
              variant="outline"
              onClick={(): void => void addFolder(store, snapshot.settings).catch(fail)}
            >
              {messages.addModelFolder}
            </Button>
            <ConfirmButton
              title={messages.resetModelsTitle}
              description={format(messages.resetModelsDescription, { path: snapshot.models_dir })}
              confirmLabel={messages.resetModelsConfirm}
              cancelLabel={messages.dialogCancel}
              onConfirm={(): void =>
                void resetModels()
                  .then((fresh: Snapshot): void => {
                    store.applySnapshot(fresh);
                    startDownloads(store, missingDownloads(fresh));
                  })
                  .catch(fail)
              }
            >
              <Trash2Icon aria-hidden="true" />
              {messages.resetModels}
            </ConfirmButton>
          </div>
          {snapshot.settings.model_dirs.length === 0 ? null : (
            <p className="break-all text-muted-foreground">
              {messages.modelFolders}: {snapshot.settings.model_dirs.join(" / ")}
            </p>
          )}
        </CardContent>
      </Card>
      {ROLE_ORDER.filter((role: Role): boolean =>
        snapshot.models.some((model: ModelStatus): boolean => model.role === role),
      ).map(
        (role: Role): VNode => (
          <Card key={role}>
            <CardHeader>
              <CardTitle>{roleName(messages, role)}</CardTitle>
              {role === "diarize" ? <CardDescription>{messages.speakerRolesHint}</CardDescription> : null}
            </CardHeader>
            <CardContent>
              <ul className="grid grid-cols-1 gap-3">
                {snapshot.models
                  .filter((model: ModelStatus): boolean => model.role === role)
                  .map(
                    (model: ModelStatus): VNode => (
                      <ModelRow
                        key={model.id}
                        model={model}
                        store={store}
                        snapshot={snapshot}
                        download={store.state.downloads.get(model.id)}
                      />
                    ),
                  )}
              </ul>
            </CardContent>
          </Card>
        ),
      )}
    </div>
  );
}
