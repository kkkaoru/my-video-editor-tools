// Runs with bun: decides which checks a list of changed app files needs, as plain data.
import { SHARED_SCOPE, type SweepScope, sweepScope } from "./a11y-scope.ts";

/** Quick checks run first and together, so a typo stops the commit in seconds. */
export type CheckPhase = "quick" | "slow";

export interface Check {
  name: string;
  directory: "app" | "rust";
  command: string[];
  phase: CheckPhase;
  /** Paths relative to the app: a check that passed is not run again until one of them changes. */
  inputs: string[];
  /** What a later check needs on disk, so its absence means running this one again. */
  outputs: string[];
}

export interface PlanInput {
  /** Paths relative to the app. */
  files: string[];
  /** Rust module to the modules it uses, from `crate::` paths. */
  moduleUses: Map<string, string[]>;
  /** A file was deleted: what depended on it cannot be told from the rest of the list. */
  everything: boolean;
  /** Rust modules that read the distribution's features: only they differ between the builds. */
  featureModules: string[];
}

const WHOLE_UI: RegExp = /^(package\.json|bun\.lock|tsconfig[^/]*\.json|vite\.config\.ts)$/;
const WHOLE_RUST: RegExp =
  /^src-tauri\/(Cargo\.(toml|lock)|build\.rs|tauri\.conf\.json|rustfmt\.toml|resources\/|capabilities\/|src\/(lib|main)\.rs)/;
const TYPED: RegExp = /\.tsx?$/;
// biome.json ignores src-tauri, and biome fails when every path it gets is ignored.
const LINTED: RegExp = /^(?!src-tauri\/).*\.(tsx?|json|css)$/;
const VITEST_INPUT: RegExp = /^(src|scripts)\/.*\.(tsx?|css)$/;
const COVERED_SOURCE: RegExp = /^src\/.*\.tsx?$/;
const NOT_COVERED: RegExp = /\.test\.tsx?$|^src\/backend\/types\.ts$/;
const SHELL: RegExp = /\.sh$/;
const RUST_SOURCE: RegExp = /^src-tauri\/(src|tests)\/.*\.rs$/;
const RUST_MODULE: RegExp = /^src-tauri\/src\/([a-z_]+)\.rs$/;
const COVERAGE_SCRIPT: string = "../scripts/verify/check-rust-coverage.sh";
const SWEEP_CONFIG: string[] = ["--config", "tests/a11y/playwright.config.ts"];
// One browser and one worker: a hook has to stay quick. `test:a11y` runs both browsers by hand.
const SWEEP_PROJECTS: string[] = ["--project=chromium", "--workers=1"];
const CLIPPY_FEATURES: string[][] = [[], ["--features", "windows-geforce"], ["--features", "windows-radeon"]];

const UI_INPUTS: string[] = [
  "src",
  "public",
  "scripts",
  "tests",
  "index.html",
  "package.json",
  "bun.lock",
  "tsconfig.json",
  "vite.config.ts",
  "biome.json",
];
const RUST_INPUTS: string[] = [
  "src-tauri/src",
  "src-tauri/tests",
  "src-tauri/resources",
  "src-tauri/capabilities",
  "src-tauri/build.rs",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
  "src-tauri/tauri.conf.json",
  "scripts/verify/check-rust-coverage.sh",
];

const inApp = (name: string, phase: CheckPhase, command: string[], outputs: string[] = []): Check => ({
  name,
  directory: "app",
  command,
  phase,
  inputs: UI_INPUTS,
  outputs,
});

const inRust = (name: string, phase: CheckPhase, command: string[]): Check => ({
  name,
  directory: "rust",
  command,
  phase,
  inputs: RUST_INPUTS,
  outputs: [],
});

const matching = (files: string[], pattern: RegExp): string[] =>
  files.filter((file: string): boolean => pattern.test(file));

/** The modules given, plus every module reaching them through `crate::` paths. */
export function withDependents(modules: string[], moduleUses: Map<string, string[]>): string[] {
  const reached: Set<string> = new Set(modules);
  const visit = (module: string): void => {
    moduleUses.forEach((uses: string[], user: string): void => {
      if (uses.includes(module) && !reached.has(user)) {
        reached.add(user);
        visit(user);
      }
    });
  };
  modules.forEach(visit);
  return [...reached].sort();
}

const typeChecks = (files: string[], whole: boolean): Check[] =>
  whole || matching(files, TYPED).length > 0 ? [inApp("types", "quick", ["bun", "run", "tsc"])] : [];

function lintChecks(files: string[], whole: boolean): Check[] {
  const linted: string[] = matching(files, LINTED);
  if (whole || files.includes("biome.json")) {
    return [inApp("lint", "quick", ["bun", "run", "lint"])];
  }
  return linted.length > 0 ? [inApp("lint", "quick", ["bunx", "biome", "check", ...linted])] : [];
}

function vitestChecks(files: string[], whole: boolean): Check[] {
  const inputs: string[] = matching(files, VITEST_INPUT);
  if (whole) {
    return [inApp("unit tests", "slow", ["bun", "run", "test"])];
  }
  const covered: string[] = matching(inputs, COVERED_SOURCE).filter(
    (file: string): boolean => !NOT_COVERED.test(file),
  );
  const coverage: string[] =
    covered.length > 0
      ? ["--coverage", ...covered.map((file: string): string => `--coverage.include=${file}`)]
      : [];
  return inputs.length > 0
    ? [inApp("unit tests", "slow", ["bunx", "vitest", "related", "--run", ...coverage, ...inputs])]
    : [];
}

const shellChecks = (files: string[]): Check[] => {
  const scripts: string[] = matching(files, SHELL);
  return scripts.length > 0 ? [inApp("shell", "quick", ["shellcheck", ...scripts])] : [];
};

const moduleOf = (file: string): string[] => {
  const name = RUST_MODULE.exec(file)?.[1];
  return name === undefined ? [] : [name];
};

function rustChecks(input: PlanInput): Check[] {
  const sources: string[] = matching(input.files, RUST_SOURCE);
  const whole: boolean = input.everything || matching(input.files, WHOLE_RUST).length > 0;
  if (!whole && sources.length === 0) {
    return [];
  }
  const modules: string[] = sources.flatMap(moduleOf);
  const everyBuild: boolean =
    whole || modules.some((module: string): boolean => input.featureModules.includes(module));
  const clippy: Check[] = (everyBuild ? CLIPPY_FEATURES : CLIPPY_FEATURES.slice(0, 1)).map(
    (features: string[]): Check =>
      inRust(`clippy ${features.join(" ")}`.trim(), "slow", [
        "cargo",
        "clippy",
        "--all-targets",
        ...features,
        "--",
        "-D",
        "warnings",
      ]),
  );
  const format: string[] = whole
    ? ["cargo", "fmt", "--check"]
    : ["rustfmt", "--check", "--edition", "2021", ...sources.map((file: string): string => file.slice(10))];
  const tested: string = withDependents(modules, input.moduleUses).join(",");
  const coverage: string[] = whole
    ? ["bash", COVERAGE_SCRIPT]
    : ["bash", COVERAGE_SCRIPT, tested, modules.join(",")];
  return [
    inRust("rust format", "quick", format),
    inApp("frontend for the Rust build", "slow", ["bun", "run", "build"], ["dist"]),
    ...clippy,
    inRust("rust tests and coverage", "slow", coverage),
  ];
}

function sweepChecks(scope: SweepScope): Check[] {
  const sweep = (grep: string[]): Check =>
    inApp("WCAG sweep", "slow", ["bunx", "playwright", "test", ...SWEEP_CONFIG, ...SWEEP_PROJECTS, ...grep]);
  const vitals = (grep: string[]): Check =>
    inApp("Web Vitals", "slow", [
      "bunx",
      "playwright",
      "test",
      ...SWEEP_CONFIG,
      "--project=vitals",
      "--workers=1",
      ...grep,
    ]);
  if (scope.kind === "none") {
    return [];
  }
  return [
    sweep(["--grep", scope.grep]),
    ...(scope.vitalsGrep === null ? [] : [vitals(["--grep", scope.vitalsGrep])]),
  ];
}

const inPhase =
  (phase: CheckPhase) =>
  (check: Check): boolean =>
    check.phase === phase;

/** Every check the change needs, the quick ones first. File-level tools get the files; whole-project ones are incremental. */
export function planChecks(input: PlanInput): Check[] {
  const whole: boolean = input.everything || matching(input.files, WHOLE_UI).length > 0;
  const checks: Check[] = [
    ...typeChecks(input.files, whole),
    ...lintChecks(input.files, whole),
    ...shellChecks(input.files),
    ...vitestChecks(input.files, whole),
    ...rustChecks(input),
    ...sweepChecks(input.everything ? SHARED_SCOPE : sweepScope(input.files)),
  ];
  return [...checks.filter(inPhase("quick")), ...checks.filter(inPhase("slow"))];
}
