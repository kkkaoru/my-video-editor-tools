// Runs with bun: runs the checks the staged files need, compared with CHECK_BASE when it is set.
// An argument keeps only the checks whose name contains it: `bun scripts/verify/changed-checks.ts WCAG`.
// A check whose inputs have not changed since it passed is skipped; CHECK_NO_CACHE=1 runs it anyway.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { type Check, planChecks } from "./changed-plan.ts";
import { digestOf, loadPassed, type PassedChecks, savePassed } from "./check-cache.ts";

const APP_DIR: string = join(import.meta.dirname, "../..");
const RUST_DIR: string = join(APP_DIR, "src-tauri");
const RUST_SOURCES: string = join(RUST_DIR, "src");
const APP_PREFIX: string = "native/transcribe-multiplatform-app/";
const CRATE_PATH: RegExp = /crate::(?:\{([^}]*)\}|([a-z_]+))/g;
const RUST_FILE: RegExp = /\.rs$/;
const DISTRIBUTION_FEATURE: string = 'feature = "windows-';
const PASSED_FILE: string = join(APP_DIR, "node_modules/.cache/changed-checks-passed.json");

const changedFiles = (filter: string): string[] =>
  spawnSync(
    "git",
    [
      "diff",
      "--cached",
      "--name-only",
      `--diff-filter=${filter}`,
      ...(process.env.CHECK_BASE ? [process.env.CHECK_BASE] : []),
    ],
    { cwd: APP_DIR, encoding: "utf8" },
  )
    .stdout.split("\n")
    .filter((path: string): boolean => path.startsWith(APP_PREFIX))
    .map((path: string): string => path.slice(APP_PREFIX.length));

const usedModules = (source: string): string[] =>
  [...source.matchAll(CRATE_PATH)].flatMap(([, group, single]: RegExpExecArray): string[] =>
    (group ?? single ?? "").split(",").map((name: string): string => name.trim().split("::")[0] ?? ""),
  );

const moduleUses = (): Map<string, string[]> =>
  new Map(
    readdirSync(RUST_SOURCES)
      .filter((file: string): boolean => RUST_FILE.test(file))
      .map((file: string): [string, string[]] => [
        file.replace(RUST_FILE, ""),
        usedModules(readFileSync(join(RUST_SOURCES, file), "utf8")),
      ]),
  );

const featureModules = (): string[] =>
  readdirSync(RUST_SOURCES)
    .filter(
      (file: string): boolean =>
        RUST_FILE.test(file) && readFileSync(join(RUST_SOURCES, file), "utf8").includes(DISTRIBUTION_FEATURE),
    )
    .map((file: string): string => file.replace(RUST_FILE, ""));

const wanted = ({ name }: Check): boolean => name.includes(process.argv[2] ?? "");

const alreadyPassed = (check: Check, passed: PassedChecks): boolean =>
  process.env.CHECK_NO_CACHE === undefined &&
  passed[check.name] === digestOf(APP_DIR, check) &&
  check.outputs.every((output: string): boolean => existsSync(join(APP_DIR, output)));

/** Output is held back while checks run together, so each reads as one piece. */
const run = ({ name, directory, command, phase }: Check): Promise<boolean> =>
  new Promise((resolve) => {
    console.log(`changed-checks: ${name}: ${command.join(" ")}`);
    const child = spawn(command[0] ?? "", command.slice(1), {
      cwd: directory === "rust" ? RUST_DIR : APP_DIR,
      stdio: phase === "quick" ? "pipe" : "inherit",
    });
    const held: Buffer[] = [];
    child.stdout?.on("data", (chunk: Buffer): void => void held.push(chunk));
    child.stderr?.on("data", (chunk: Buffer): void => void held.push(chunk));
    child.on("close", (status: number | null): void => {
      if (status !== 0) {
        process.stdout.write(Buffer.concat(held));
      }
      resolve(status === 0);
    });
  });

async function runRecorded(check: Check, passed: PassedChecks): Promise<boolean> {
  if (alreadyPassed(check, passed)) {
    console.log(`changed-checks: ${check.name}: same inputs passed before, skipped`);
    return true;
  }
  const before: string = digestOf(APP_DIR, check);
  const ok: boolean = await run(check);
  if (ok) {
    passed[check.name] = before;
    savePassed(PASSED_FILE, passed);
  }
  return ok;
}

async function main(): Promise<boolean> {
  const checks: Check[] = planChecks({
    files: changedFiles("d"),
    moduleUses: moduleUses(),
    everything: changedFiles("D").length > 0,
    featureModules: featureModules(),
  }).filter(wanted);
  const passed: PassedChecks = loadPassed(PASSED_FILE);
  const quick: boolean[] = await Promise.all(
    checks
      .filter(({ phase }: Check): boolean => phase === "quick")
      .map((check: Check): Promise<boolean> => runRecorded(check, passed)),
  );
  if (!quick.every(Boolean)) {
    return false;
  }
  const slow: Check[] = checks.filter(({ phase }: Check): boolean => phase === "slow");
  return slow.reduce(
    async (ok: Promise<boolean>, check: Check): Promise<boolean> =>
      (await ok) ? runRecorded(check, passed) : false,
    Promise.resolve(true),
  );
}

process.exit((await main()) ? 0 : 1);
