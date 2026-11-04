// Runs with bun: the window: language selection, tabs and the failure banner.
import type { VNode } from "preact";
import { Alert } from "@/components/ui/alert.tsx";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs.tsx";
import type { ModelStatus, Snapshot } from "../backend/types.ts";
import { type Language, languageFor } from "../i18n/language.ts";
import { errorText, format } from "../i18n/messages.ts";
import { MessagesContext } from "../i18n/use-messages.ts";
import { useAppStore } from "../state/use-app-store.ts";
import type { SelectOption } from "./fields.tsx";
import { HelpPane } from "./HelpPane.tsx";
import { JobsPane } from "./JobsPane.tsx";
import { ModelsPane } from "./ModelsPane.tsx";
import { StylePane } from "./StylePane.tsx";

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
  const { messages } = language;
  const live: Snapshot = store.state.snapshot;
  return (
    <MessagesContext.Provider value={messages}>
      <Tabs defaultValue="jobs" className="mx-auto min-h-screen w-full max-w-5xl p-4">
        <header className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <h1 className="sr-only">{messages.appName}</h1>
          <TabsList className="w-full">
            <TabsTrigger value="jobs">{messages.tabJobs}</TabsTrigger>
            <TabsTrigger value="style">{messages.tabStyle}</TabsTrigger>
            <TabsTrigger value="models">{messages.tabModels}</TabsTrigger>
            <TabsTrigger value="help">{messages.tabHelp}</TabsTrigger>
          </TabsList>
        </header>
        <main className="grid flex-1 gap-4">
          {store.state.loadError === null ? null : (
            <Alert>
              {format(messages.loadFailed, { detail: errorText(messages, store.state.loadError, "") })}
            </Alert>
          )}
          <TabsContent value="jobs" keepMounted>
            <JobsPane store={store} snapshot={live} />
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
            <HelpPane version={live.version} />
          </TabsContent>
        </main>
      </Tabs>
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
