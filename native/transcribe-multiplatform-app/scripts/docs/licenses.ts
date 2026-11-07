// Runs with bun: writes the license list the settings tab shows, from what the build really links and bundles.
// Only what the dependency data gives: tools and models beside the app are in THIRD_PARTY_NOTICES.md.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import type { LicenseEntry, LicenseList } from "../../src/backend/licenses.ts";

export interface FoundPackage {
  name: string;
  version: string;
  license: string;
  url: string;
  directory: string;
  licenseFile: string | null;
}

export interface CargoTarget {
  kind: string[];
}

export interface CargoPackage {
  id: string;
  name: string;
  version: string;
  license: string | null;
  license_file: string | null;
  manifest_path: string;
  repository: string | null;
  homepage: string | null;
  targets: CargoTarget[];
}

export interface CargoDependency {
  pkg: string;
  dep_kinds: { kind: string | null }[];
}

export interface CargoNode {
  id: string;
  deps: CargoDependency[];
}

export interface CargoMetadata {
  packages: CargoPackage[];
  resolve: { root: string; nodes: CargoNode[] };
}

export interface NodeManifest {
  name: string;
  version: string;
  license?: string;
  repository?: string | { url?: string };
  homepage?: string;
  dependencies?: Record<string, string>;
  optionalDependencies?: Record<string, string>;
}

const APP_DIR: string = join(import.meta.dirname, "../..");
export const OUTPUT: string = join(APP_DIR, "public/licenses.js");
/** Every system a build ships for; features are all on, so each GPU build's crates count. */
const TARGETS: string[] = ["aarch64-apple-darwin", "x86_64-pc-windows-msvc"];
const LICENSE_FILE: RegExp = /^(licen[cs]e|copying|notice|unlicense)/i;
const GIT_URL_PARTS: RegExp = /^git\+|\.git$/g;

const isProcMacro = ({ targets }: CargoPackage): boolean =>
  targets.some(({ kind }: CargoTarget): boolean => kind.includes("proc-macro"));

const linksAtRuntime = ({ dep_kinds: kinds }: CargoDependency): boolean =>
  kinds.some(({ kind }: { kind: string | null }): boolean => kind === null);

/** What ends up in the program: normal dependencies only, and nothing only the compiler runs. */
export function shippedCrates({ packages, resolve }: CargoMetadata): CargoPackage[] {
  const byId: Map<string, CargoPackage> = new Map(
    packages.map((found: CargoPackage): [string, CargoPackage] => [found.id, found]),
  );
  const nodes: Map<string, CargoNode> = new Map(
    resolve.nodes.map((node: CargoNode): [string, CargoNode] => [node.id, node]),
  );
  const seen: Set<string> = new Set([resolve.root]);
  const pending: string[] = [resolve.root];
  while (pending.length > 0) {
    const deps: CargoDependency[] = nodes.get(pending.pop() ?? "")?.deps ?? [];
    deps.filter(linksAtRuntime).forEach(({ pkg }: CargoDependency): void => {
      const found: CargoPackage | undefined = byId.get(pkg);
      if (found !== undefined && !isProcMacro(found) && !seen.has(pkg)) {
        seen.add(pkg);
        pending.push(pkg);
      }
    });
  }
  seen.delete(resolve.root);
  return packages.filter(({ id }: CargoPackage): boolean => seen.has(id));
}

const crateFound = (found: CargoPackage): FoundPackage => ({
  name: found.name,
  version: found.version,
  license: found.license ?? "",
  url: found.repository ?? found.homepage ?? "",
  directory: dirname(found.manifest_path),
  licenseFile: found.license_file,
});

function cargoMetadata(target: string): CargoMetadata {
  const run = spawnSync(
    "cargo",
    ["metadata", "--format-version", "1", "--all-features", "--locked", "--filter-platform", target],
    { cwd: join(APP_DIR, "src-tauri"), encoding: "utf8", maxBuffer: 256 * 1024 * 1024 },
  );
  if (run.status !== 0) {
    throw new Error(`cargo metadata failed: ${run.stderr}`);
  }
  return JSON.parse(run.stdout) satisfies CargoMetadata;
}

const manifestUrl = ({ repository, homepage }: NodeManifest): string => {
  const raw: string = (typeof repository === "string" ? repository : repository?.url) ?? homepage ?? "";
  return raw.replace(GIT_URL_PARTS, "");
};

/** Node's lookup: a package's own node_modules first, then the app's. */
const packageDirectory = (name: string, parent: string, root: string): string | null =>
  [join(parent, "node_modules", name), join(root, "node_modules", name)].find((directory: string): boolean =>
    existsSync(join(directory, "package.json")),
  ) ?? null;

const readManifest = (directory: string): NodeManifest =>
  JSON.parse(readFileSync(join(directory, "package.json"), "utf8")) satisfies NodeManifest;

/** The app's runtime dependencies and theirs; an optional one this system did not install is not shipped from here. */
export function shippedNodePackages(root: string): FoundPackage[] {
  const found: Map<string, FoundPackage> = new Map();
  const visit = (name: string, parent: string, optional: boolean): void => {
    const directory: string | null = packageDirectory(name, parent, root);
    if (directory === null) {
      if (optional) {
        return;
      }
      throw new Error(`${name} is not installed: run bun install`);
    }
    const manifest: NodeManifest = readManifest(directory);
    const key = `${manifest.name}@${manifest.version}`;
    if (found.has(key)) {
      return;
    }
    found.set(key, {
      name: manifest.name,
      version: manifest.version,
      license: manifest.license ?? "",
      url: manifestUrl(manifest),
      directory,
      licenseFile: null,
    });
    Object.keys(manifest.dependencies ?? {}).forEach((next: string): void => {
      visit(next, directory, false);
    });
    Object.keys(manifest.optionalDependencies ?? {}).forEach((next: string): void => {
      visit(next, directory, true);
    });
  };
  Object.keys(readManifest(root).dependencies ?? {}).forEach((name: string): void => {
    visit(name, root, false);
  });
  return [...found.values()];
}

/** Every license file the package ships, as they are; nothing is filled in for a package without one. */
export function licenseText({
  directory,
  licenseFile,
}: Pick<FoundPackage, "directory" | "licenseFile">): string | null {
  const named: string[] = readdirSync(directory)
    .filter((file: string): boolean => LICENSE_FILE.test(file) && statSync(join(directory, file)).isFile())
    .sort();
  const files: string[] =
    licenseFile === null || named.includes(licenseFile) ? named : [...named, licenseFile];
  const texts: string[] = files
    .map((file: string): string => join(directory, file))
    .filter((path: string): boolean => existsSync(path))
    .map((path: string): string => readFileSync(path, "utf8").trim());
  return texts.length === 0 ? null : texts.join("\n\n");
}

const compareEntries = (left: FoundPackage, right: FoundPackage): number =>
  left.name === right.name ? left.version.localeCompare(right.version) : left.name.localeCompare(right.name);

/** One row per name and version, with equal license texts stored once. */
export function buildList(found: FoundPackage[]): LicenseList {
  const unique: FoundPackage[] = [
    ...new Map(
      found.map((item: FoundPackage): [string, FoundPackage] => [`${item.name}@${item.version}`, item]),
    ).values(),
  ].sort(compareEntries);
  const texts: string[] = [];
  const indexes: Map<string, number> = new Map();
  const entries: LicenseEntry[] = unique.map((item: FoundPackage): LicenseEntry => {
    const text: string | null = licenseText(item);
    if (text !== null && !indexes.has(text)) {
      indexes.set(text, texts.length);
      texts.push(text);
    }
    return {
      name: item.name,
      version: item.version,
      license: item.license,
      url: item.url,
      text: text === null ? null : (indexes.get(text) ?? null),
    };
  });
  return { entries, texts };
}

/** A module, loaded only when the list is opened: see src/backend/licenses.ts. */
export const serialized = (list: LicenseList): string => `export default ${JSON.stringify(list, null, 2)};\n`;

export const collectAll = (): FoundPackage[] => [
  ...TARGETS.flatMap((target: string): FoundPackage[] =>
    shippedCrates(cargoMetadata(target)).map(crateFound),
  ),
  ...shippedNodePackages(APP_DIR),
];

/** The file sits under a folder the app watcher builds on, so an unchanged rewrite would start another build. */
export function writeIfChanged(path: string, text: string): void {
  if (!existsSync(path) || readFileSync(path, "utf8") !== text) {
    writeFileSync(path, text);
  }
}

if (import.meta.main) {
  const list: LicenseList = buildList(collectAll());
  mkdirSync(dirname(OUTPUT), { recursive: true });
  writeIfChanged(OUTPUT, serialized(list));
  const missing: number = list.entries.filter(({ text }: LicenseEntry): boolean => text === null).length;
  console.log(
    `licenses: ${list.entries.length} packages, ${missing} without a license text, ${list.texts.length} texts`,
  );
}
