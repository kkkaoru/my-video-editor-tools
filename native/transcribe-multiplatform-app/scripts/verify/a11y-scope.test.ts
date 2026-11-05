// Runs with bun: vitest for the sweep scope.
import { expect, it } from "vitest";
import { sweepScope } from "./a11y-scope.ts";

it("needs no sweep when only tests, Rust or documents change", () => {
  expect(
    sweepScope([
      "src/view/App.test.tsx",
      "src-tauri/src/lib.rs",
      "README.md",
      "native/transcribe-multiplatform-app/scripts/verify/watch-app.ts",
    ]),
  ).toStrictEqual({ kind: "none" });
});

it("sweeps everything for a file used across screens", () => {
  expect(sweepScope(["src/entry/styles.css"])).toStrictEqual({ kind: "all" });
});

it("sweeps everything for a sweep or build change", () => {
  expect(sweepScope(["tests/a11y/fixtures.ts"])).toStrictEqual({ kind: "all" });
});

it("sweeps everything when a shared file comes with a screen file", () => {
  expect(sweepScope(["src/view/JobsPane.tsx", "src/view/fields.tsx"])).toStrictEqual({ kind: "all" });
});

it("sweeps one screen for its own file and strips the repository path", () => {
  expect(sweepScope(["native/transcribe-multiplatform-app/src/view/ModelsPane.tsx"])).toStrictEqual({
    kind: "some",
    grep: "\\.spec\\.ts (the sweep detects|models? )",
    vitalsGrep: "stay good: (models )",
  });
});

it("sweeps both screens for a component they share, without repeats", () => {
  expect(sweepScope(["src/view/ConfirmButton.tsx", "src/view/StylePane.tsx"])).toStrictEqual({
    kind: "some",
    grep: "\\.spec\\.ts (the sweep detects|style |speaker names|models? )",
    vitalsGrep: "stay good: (style |models )",
  });
});

it("sweeps one covered language for its text", () => {
  expect(sweepScope(["src/i18n/locale-de.ts"])).toStrictEqual({
    kind: "some",
    grep: "\\.spec\\.ts (the sweep detects)| / de( / (light|dark))?$",
    vitalsGrep: null,
  });
});

it("needs no sweep for a language outside the covered three", () => {
  expect(sweepScope(["src/i18n/locale-fr.ts", "src/i18n/locale-zh-hant.ts"])).toStrictEqual({ kind: "none" });
});

it("sweeps the help screen for its images", () => {
  expect(sweepScope(["public/help/ja/start.webp"])).toStrictEqual({
    kind: "some",
    grep: "\\.spec\\.ts (the sweep detects|help )",
    vitalsGrep: "stay good: (help)",
  });
});
