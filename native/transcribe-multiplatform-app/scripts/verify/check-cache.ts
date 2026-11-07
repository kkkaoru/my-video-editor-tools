// Runs with bun: remembers which checks passed on which inputs, so the same inputs are not checked twice.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

export type PassedChecks = Record<string, string>;

export interface CheckIdentity {
  name: string;
  command: string[];
  inputs: string[];
}

const SKIPPED_FOLDERS: Set<string> = new Set(["node_modules", "target", "gen", ".git", "dist"]);

const filesUnder = (root: string, path: string): string[] => {
  const full: string = join(root, path);
  if (!existsSync(full)) {
    return [];
  }
  if (!statSync(full).isDirectory()) {
    return [path];
  }
  return readdirSync(full)
    .filter((name: string): boolean => !SKIPPED_FOLDERS.has(name))
    .sort()
    .flatMap((name: string): string[] => filesUnder(root, join(path, name)));
};

/** Changes when the command, a file's name or a file's contents do. */
export function digestOf(root: string, { command, inputs }: CheckIdentity): string {
  const hash = createHash("sha256").update(command.join("\0"));
  inputs
    .flatMap((input: string): string[] => filesUnder(root, input))
    .forEach((file: string): void => {
      hash.update(`\0${file}\0`).update(readFileSync(join(root, file)));
    });
  return hash.digest("hex");
}

/** An unreadable record only means checking again. */
export function loadPassed(file: string): PassedChecks {
  try {
    return JSON.parse(readFileSync(file, "utf8")) as PassedChecks;
  } catch {
    return {};
  }
}

export function savePassed(file: string, passed: PassedChecks): void {
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, JSON.stringify(passed));
}
