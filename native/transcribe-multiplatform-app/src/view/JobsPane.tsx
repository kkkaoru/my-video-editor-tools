// Runs with bun: drop target, readiness note and the job list.
import { CheckIcon, CircleIcon, LoaderIcon, UploadIcon } from "lucide-preact";
import type { VNode } from "preact";
import { Alert } from "@/components/ui/alert.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card.tsx";
import { Progress } from "@/components/ui/progress.tsx";
import { cancelJob, pickFiles, relayoutTranscript } from "../backend/backend.ts";
import type { Profile, Role, Snapshot, Stage } from "../backend/types.ts";
import { languageFor } from "../i18n/language.ts";
import { errorText, format, type Messages, roleName } from "../i18n/messages.ts";
import { useMessages } from "../i18n/use-messages.ts";
import {
  type FinishedJobRow,
  type JobRow,
  missingModelRoles,
  type RunningJobRow,
} from "../state/app-state.ts";
import {
  type AppStore,
  reportFailure,
  SUBTITLE_EXTENSIONS,
  VIDEO_EXTENSIONS,
} from "../state/use-app-store.ts";
import { SelectField, type SelectOption } from "./fields.tsx";
import { HelpTip } from "./HelpTip.tsx";

export type StepPosition = "done" | "current" | "pending";

export interface JobsPaneProps {
  store: AppStore;
  snapshot: Snapshot;
}

interface JobItemProps {
  job: JobRow;
  store: AppStore;
  snapshot: Snapshot;
}

interface StepRowProps {
  stage: Stage;
  position: StepPosition;
  messages: Messages;
}

const SECONDS_PER_MINUTE: number = 60;
const MAX_SPEAKER_COUNT: number = 8;
const PERCENT: number = 100;
const FOLDERS: RegExp = /^.*[\\/]/s;
const BEFORE_EXTENSION: RegExp = /^.*\./s;
const STEP_ICONS: Record<StepPosition, typeof CheckIcon> = {
  done: CheckIcon,
  current: LoaderIcon,
  pending: CircleIcon,
};

export const fileName = (path: string): string => path.replace(FOLDERS, "");

const stepPosition = (index: number, currentIndex: number): StepPosition => {
  if (index === currentIndex) {
    return "current";
  }
  return index < currentIndex ? "done" : "pending";
};

/** Rounded up to whole minutes above one minute: a second-by-second countdown would only look precise. */
export const remainingText = (seconds: number | null, messages: Messages): string => {
  if (seconds === null) {
    return messages.jobEstimating;
  }
  return seconds < SECONDS_PER_MINUTE
    ? messages.jobRemainingUnderMinute
    : format(messages.jobRemaining, { minutes: Math.ceil(seconds / SECONDS_PER_MINUTE) });
};

export const outputPurpose = (path: string, messages: Messages): string => {
  const extension: string = path.replace(BEFORE_EXTENSION, "");
  return messages.outputPurposes[extension] ?? extension;
};

/** Only when the work will be slower than this computer could manage: there is nothing to say otherwise. */
export const slowProcessingNotice = ({ backend, hardware }: Snapshot, messages: Messages): string | null => {
  if (backend === "speakers_on_cpu") {
    return messages.noticeSpeakersOnCpu;
  }
  return !hardware.is_mac && hardware.gpu === null ? messages.noticeCpuOnly : null;
};

export const speakerCountOptions = (messages: Messages): SelectOption[] => [
  { value: "0", label: messages.speakerCountAuto, disabled: false },
  ...Array.from(
    { length: MAX_SPEAKER_COUNT },
    (_, index: number): SelectOption => ({
      value: String(index + 1),
      label: format(messages.speakerCountValue, { count: index + 1 }),
      disabled: false,
    }),
  ),
];

export const missingRoles = (snapshot: Snapshot, messages: Messages): string[] =>
  missingModelRoles(snapshot).map((role: Role): string => roleName(messages, role));

/** Says which look dropped subtitle files get; a selection left over from a deleted profile does not count. */
export const subtitleTargetText = (snapshot: Snapshot, messages: Messages): string =>
  snapshot.profiles.some(({ name }: Profile): boolean => name === snapshot.settings.active_profile)
    ? format(messages.dropProfileHint, { name: snapshot.settings.active_profile })
    : messages.dropCurrentHint;

export const modelLabel = (snapshot: Snapshot, id: string | undefined): string =>
  snapshot.models.find((model): boolean => model.id === id)?.label ?? id ?? "";

/** Each step says its state in words and with an icon, never by colour alone (WCAG 1.4.1). */
function StepRow({ stage, position, messages }: StepRowProps): VNode {
  const Icon = STEP_ICONS[position];
  return (
    <li className={`flex items-center gap-2 ${position === "pending" ? "text-muted-foreground" : ""}`}>
      <Icon className="size-4 shrink-0" aria-hidden="true" />
      <span className={position === "current" ? "font-medium" : ""}>{messages.stages[stage]}</span>
      <span className="sr-only">{messages.stepStates[position]}</span>
    </li>
  );
}

function RunningJob({ job, name }: { job: RunningJobRow; name: string }): VNode {
  const messages: Messages = useMessages();
  const {
    stage,
    overall,
    determinate,
    plan,
    remaining_seconds: remaining,
    stage_fraction: fraction,
  } = job.progress;
  const currentIndex: number = plan.indexOf(stage);
  const detail: string = determinate ? `${Math.round(fraction * PERCENT)}%` : messages.jobNoPercent;
  return (
    <div className="grid grid-cols-1 gap-3">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        {/* Only the stage is live: announcing every percent and second would interrupt (WCAG 2.2.4). */}
        <p role="status" className="font-medium">
          {format(messages.jobStep, {
            number: currentIndex + 1,
            total: plan.length,
            stage: messages.stages[stage],
          })}
        </p>
        <p className="text-muted-foreground tabular-nums">{detail}</p>
      </div>
      <Progress
        value={overall}
        min={0}
        max={1}
        aria-label={format(messages.jobProgressLabel, { name })}
        getAriaValueText={(): string => `${Math.round(overall * PERCENT)}%`}
      />
      <p className="text-muted-foreground tabular-nums">{remainingText(remaining, messages)}</p>
      <ol className="grid gap-1 sm:grid-cols-2">
        {plan.map(
          (planned: Stage, index: number): VNode => (
            <StepRow
              key={planned}
              stage={planned}
              position={stepPosition(index, currentIndex)}
              messages={messages}
            />
          ),
        )}
      </ol>
    </div>
  );
}

const FinishedJob = ({ job }: { job: FinishedJobRow }): VNode => {
  const messages: Messages = useMessages();
  return (
    <div className="grid grid-cols-1 gap-2">
      <p role="status" className="font-medium text-success">
        {format(messages.jobDone, { cues: job.outcome.cue_count, speakers: job.outcome.speaker_count })}
      </p>
      {job.outcome.requested_speakers > job.outcome.speaker_count ? (
        <p className="text-muted-foreground">
          {format(messages.jobFewerSpeakers, {
            requested: job.outcome.requested_speakers,
            found: job.outcome.speaker_count,
          })}
        </p>
      ) : null}
      {job.outcome.faceless ? <p className="text-muted-foreground">{messages.jobFaceless}</p> : null}
      <div>
        <h3 className="font-medium">{messages.jobOutputs}</h3>
        <ul className="break-all text-muted-foreground">
          {job.outcome.outputs.map(
            (path: string): VNode => (
              <li key={path} className="max-w-[min(65ch,40em)]">
                <span className="font-medium text-foreground">{outputPurpose(path, messages)}</span>: {path}
              </li>
            ),
          )}
        </ul>
      </div>
    </div>
  );
};

function JobActions({ job, name, store }: { job: JobRow; name: string; store: AppStore }): VNode | null {
  const messages: Messages = useMessages();
  const fail = (error: unknown): void => reportFailure(store.setState, error);
  if (job.state === "queued" || job.state === "running") {
    return (
      <Button variant="outline" onClick={(): void => void cancelJob(job.id).catch(fail)}>
        {format(messages.jobCancel, { name })}
      </Button>
    );
  }
  return job.state === "finished" ? (
    <Button
      variant="outline"
      onClick={(): void => void relayoutTranscript(`${job.outcome.base}.transcript.json`).catch(fail)}
    >
      {format(messages.jobRelayout, { name })}
    </Button>
  ) : null;
}

function JobItem({ job, store, snapshot }: JobItemProps): VNode {
  const messages: Messages = useMessages();
  const name: string = fileName(job.path);
  return (
    <li>
      <Card>
        <CardHeader className="flex flex-wrap items-center justify-between gap-2">
          <CardTitle className="min-w-0 break-all">{name}</CardTitle>
          <JobActions job={job} name={name} store={store} />
        </CardHeader>
        <CardContent className="grid grid-cols-1 gap-2">
          {job.state === "queued" ? (
            <p role="status" className="text-muted-foreground">
              {messages.jobQueued}
            </p>
          ) : null}
          {job.state === "running" ? <RunningJob job={job} name={name} /> : null}
          {job.state === "cancelled" ? (
            <p role="status" className="text-muted-foreground">
              {messages.jobCancelled}
            </p>
          ) : null}
          {job.state === "finished" ? <FinishedJob job={job} /> : null}
          {job.state === "failed" ? (
            <Alert>{errorText(messages, job.error, modelLabel(snapshot, job.error.model))}</Alert>
          ) : null}
        </CardContent>
      </Card>
    </li>
  );
}

function chooseSubtitles(store: AppStore, messages: Messages): void {
  pickFiles(SUBTITLE_EXTENSIONS, messages.subtitleFilterName)
    .then((picked: string[] | null): void => {
      if (picked !== null) {
        store.dropFiles(picked);
      }
    })
    .catch((error: unknown): void => reportFailure(store.setState, error));
}

function chooseVideos(store: AppStore, messages: Messages): void {
  const missing: Role[] = missingModelRoles(store.state.snapshot);
  if (missing.length > 0) {
    reportFailure(store.setState, { code: "models_missing", roles: missing });
    return;
  }
  pickFiles(VIDEO_EXTENSIONS, messages.videoFilterName)
    .then((picked: string[] | null): void => {
      if (picked !== null) {
        store.dropFiles(picked);
      }
    })
    .catch((error: unknown): void => reportFailure(store.setState, error));
}

export function JobsPane({ store, snapshot }: JobsPaneProps): VNode {
  const messages: Messages = useMessages();
  const missing: string[] = missingRoles(snapshot, messages);
  const { hovering, jobs } = store.state;
  const notice: string | null = slowProcessingNotice(snapshot, messages);
  return (
    <div className="grid grid-cols-1 gap-4">
      {notice === null ? null : <p className="text-muted-foreground">{notice}</p>}
      <div className="max-w-sm">
        <SelectField
          label={messages.speakerCount}
          value={String(snapshot.settings.speaker_count)}
          options={speakerCountOptions(messages)}
          help={<HelpTip label={messages.speakerCountHelpLabel}>{messages.speakerCountHelp}</HelpTip>}
          onChange={(value: string): void =>
            store.updateSettings({ ...snapshot.settings, speaker_count: Number(value) })
          }
        />
      </div>
      <section
        aria-label={messages.dropTitle}
        className={`flex flex-col items-center gap-3 rounded-xl border-2 border-dashed p-10 text-center transition-colors ${hovering ? "border-primary bg-muted" : "border-border bg-card"}`}
      >
        <UploadIcon className="size-8 text-muted-foreground" aria-hidden="true" />
        <div className="grid justify-items-center gap-1">
          <h2 className="text-lg font-semibold">{hovering ? messages.dropHover : messages.dropTitle}</h2>
          <p className="text-muted-foreground">{messages.dropHint}</p>
          <p className="text-muted-foreground">{subtitleTargetText(snapshot, messages)}</p>
        </div>
        {/* Buttons, because dragging must not be the only way (WCAG 2.5.7). */}
        <div className="flex flex-wrap justify-center gap-2">
          <Button onClick={(): void => chooseVideos(store, messages)}>{messages.pickVideos}</Button>
          <Button variant="outline" onClick={(): void => chooseSubtitles(store, messages)}>
            {messages.pickSubtitles}
          </Button>
        </div>
      </section>
      {missing.length === 0 ? null : (
        <Alert>
          {format(messages.readinessMissing, {
            roles: new Intl.ListFormat(languageFor(snapshot.locale).tag, { type: "conjunction" }).format(
              missing,
            ),
          })}
        </Alert>
      )}
      {jobs.length === 0 ? null : (
        <ul className="grid grid-cols-1 gap-3">
          {jobs.map(
            (job: JobRow): VNode => (
              <JobItem key={job.id} job={job} store={store} snapshot={snapshot} />
            ),
          )}
        </ul>
      )}
    </div>
  );
}
