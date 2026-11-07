// Runs with bun: the list holds what the build ships and what each package says, and it is up to date.
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, beforeAll, expect, it } from "vitest";
import {
  buildList,
  type CargoMetadata,
  collectAll,
  type FoundPackage,
  licenseText,
  OUTPUT,
  serialized,
  shippedCrates,
  shippedNodePackages,
} from "./licenses.ts";

const ROOT: string = join(import.meta.dirname, "../../node_modules/.cache/licenses-test");

const crate = (id: string, kind: string): CargoMetadata["packages"][number] => ({
  id,
  name: id,
  version: "1.0.0",
  license: "MIT",
  license_file: null,
  manifest_path: `/c/${id}/Cargo.toml`,
  repository: null,
  homepage: null,
  targets: [{ kind: [kind] }],
});

const file = (path: string, text: string): void => {
  mkdirSync(join(ROOT, path, ".."), { recursive: true });
  writeFileSync(join(ROOT, path), text);
};

beforeAll(() => {
  rmSync(ROOT, { recursive: true, force: true });
  file("a/LICENSE-MIT", "mit text\n");
  file("a/LICENSE-APACHE", "apache text\n");
  file("a/README.md", "not a license");
  file("b/COPYING", "copying text");
  file("c/lib.rs", "");
  file("d/docs/TERMS.txt", "terms");
  file("same/LICENSE", "mit text\n\napache");
  file("app/package.json", JSON.stringify({ name: "app", version: "1.0.0", dependencies: { one: "1" } }));
});

afterAll(() => {
  rmSync(ROOT, { recursive: true, force: true });
});

it("keeps normal dependencies, without what only the compiler or the tests run", () => {
  const metadata: CargoMetadata = {
    packages: [
      crate("root", "bin"),
      crate("linked", "lib"),
      crate("deep", "lib"),
      crate("macro", "proc-macro"),
      crate("macro-only", "lib"),
      crate("build-only", "lib"),
      crate("dev-only", "lib"),
    ],
    resolve: {
      root: "root",
      nodes: [
        {
          id: "root",
          deps: [
            { pkg: "linked", dep_kinds: [{ kind: null }] },
            { pkg: "macro", dep_kinds: [{ kind: null }] },
            { pkg: "build-only", dep_kinds: [{ kind: "build" }] },
            { pkg: "dev-only", dep_kinds: [{ kind: "dev" }] },
            { pkg: "not-listed", dep_kinds: [{ kind: null }] },
          ],
        },
        {
          id: "linked",
          deps: [
            { pkg: "deep", dep_kinds: [{ kind: null }] },
            { pkg: "linked", dep_kinds: [{ kind: null }] },
          ],
        },
        { id: "macro", deps: [{ pkg: "macro-only", dep_kinds: [{ kind: null }] }] },
      ],
    },
  };
  expect(shippedCrates(metadata).map(({ name }): string => name)).toStrictEqual(["linked", "deep"]);
});

it("reads every license file a package ships, as it is", () => {
  expect(licenseText({ directory: join(ROOT, "a"), licenseFile: null })).toBe("apache text\n\nmit text");
  expect(licenseText({ directory: join(ROOT, "b"), licenseFile: null })).toBe("copying text");
});

it("also reads the file a manifest names", () => {
  expect(licenseText({ directory: join(ROOT, "d"), licenseFile: "docs/TERMS.txt" })).toBe("terms");
  expect(licenseText({ directory: join(ROOT, "a"), licenseFile: "LICENSE-MIT" })).toBe(
    "apache text\n\nmit text",
  );
});

it("makes up nothing for a package without a license file", () => {
  expect(licenseText({ directory: join(ROOT, "c"), licenseFile: null })).toBe(null);
  expect(licenseText({ directory: join(ROOT, "c"), licenseFile: "gone.txt" })).toBe(null);
});

it("sorts, keeps one row per version, and stores equal texts once", () => {
  const found = (name: string, version: string, directory: string): FoundPackage => ({
    name,
    version,
    license: "MIT",
    url: "",
    directory: join(ROOT, directory),
    licenseFile: null,
  });
  expect(
    buildList([
      found("zeta", "1.0.0", "b"),
      found("alpha", "2.0.0", "c"),
      found("alpha", "1.0.0", "b"),
      found("zeta", "1.0.0", "b"),
    ]),
  ).toStrictEqual({
    entries: [
      { name: "alpha", version: "1.0.0", license: "MIT", url: "", text: 0 },
      { name: "alpha", version: "2.0.0", license: "MIT", url: "", text: null },
      { name: "zeta", version: "1.0.0", license: "MIT", url: "", text: 0 },
    ],
    texts: ["copying text"],
  });
});

it("follows a node package's dependencies, nearest copy first, without a missing optional one", () => {
  file(
    "app/node_modules/one/package.json",
    JSON.stringify({
      name: "one",
      version: "1.0.0",
      license: "MIT",
      repository: { url: "git+https://example.com/one.git" },
      dependencies: { two: "1", one: "1" },
      optionalDependencies: { absent: "1" },
    }),
  );
  file(
    "app/node_modules/one/node_modules/two/package.json",
    JSON.stringify({
      name: "two",
      version: "2.0.0",
      homepage: "https://example.com/two",
    }),
  );
  expect(
    shippedNodePackages(join(ROOT, "app")).map(({ name, license, url }): string[] => [name, license, url]),
  ).toStrictEqual([
    ["one", "MIT", "https://example.com/one"],
    ["two", "", "https://example.com/two"],
  ]);
});

it("refuses a required node package that is not installed", () => {
  file(
    "broken/package.json",
    JSON.stringify({ name: "broken", version: "1.0.0", dependencies: { lost: "1" } }),
  );
  expect(() => shippedNodePackages(join(ROOT, "broken"))).toThrow("lost is not installed: run bun install");
});

it("is up to date with the dependencies", { timeout: 120_000 }, () => {
  expect(readFileSync(OUTPUT, "utf8")).toBe(serialized(buildList(collectAll())));
});
