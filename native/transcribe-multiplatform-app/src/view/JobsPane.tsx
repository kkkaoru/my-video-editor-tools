// Runs with bun: drop target, readiness note and the job list.
import type { VNode } from "preact";
import { cancelJob, pickFiles, relayoutTranscript } from "../backend/backend.ts";
import type { Snapshot } from "../backend/types.ts";
import { languageFor } from "../i18n/language.ts";
import { errorText, format, type Messages } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import type { JobRow } from "../state/app-state.ts";
import { type AppStore, reportFailure, VIDEO_EXTENSIONS } from "../state/use-app-store.ts";
import { ActionButton, ProgressBar } from "./controls.tsx";

export interface JobsPaneProps {
  store: AppStore;
  snapshot: Snapshot;
  now: number;
}

interface JobItemProps {
  job: JobRow;
  store: AppStore;
  snapshot: Snapshot;
  now: number;
}

const MS_PER_SECOND: number = 1000;
const SECONDS_PER_MINUTE: number = 60;
const PERCENT: number = 100;
const FOLDERS: RegExp = /^.*[\\/]/s;

export const fileName = (path: string): string => path.replace(FOLDERS, "");

export const elapsedParts = (ms: number): { minutes: number; seconds: number } => ({
  minutes: Math.floor(ms / MS_PER_SECOND / SECONDS_PER_MINUTE),
  seconds: Math.floor(ms / MS_PER_SECOND) % SECONDS_PER_MINUTE,
});

export const missingRoles = (snapshot: Snapshot, messages: Messages): string[] =>
  [
    { role: "transcribe", text: messages.roleTranscribe },
    { role: "diarize", text: messages.roleDiarize },
  ]
    .filter(
      ({ role }): boolean =>
        !snapshot.models.some(
          (model): boolean => model.role === role && model.found_at !== null && model.blocked === null,
        ),
    )
    .map(({ text }): string => text);

export const modelLabel = (snapshot: Snapshot, id: string | undefined): string =>
  snapshot.models.find((model): boolean => model.id === id)?.label ?? id ?? "";

function RunningJob({ job, name, now }: { job: JobRow; name: string; now: number }): VNode {
  const messages: Messages = useMessages();
  if (job.progress === null) {
    return <p className="text-sm text-muted">{messages.stages.prepare}</p>;
  }
  const { stage, overall, determinate } = job.progress;
  const detail: string = determinate ? `${Math.round(overall * PERCENT)}%` : messages.jobNoPercent;
  return (
    <>
      {/* Only the stage is live: announcing every percent and second would interrupt (WCAG 2.2.4). */}
      <p role="status" className="text-sm text-muted">
        {messages.stages[stage]}
      </p>
      <p className="text-sm text-muted">
        {detail} / {format(messages.jobElapsed, elapsedParts(now - job.startedAt))}
      </p>
      <ProgressBar
        label={format(messages.jobProgressLabel, { name })}
        fraction={determinate ? overall : null}
        valueText={`${messages.stages[stage]}: ${detail}`}
      />
    </>
  );
}

const FinishedJob = ({ job }: { job: JobRow }): VNode | null => {
  const messages: Messages = useMessages();
  return job.outcome === null ? null : (
    <>
      <p role="status" className="text-ok">
        {format(messages.jobDone, { cues: job.outcome.cue_count, speakers: job.outcome.speaker_count })}
      </p>
      {job.outcome.faceless ? <p className="text-sm text-muted">{messages.jobFaceless}</p> : null}
      <h3 className="mt-2 text-sm font-semibold">{messages.jobOutputs}</h3>
      <ul className="text-sm break-all text-muted">
        {job.outcome.outputs.map(
          (path: string): VNode => (
            <li key={path}>{path}</li>
          ),
        )}
      </ul>
    </>
  );
};

function JobActions({ job, name, store }: { job: JobRow; name: string; store: AppStore }): VNode | null {
  const messages: Messages = useMessages();
  if (job.state === "queued" || job.state === "running") {
    return (
      <ActionButton
        onClick={(): void =>
          void cancelJob(job.id).catch((error: unknown): void => reportFailure(store.setState, error))
        }
      >
        {format(messages.jobCancel, { name })}
      </ActionButton>
    );
  }
  const base: string | undefined = job.outcome?.base;
  return base === undefined ? null : (
    <ActionButton
      onClick={(): void =>
        void relayoutTranscript(`${base}.transcript.json`).catch((error: unknown): void =>
          reportFailure(store.setState, error),
        )
      }
    >
      {format(messages.jobRelayout, { name })}
    </ActionButton>
  );
}

function JobItem({ job, store, snapshot, now }: JobItemProps): VNode {
  const messages: Messages = useMessages();
  const name: string = fileName(job.path);
  return (
    <li className="rounded-lg border border-line bg-panel p-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="font-semibold break-all">{name}</h2>
        <JobActions job={job} name={name} store={store} />
      </div>
      <div>
        {job.state === "queued" ? (
          <p role="status" className="text-sm text-muted">
            {messages.jobQueued}
          </p>
        ) : null}
        {job.state === "running" ? <RunningJob job={job} name={name} now={now} /> : null}
        {job.state === "cancelled" ? (
          <p role="status" className="text-sm text-muted">
            {messages.jobCancelled}
          </p>
        ) : null}
        {job.state === "finished" ? <FinishedJob job={job} /> : null}
      </div>
      {job.error === null ? null : (
        <p role="alert" className="whitespace-pre-wrap break-all text-error">
          {errorText(messages, job.error, modelLabel(snapshot, job.error.model))}
        </p>
      )}
    </li>
  );
}

function chooseVideos(store: AppStore, messages: Messages): void {
  pickFiles(VIDEO_EXTENSIONS, messages.videoFilterName)
    .then((picked: string[] | null): void => {
      if (picked !== null) {
        store.dropVideos(picked);
      }
    })
    .catch((error: unknown): void => reportFailure(store.setState, error));
}

export function JobsPane({ store, snapshot, now }: JobsPaneProps): VNode {
  const messages: Messages = useMessages();
  const missing: string[] = missingRoles(snapshot, messages);
  const { hovering, jobs } = store.state;
  return (
    <>
      <section
        aria-label={messages.dropTitle}
        className={`flex flex-col items-center gap-2 rounded-lg border-2 border-dashed p-8 text-center ${hovering ? "border-accent bg-canvas" : "border-line bg-panel"}`}
      >
        <h2 className="text-lg font-semibold">{hovering ? messages.dropHover : messages.dropTitle}</h2>
        <p className="text-muted">{messages.dropHint}</p>
        <ActionButton primary onClick={(): void => chooseVideos(store, messages)}>
          {messages.pickVideos}
        </ActionButton>
      </section>
      {missing.length === 0 ? null : (
        <p role="alert" className="mt-3 text-error">
          {format(messages.readinessMissing, {
            roles: new Intl.ListFormat(languageFor(snapshot.locale).tag, { type: "conjunction" }).format(
              missing,
            ),
          })}
        </p>
      )}
      <ul className="mt-4 grid gap-2">
        {jobs.map(
          (job: JobRow): VNode => (
            <JobItem key={job.id} job={job} store={store} snapshot={snapshot} now={now} />
          ),
        )}
      </ul>
    </>
  );
}
