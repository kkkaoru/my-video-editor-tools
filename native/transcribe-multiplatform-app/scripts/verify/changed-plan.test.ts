// Runs with bun: vitest for the plan of checks a change needs.
import { expect, it } from "vitest";
import { type Check, planChecks, withDependents } from "./changed-plan.ts";

const USES: Map<string, string[]> = new Map([
  ["captions", ["settings"]],
  ["pipeline", ["captions", "settings"]],
  ["service", ["pipeline"]],
  ["fonts", ["settings"]],
  ["eta", []],
]);

const namesFor = (files: string[], everything: boolean): string[] =>
  planChecks({ files, moduleUses: USES, everything }).map(({ name }: Check): string => name);

const commandOf = (files: string[], name: string): string[] | undefined =>
  planChecks({ files, moduleUses: USES, everything: false }).find(
    (check: Check): boolean => check.name === name,
  )?.command;

it("finds the dependents of a module through other modules", () => {
  expect(withDependents(["captions"], USES)).toStrictEqual(["captions", "pipeline", "service"]);
});

it("finds no dependents for a module nobody uses", () => {
  expect(withDependents(["eta"], USES)).toStrictEqual(["eta"]);
});

it("plans nothing for a document", () => {
  expect(namesFor(["README.md"], false)).toStrictEqual([]);
});

it("lints and tests only the changed screen file", () => {
  expect(commandOf(["src/view/ModelsPane.tsx"], "lint")).toStrictEqual([
    "bunx",
    "biome",
    "check",
    "src/view/ModelsPane.tsx",
  ]);
  expect(commandOf(["src/view/ModelsPane.tsx"], "unit tests")).toStrictEqual([
    "bunx",
    "vitest",
    "related",
    "--run",
    "--coverage",
    "--coverage.include=src/view/ModelsPane.tsx",
    "src/view/ModelsPane.tsx",
  ]);
});

it("does not ask for coverage when only a test changes", () => {
  expect(commandOf(["src/view/ModelsPane.test.tsx"], "unit tests")).toStrictEqual([
    "bunx",
    "vitest",
    "related",
    "--run",
    "src/view/ModelsPane.test.tsx",
  ]);
});

it("runs no browsers and no Rust for a script", () => {
  expect(namesFor(["scripts/verify/watch-app.ts"], false)).toStrictEqual(["types", "lint", "unit tests"]);
});

it("checks only the changed shell script", () => {
  expect(commandOf(["scripts/release/fetch-runtime.sh"], "shell")).toStrictEqual([
    "shellcheck",
    "scripts/release/fetch-runtime.sh",
  ]);
});

it("runs the whole UI suite when its configuration changes", () => {
  expect(commandOf(["vite.config.ts"], "unit tests")).toStrictEqual(["bun", "run", "test"]);
  expect(commandOf(["vite.config.ts"], "lint")).toStrictEqual(["bun", "run", "lint"]);
});

it("does not lint the JSON under src-tauri, which biome ignores", () => {
  expect(commandOf(["src-tauri/tauri.conf.json"], "lint")).toStrictEqual(undefined);
});

it("lints everything when the lint rules change", () => {
  expect(commandOf(["biome.json"], "lint")).toStrictEqual(["bun", "run", "lint"]);
});

it("tests a Rust module with its dependents and measures only the module", () => {
  expect(namesFor(["src-tauri/src/captions.rs"], false)).toStrictEqual([
    "frontend for the Rust build",
    "rust format",
    "clippy",
    "clippy --features windows-geforce",
    "clippy --features windows-radeon",
    "rust tests and coverage",
  ]);
  expect(commandOf(["src-tauri/src/captions.rs"], "rust format")).toStrictEqual([
    "rustfmt",
    "--check",
    "--edition",
    "2021",
    "src/captions.rs",
  ]);
  expect(commandOf(["src-tauri/src/captions.rs"], "rust tests and coverage")).toStrictEqual([
    "bash",
    "../scripts/verify/check-rust-coverage.sh",
    "captions,pipeline,service",
    "captions",
  ]);
});

it("runs only the integration tests when only they change", () => {
  expect(commandOf(["src-tauri/tests/integration.rs"], "rust tests and coverage")).toStrictEqual([
    "bash",
    "../scripts/verify/check-rust-coverage.sh",
    "",
    "",
  ]);
});

it("tests all of Rust when the crate root or dependencies change", () => {
  expect(commandOf(["src-tauri/Cargo.lock"], "rust tests and coverage")).toStrictEqual([
    "bash",
    "../scripts/verify/check-rust-coverage.sh",
  ]);
  expect(commandOf(["src-tauri/src/lib.rs"], "rust format")).toStrictEqual(["cargo", "fmt", "--check"]);
});

it("runs one screen's sweep and Web Vitals", () => {
  expect(commandOf(["src/view/HelpPane.tsx"], "WCAG sweep")).toStrictEqual([
    "bunx",
    "playwright",
    "test",
    "--config",
    "tests/a11y/playwright.config.ts",
    "--project=webkit",
    "--project=chromium",
    "--grep",
    "\\.spec\\.ts (the sweep detects|help )",
  ]);
  expect(commandOf(["src/view/HelpPane.tsx"], "Web Vitals")).toStrictEqual([
    "bunx",
    "playwright",
    "test",
    "--config",
    "tests/a11y/playwright.config.ts",
    "--project=vitals",
    "--workers=1",
    "--grep",
    "stay good: (help)",
  ]);
});

it("skips Web Vitals for a translation", () => {
  expect(namesFor(["src/i18n/locale-de.ts"], false)).toStrictEqual([
    "types",
    "lint",
    "unit tests",
    "WCAG sweep",
  ]);
});

it("runs every check when a file was deleted", () => {
  expect(namesFor([], true)).toStrictEqual([
    "types",
    "lint",
    "unit tests",
    "frontend for the Rust build",
    "rust format",
    "clippy",
    "clippy --features windows-geforce",
    "clippy --features windows-radeon",
    "rust tests and coverage",
    "WCAG sweep",
    "Web Vitals",
  ]);
});
