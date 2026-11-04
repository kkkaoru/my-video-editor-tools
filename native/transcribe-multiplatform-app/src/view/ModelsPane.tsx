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

const GIB: number = 2 ** 30;
const MIB: number = 2 ** 20;
const ROLE_ORDER: Role[] = ["transcribe", "diarize", "face", "runtime"];

export const formatBytes = (bytes: number): string =>
  bytes >= GIB ? `${(bytes / GIB).toFixed(1)} GB` : `${Math.ceil(bytes / MIB)} MB`;

const isOwnCopy = (model: ModelStatus, snapshot: Snapshot): boolean =>
  model.found_at?.startsWith(snapshot.models_dir) === true;

const hasNote = (notes: ModelNotes, id: string): id is keyof ModelNotes => id in notes;

const noteFor = (notes: ModelNotes, id: string): string => (hasNote(notes, id) ? notes[id] : "");

const canDownload = (model: ModelStatus): boolean =>
  model.download_bytes > 0 && model.found_at === null && model.blocked === null;

const bySize = ({ download_bytes: first }: ModelStatus, { download_bytes: second }: ModelStatus): number =>
  first - second;

const totalBytes = ([first, ...rest]: ModelStatus[]): number =>
  first === undefined ? 0 : first.download_bytes + totalBytes(rest);

const isReadyDiarizer = ({ role, found_at, blocked }: ModelStatus): boolean =>
  role === "diarize" && found_at !== null && blocked === null;

/** What this computer lacks: the recommended models, and the smallest speaker model when none would be usable. */
export const missingDownloads = ({ models }: Snapshot): ModelStatus[] => {
  const recommended: ModelStatus[] = models.filter(
    (model: ModelStatus): boolean => model.recommended && canDownload(model),
  );
  const hasDiarizer: boolean =
    models.some(isReadyDiarizer) || recommended.some(({ role }: ModelStatus): boolean => role === "diarize");
  const smallest: ModelStatus | undefined = models
    .filter((model: ModelStatus): boolean => model.role === "diarize" && canDownload(model))
    .toSorted(bySize)[0];
  return hasDiarizer || smallest === undefined ? recommended : [...recommended, smallest];
};

function startDownloads(store: AppStore, models: ModelStatus[]): void {
  models.forEach(({ id, download_bytes: bytes }: ModelStatus): void => {
    store.setState((state: AppState): AppState => startedDownload(state, id, bytes));
    startDownload(id).catch((error: unknown): void => reportFailure(store.setState, error));
  });
}

function SelectButton({
  model,
  store,
  snapshot,
}: {
  model: ModelStatus;
  store: AppStore;
  snapshot: Snapshot;
}): VNode | null {
  const messages: Messages = useMessages();
  if (model.role !== "diarize" || model.found_at === null || model.blocked !== null) {
    return null;
  }
  const { settings } = snapshot;
  const selected: boolean = settings.diarize_model === model.id;
  return (
    <Button
      variant={selected ? "default" : "outline"}
      aria-pressed={selected}
      onClick={(): void => store.updateSettings({ ...settings, diarize_model: model.id })}
    >
      {format(selected ? messages.inUseModel : messages.useModel, { name: model.label })}
    </Button>
  );
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
      <Button
        onClick={(): void => {
          store.setState(
            (state: AppState): AppState => startedDownload(state, model.id, model.download_bytes),
          );
          startDownload(model.id).catch(fail);
        }}
      >
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

function ModelRow({ model, store, snapshot, download }: ModelRowProps): VNode {
  const messages: Messages = useMessages();
  return (
    <li className="grid gap-2 border-t border-border pt-3 first:border-t-0 first:pt-0">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="font-medium">{model.label}</h3>
        {model.recommended ? <Badge variant="default">{messages.badgeRecommended}</Badge> : null}
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
        <SelectButton model={model} store={store} snapshot={snapshot} />
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
