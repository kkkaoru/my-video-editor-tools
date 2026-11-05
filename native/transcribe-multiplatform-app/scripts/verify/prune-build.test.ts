// Runs with bun: drives the real prune script against a throwaway build directory inside node_modules/.cache.
import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { afterEach, expect, it } from "vitest";

const SCRIPT: string = join(import.meta.dirname, "prune-build.sh");
const CACHE: string = join(import.meta.dirname, "../../node_modules/.cache");
const MEBIBYTE: number = 1024 * 1024;
const CREATED: string[] = [];

function fakeApp(): string {
  mkdirSync(CACHE, { recursive: true });
  const root: string = mkdtempSync(join(CACHE, "prune-test-"));
  CREATED.push(root);
  mkdirSync(join(root, "scripts/verify"), { recursive: true });
  copyFileSync(SCRIPT, join(root, "scripts/verify/prune-build.sh"));
  return root;
}

function addFile(root: string, path: string, bytes: number): void {
  const file: string = join(root, path);
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, Buffer.alloc(bytes, 1));
}

function addFullTarget(root: string, target: string): void {
  addFile(root, `${target}/llvm-cov-target/a`, MEBIBYTE);
  addFile(root, `${target}/debug/deps/a`, MEBIBYTE);
  addFile(root, `${target}/release/bundle/app`, 1024);
}

function prune(root: string, budgetKib: string, cargoRunning: boolean): string {
  const stub: string = join(root, "pgrep");
  writeFileSync(stub, `#!/bin/sh\nexit ${cargoRunning ? 0 : 1}\n`);
  chmodSync(stub, 0o755);
  const result = spawnSync("bash", [join(root, "scripts/verify/prune-build.sh")], {
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${root}:${process.env.PATH}`,
      CARGO_TARGET_DIR: join(root, "shared"),
      TARGET_BUDGET_KIB: budgetKib,
    },
  });
  expect(result.status).toBe(0);
  return result.stdout;
}

afterEach((): void => {
  CREATED.splice(0).forEach((root: string): void => {
    rmSync(root, { recursive: true, force: true });
  });
});

it("keeps every build under the budget", () => {
  const root: string = fakeApp();
  addFullTarget(root, "shared");
  prune(root, "8192", false);
  expect(existsSync(join(root, "shared/llvm-cov-target/a"))).toBe(true);
  expect(existsSync(join(root, "shared/debug/deps/a"))).toBe(true);
});

it("drops only the coverage build when that is enough", () => {
  const root: string = fakeApp();
  addFullTarget(root, "shared");
  prune(root, "1536", false);
  expect(existsSync(join(root, "shared/llvm-cov-target"))).toBe(false);
  expect(existsSync(join(root, "shared/debug/deps/a"))).toBe(true);
});

it("drops debug too when still over, and never the finished app", () => {
  const root: string = fakeApp();
  addFullTarget(root, "shared");
  prune(root, "128", false);
  expect(existsSync(join(root, "shared/llvm-cov-target"))).toBe(false);
  expect(existsSync(join(root, "shared/debug"))).toBe(false);
  expect(existsSync(join(root, "shared/release/bundle/app"))).toBe(true);
});

it("caps the directory rust-analyzer writes to as well", () => {
  const root: string = fakeApp();
  addFullTarget(root, "src-tauri/target");
  prune(root, "128", false);
  expect(existsSync(join(root, "src-tauri/target/debug"))).toBe(false);
  expect(existsSync(join(root, "src-tauri/target/release/bundle/app"))).toBe(true);
});

it("touches nothing while cargo is running", () => {
  const root: string = fakeApp();
  addFullTarget(root, "shared");
  addFile(root, "coverage/report", 1024);
  expect(prune(root, "128", true)).toBe("prune-build: cargo is running, skipped\n");
  expect(existsSync(join(root, "shared/debug/deps/a"))).toBe(true);
  expect(existsSync(join(root, "coverage/report"))).toBe(true);
});

it("succeeds when no build exists yet", () => {
  const root: string = fakeApp();
  prune(root, "128", false);
  expect(existsSync(join(root, "shared"))).toBe(false);
});
