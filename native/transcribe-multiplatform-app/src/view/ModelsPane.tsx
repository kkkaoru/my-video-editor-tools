// Runs with bun: download, reuse and remove speech and face models.
import type { VNode } from "preact";
import { cancelDownload, deleteModel, pickFolder, saveSettings, startDownload } from "../backend/backend.ts";
import type { ModelStatus, Role, Settings, Snapshot } from "../backend/types.ts";
import { errorText, format, type Messages, type ModelNotes } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import { type AppState, type DownloadRow, startedDownload } from "../state/app-state.ts";
import { type AppStore, reportFailure } from "../state/use-app-store.ts";
import { ActionButton, ProgressBar } from "./controls.tsx";

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
const SELECTION_KEYS: ReadonlyMap<Role, "transcribe_model" | "diarize_model"> = new Map([
  ["transcribe", "transcribe_model"],
  ["diarize", "diarize_model"],
]);

export const formatBytes = (bytes: number): string =>
  bytes >= GIB ? `${(bytes / GIB).toFixed(1)} GB` : `${Math.ceil(bytes / MIB)} MB`;

const isOwnCopy = (model: ModelStatus, snapshot: Snapshot): boolean =>
  model.found_at?.startsWith(snapshot.models_dir) === true;

const hasNote = (notes: ModelNotes, id: string): id is keyof ModelNotes => id in notes;

const noteFor = (notes: ModelNotes, id: string): string => (hasNote(notes, id) ? notes[id] : "");

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
  const key: "transcribe_model" | "diarize_model" | undefined = SELECTION_KEYS.get(model.role);
  if (key === undefined || model.found_at === null || model.blocked !== null) {
    return null;
  }
  const { settings } = snapshot;
  const selected: boolean = settings[key] === model.id;
  return (
    <ActionButton
      pressed={selected}
      onClick={(): void => store.updateSettings({ ...settings, [key]: model.id })}
    >
      {format(selected ? messages.inUseModel : messages.useModel, { name: model.label })}
    </ActionButton>
  );
}

function DownloadButtons({ model, store, snapshot, download }: ModelRowProps): VNode | null {
  const messages: Messages = useMessages();
  const name: string = model.label;
  const fail = (error: unknown): void => reportFailure(store.setState, error);
  if (download?.running === true) {
    return (
      <ActionButton onClick={(): void => void cancelDownload(model.id).catch(fail)}>
        {format(messages.downloadCancel, { name })}
      </ActionButton>
    );
  }
  if (model.download_bytes > 0 && model.found_at === null && model.blocked === null) {
    return (
      <ActionButton
        primary
        onClick={(): void => {
          store.setState(
            (state: AppState): AppState => startedDownload(state, model.id, model.download_bytes),
          );
          startDownload(model.id).catch(fail);
        }}
      >
        {format(messages.download, { name, size: formatBytes(model.download_bytes) })}
      </ActionButton>
    );
  }
  return isOwnCopy(model, snapshot) ? (
    <ActionButton onClick={(): void => void deleteModel(model.id).then(store.applySnapshot).catch(fail)}>
      {format(messages.deleteModel, { name })}
    </ActionButton>
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
    return (
      <p role="alert" className="break-all text-error">
        {errorText(messages, download.error, model.label)}
      </p>
    );
  }
  const text: string = format(messages.downloadProgress, {
    done: formatBytes(download.doneBytes),
    total: formatBytes(download.totalBytes),
  });
  return (
    <div>
      <p className="text-sm text-muted">{text}</p>
      <ProgressBar
        label={format(messages.downloadProgressLabel, { name: model.label })}
        fraction={download.totalBytes > 0 ? download.doneBytes / download.totalBytes : null}
        valueText={text}
      />
    </div>
  );
}

function ModelRow({ model, store, snapshot, download }: ModelRowProps): VNode {
  const messages: Messages = useMessages();
  return (
    <li className="rounded-lg border border-line bg-panel p-3">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="font-semibold">{model.label}</h3>
        {model.recommended ? (
          <span className="rounded border border-accent px-2 text-sm text-ink">
            {messages.badgeRecommended}
          </span>
        ) : null}
        {model.found_at === null ? null : (
          <span className="rounded border border-ok px-2 text-sm text-ok">{messages.badgePresent}</span>
        )}
      </div>
      <p className="text-sm text-muted">{noteFor(messages.modelNotes, model.id)}</p>
      {model.blocked === null ? null : (
        <p className="text-sm text-error">{errorText(messages, model.blocked, model.label)}</p>
      )}
      {model.found_at === null ? null : (
        <p className="text-sm break-all text-muted">
          {format(messages.locationLabel, { path: model.found_at })}
        </p>
      )}
      {model.found_at === null && model.source_url !== "" ? (
        <p className="text-sm break-all text-muted">
          {format(messages.sourceLabel, { url: model.source_url })}
        </p>
      ) : null}
      <div className="mt-2 flex flex-wrap gap-2">
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

const ROLE_ORDER: Role[] = ["transcribe", "diarize", "face", "runtime"];

const roleHeading = (messages: Messages, role: Role): string =>
  (
    ({
      transcribe: messages.roleTranscribe,
      diarize: messages.roleDiarize,
      face: messages.roleFace,
      runtime: messages.roleRuntime,
    }) satisfies Record<Role, string>
  )[role];

export function ModelsPane({ store, snapshot }: ModelsPaneProps): VNode {
  const messages: Messages = useMessages();
  const visible: ModelStatus[] = snapshot.models.filter(({ hidden }: ModelStatus): boolean => !hidden);
  return (
    <>
      <h2 className="font-semibold">{messages.modelsHeading}</h2>
      <p className="text-sm text-muted">{messages.modelsHint}</p>
      <div className="my-2">
        <ActionButton
          onClick={(): void =>
            void addFolder(store, snapshot.settings).catch((error: unknown): void =>
              reportFailure(store.setState, error),
            )
          }
        >
          {messages.addModelFolder}
        </ActionButton>
      </div>
      {snapshot.settings.model_dirs.length === 0 ? null : (
        <p className="text-sm break-all text-muted">
          {messages.modelFolders}: {snapshot.settings.model_dirs.join(" / ")}
        </p>
      )}
      {ROLE_ORDER.filter((role: Role): boolean =>
        visible.some((model: ModelStatus): boolean => model.role === role),
      ).map(
        (role: Role): VNode => (
          <section key={role} aria-label={roleHeading(messages, role)} className="mt-4">
            <h2 className="font-semibold">{roleHeading(messages, role)}</h2>
            <ul className="mt-1 grid gap-2">
              {visible
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
          </section>
        ),
      )}
    </>
  );
}
