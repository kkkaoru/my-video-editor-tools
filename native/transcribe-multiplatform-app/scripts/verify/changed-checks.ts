// Runs with bun: runs the checks the staged files need, compared with CHECK_BASE when it is set.
// An argument keeps only the checks whose name contains it: `bun scripts/verify/changed-checks.ts WCAG`.
import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { type Check, planChecks } from "./changed-plan.ts";

const APP_DIR: string = join(import.meta.dirname, "../..");
const RUST_DIR: string = join(APP_DIR, "src-tauri");
const RUST_SOURCES: string = join(RUST_DIR, "src");
const APP_PREFIX: string = "native/transcribe-multiplatform-app/";
const CRATE_PATH: RegExp = /crate::(?:\{([^}]*)\}|([a-z_]+))/g;
const RUST_FILE: RegExp = /\.rs$/;

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

function run({ name, directory, command }: Check): boolean {
  console.log(`changed-checks: ${name}: ${command.join(" ")}`);
  return (
    spawnSync(command[0] ?? "", command.slice(1), {
      cwd: directory === "rust" ? RUST_DIR : APP_DIR,
      stdio: "inherit",
    }).status === 0
  );
}

const wanted = ({ name }: Check): boolean => name.includes(process.argv[2] ?? "");

process.exit(
  planChecks({
    files: changedFiles("d"),
    moduleUses: moduleUses(),
    everything: changedFiles("D").length > 0,
  })
    .filter(wanted)
    .every(run)
    ? 0
    : 1,
);
