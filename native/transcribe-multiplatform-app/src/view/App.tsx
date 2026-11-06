// Runs with bun: the window: language selection, tabs and the failure banner.
import type { VNode } from "preact";
import { useState } from "preact/hooks";
import { Alert } from "@/components/ui/alert.tsx";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs.tsx";
import type { ModelStatus, Snapshot, UiTheme } from "../backend/types.ts";
import { type Language, languageFor } from "../i18n/language.ts";
import { errorText, format } from "../i18n/messages.ts";
import { MessagesContext } from "../i18n/use-messages.ts";
import { isWorking, uiLocale } from "../state/app-state.ts";
import { useAppStore } from "../state/use-app-store.ts";
import { type Updater, useUpdater } from "../state/use-updater.ts";
import type { SelectOption } from "./fields.tsx";
import { HelpPane } from "./HelpPane.tsx";
import { JobsPane } from "./JobsPane.tsx";
import { ModelsPane } from "./ModelsPane.tsx";
import { SettingsPane } from "./SettingsPane.tsx";
import { StylePane } from "./StylePane.tsx";
import { UpdateOffer } from "./UpdatePanels.tsx";

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

/** Before the paint, so assistive tech reads the right voice and nothing flashes in the other theme. */
function applyToDocument({ tag, messages }: Language, theme: UiTheme): void {
  document.documentElement.lang = tag;
  document.title = messages.appName;
  if (theme === "system") {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = theme;
  }
}

function Loaded({ snapshot }: { snapshot: Snapshot }): VNode {
  const store = useAppStore(snapshot);
  const updater: Updater = useUpdater();
  const [tab, setTab] = useState<string>("jobs");
  const openModels = (): void => {
    setTab("models");
    // The button pressed goes away with its tab; the keyboard stays with what replaced it.
    document.querySelector<HTMLElement>('[data-tab="models"]')?.focus();
  };
  const live: Snapshot = store.state.snapshot;
  const language: Language = languageFor(uiLocale(live));
  const { messages } = language;
  applyToDocument(language, live.settings.ui_theme);
  return (
    <MessagesContext.Provider value={messages}>
      <Tabs
        value={tab}
        onValueChange={(next: string): void => setTab(next)}
        className="mx-auto min-h-screen w-full max-w-5xl p-4"
      >
        <header className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <h1 className="sr-only">{messages.appName}</h1>
          <TabsList className="w-full">
            <TabsTrigger value="jobs">{messages.tabJobs}</TabsTrigger>
            <TabsTrigger value="style">{messages.tabStyle}</TabsTrigger>
            <TabsTrigger value="models" data-tab="models">
              {messages.tabModels}
            </TabsTrigger>
            <TabsTrigger value="help">{messages.tabHelp}</TabsTrigger>
            <TabsTrigger value="settings">{messages.tabSettings}</TabsTrigger>
          </TabsList>
        </header>
        <main className="grid flex-1 content-start gap-4">
          <UpdateOffer updater={updater} busy={isWorking(store.state)} />
          {store.state.loadError === null ? null : (
            <Alert>
              {format(messages.loadFailed, { detail: errorText(messages, store.state.loadError, "") })}
            </Alert>
          )}
          <TabsContent value="jobs" keepMounted>
            <JobsPane store={store} snapshot={live} onOpenModels={openModels} />
          </TabsContent>
          <TabsContent value="style" keepMounted>
            <StylePane
              store={store}
              settings={live.settings}
              profiles={live.profiles}
              faceOptions={faceOptions(live.models)}
            />
          </TabsContent>
          <TabsContent value="models" keepMounted>
            <ModelsPane store={store} snapshot={live} />
          </TabsContent>
          <TabsContent value="help" keepMounted>
            <HelpPane />
          </TabsContent>
          <TabsContent value="settings" keepMounted>
            <SettingsPane
              version={live.version}
              settings={live.settings}
              onChange={store.updateSettings}
              updater={updater}
            />
          </TabsContent>
        </main>
      </Tabs>
    </MessagesContext.Provider>
  );
}

export interface AppProps {
  initial: Snapshot;
}

export function App({ initial }: AppProps): VNode {
  return <Loaded snapshot={initial} />;
}
