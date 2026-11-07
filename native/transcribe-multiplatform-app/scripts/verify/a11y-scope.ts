// Runs with bun: decides which WCAG sweep tests a list of changed files can affect.

export interface PartialSweep {
  kind: "some";
  grep: string;
  /** Null when no screen changed: Web Vitals load one language, so a translation does not move them. */
  vitalsGrep: string | null;
}

export type SweepScope = { kind: "none" } | PartialSweep;

interface ScreenRule {
  file: RegExp;
  titles: string[];
  vitals: string[];
}

interface FileImpact {
  titles: string[];
  vitals: string[];
  locales: string[];
  shared: boolean;
}

const APP_PREFIX: RegExp = /^native\/transcribe-multiplatform-app\//;
const TEST_FILE: RegExp = /\.test\.tsx?$/;
const LOCALE_FILE: RegExp = /^src\/i18n\/locale-([a-z-]+)\.ts$/;
// Anything else that can reach the rendered page; an unlisted file here runs the shared sample below.
const UI_FILE: RegExp =
  /^(src\/|public\/|tests\/a11y\/|index\.html$|package\.json$|bun\.lock$|vite\.config\.ts$)/;
const JOBS: string = "jobs? ";
const STYLE: string = "style |speaker names";
const MODELS: string = "models? ";
const HELP: string = "help ";
const JOBS_VITALS: string = "jobs? ";
const STYLE_VITALS: string = "style ";
const MODELS_VITALS: string = "models ";
const HELP_VITALS: string = "help";
const SCREEN_RULES: ScreenRule[] = [
  { file: /^src\/view\/(JobsPane|HelpTip)\.tsx$/, titles: [JOBS], vitals: [JOBS_VITALS] },
  {
    file: /^src\/(view\/(StylePane|FontField|LayeredText|ProfilesCard)\.tsx|layout\/preview-layout\.ts)$/,
    titles: [STYLE],
    vitals: [STYLE_VITALS],
  },
  { file: /^src\/view\/ConfirmButton\.tsx$/, titles: [STYLE, MODELS], vitals: [STYLE_VITALS, MODELS_VITALS] },
  { file: /^src\/view\/ModelsPane\.tsx$/, titles: [MODELS], vitals: [MODELS_VITALS] },
  { file: /^(src\/view\/HelpPane\.tsx|public\/help\/)/, titles: [HELP], vitals: [HELP_VITALS] },
];
// The languages the sweep covers; the others share their layout, and unit tests check their structure.
const SWEPT_LOCALES: Set<string> = new Set(["ja", "en", "de"]);
// Always kept: the test that proves the sweep can still detect a failure, and the one without a language.
const DETECTOR_TITLE: string = "the sweep detects|the speaker count help";
// A hook checks a sample and stays quick; `test:a11y` is the whole sweep, run by hand.
const SCREEN_SAMPLE: string = " / ja / light$";
const LOCALE_SAMPLE: string = "light";
// One state of each screen, for a file every screen uses.
const SHARED_STATES: string = "jobs empty|style with profiles|models idle|help";
const SHARED_SAMPLE: string = " / ja / light$";
const SHARED_VITALS: string = "jobs empty";

const localeImpact = (locale: string): FileImpact => ({
  titles: [],
  vitals: [],
  locales: [locale],
  shared: false,
});

function impactOf(appPath: string): FileImpact {
  const locale = LOCALE_FILE.exec(appPath)?.[1];
  if (locale !== undefined) {
    return localeImpact(locale);
  }
  const rule = SCREEN_RULES.find(({ file }: ScreenRule): boolean => file.test(appPath));
  return { titles: rule?.titles ?? [], vitals: rule?.vitals ?? [], locales: [], shared: rule === undefined };
}

const isUnsweptLocale = (appPath: string): boolean => {
  const locale = LOCALE_FILE.exec(appPath)?.[1];
  return locale !== undefined && !SWEPT_LOCALES.has(locale);
};

const isUiFile = (appPath: string): boolean =>
  UI_FILE.test(appPath) && !TEST_FILE.test(appPath) && !isUnsweptLocale(appPath);

const unique = (values: string[]): string[] => [...new Set(values)];

/** Titles look like "<state> / <locale> / <theme>", and Playwright matches them after the file name. */
const toGrep = (titles: string[], locales: string[], shared: boolean): string =>
  [
    `\\.spec\\.ts (${[
      DETECTOR_TITLE,
      ...(titles.length > 0 ? [`(${titles.join("|")}).*${SCREEN_SAMPLE}`] : []),
      ...(shared ? [`(${SHARED_STATES})${SHARED_SAMPLE}`] : []),
    ].join("|")})`,
    ...locales.map((locale: string): string => ` / ${locale} / ${LOCALE_SAMPLE}$`),
  ].join("|");

const toVitalsGrep = (vitals: string[]): string | null =>
  vitals.length > 0 ? `stay good: (${vitals.join("|")})` : null;

/** The sample a change needs. Paths may be relative to the repository or to the app. */
export function sweepScope(changedFiles: string[]): SweepScope {
  const impacts: FileImpact[] = changedFiles
    .map((path: string): string => path.replace(APP_PREFIX, ""))
    .filter(isUiFile)
    .map(impactOf);
  if (impacts.length === 0) {
    return { kind: "none" };
  }
  const shared: boolean = impacts.some((impact: FileImpact): boolean => impact.shared);
  return {
    kind: "some",
    grep: toGrep(
      unique(impacts.flatMap(({ titles }: FileImpact): string[] => titles)),
      unique(impacts.flatMap(({ locales }: FileImpact): string[] => locales)),
      shared,
    ),
    vitalsGrep: toVitalsGrep(
      unique([
        ...impacts.flatMap(({ vitals }: FileImpact): string[] => vitals),
        ...(shared ? [SHARED_VITALS] : []),
      ]),
    ),
  };
}

/** A deleted file can be used by anything, and its name says nothing: the sample every shared file gets. */
export const SHARED_SCOPE: SweepScope = {
  kind: "some",
  grep: toGrep([], [], true),
  vitalsGrep: toVitalsGrep([SHARED_VITALS]),
};
