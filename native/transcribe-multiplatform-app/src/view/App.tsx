// Runs with bun: the window: language selection, tabs and the failure banner.
import { Tabs } from "@base-ui/react/tabs";
import type { VNode } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { Hardware, ModelStatus, Snapshot } from "../backend/types.ts";
import { type Language, languageFor } from "../i18n/language.ts";
import { errorText, format, type Messages } from "../i18n/messages.ts";
import { MessagesContext } from "../i18n/use-messages.ts";
import { useAppStore } from "../state/use-app-store.ts";
import type { SelectOption } from "./controls.tsx";
import { JobsPane } from "./JobsPane.tsx";
import { ModelsPane } from "./ModelsPane.tsx";
import { StylePane } from "./StylePane.tsx";

const CLOCK_INTERVAL_MS: number = 1000;
const TAB_CLASS: string =
  "min-h-11 min-w-11 rounded-md border border-transparent px-3 py-2 text-ink hover:border-line aria-selected:border-accent aria-selected:font-semibold aria-selected:underline";

const integratedGpuLabel = (isMac: boolean, messages: Messages): string =>
  isMac ? messages.hardwareMetal : messages.hardwareCpu;

export const hardwareLabel = (
  { nvidia_name: name, nvidia_memory_gib: vram, is_mac: isMac, memory_gib: memory }: Hardware,
  messages: Messages,
): string =>
  `${name === null ? integratedGpuLabel(isMac, messages) : `${name} (${vram} GB)`} / ${format(messages.hardwareMemory, { gib: memory })}`;

const faceOptions = (models: ModelStatus[]): SelectOption[] =>
  models
    .filter(({ role }: ModelStatus): boolean => role === "face")
    .map(
      ({ id, label, found_at }: ModelStatus): SelectOption => ({
        value: id,
        label,
        disabled: found_at === null,
      }),
    );

function Loaded({ snapshot, language }: { snapshot: Snapshot; language: Language }): VNode {
  const store = useAppStore(snapshot);
  const [now, setNow] = useState<number>(Date.now());
  useEffect((): (() => void) => {
    const timer = setInterval((): void => setNow(Date.now()), CLOCK_INTERVAL_MS);
    return (): void => clearInterval(timer);
  }, []);
  const { messages } = language;
  const live: Snapshot = store.state.snapshot;
  return (
    <MessagesContext.Provider value={messages}>
      <Tabs.Root defaultValue="jobs" className="flex min-h-screen flex-col">
        <header className="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-4 py-2">
          <h1 className="font-semibold">{messages.appName}</h1>
          <Tabs.List className="flex flex-wrap gap-1">
            <Tabs.Tab value="jobs" className={TAB_CLASS}>
              {messages.tabJobs}
            </Tabs.Tab>
            <Tabs.Tab value="style" className={TAB_CLASS}>
              {messages.tabStyle}
            </Tabs.Tab>
            <Tabs.Tab value="models" className={TAB_CLASS}>
              {messages.tabModels}
            </Tabs.Tab>
          </Tabs.List>
          <p className="ml-auto text-sm text-muted">{hardwareLabel(live.hardware, messages)}</p>
        </header>
        <main className="flex-1 p-4">
          {store.state.loadError === null ? null : (
            <p role="alert" className="mb-3 break-all text-error">
              {format(messages.loadFailed, { detail: errorText(messages, store.state.loadError, "") })}
            </p>
          )}
          <Tabs.Panel value="jobs">
            <JobsPane store={store} snapshot={live} now={now} />
          </Tabs.Panel>
          <Tabs.Panel value="style">
            <StylePane store={store} settings={live.settings} faceOptions={faceOptions(live.models)} />
          </Tabs.Panel>
          <Tabs.Panel value="models">
            <ModelsPane store={store} snapshot={live} />
          </Tabs.Panel>
        </main>
      </Tabs.Root>
    </MessagesContext.Provider>
  );
}

export interface AppProps {
  initial: Snapshot;
}

/** Sets the document language before the first paint so assistive tech reads the right voice. */
export function App({ initial }: AppProps): VNode {
  const language: Language = languageFor(initial.locale);
  document.documentElement.lang = language.tag;
  document.title = language.messages.appName;
  return <Loaded snapshot={initial} language={language} />;
}
