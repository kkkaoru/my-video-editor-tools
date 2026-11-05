// Runs with bun: every picture belongs to a topic with numbered steps, and exists in every language.
import { existsSync } from "node:fs";
import { expect, it } from "vitest";
import { ALL_LANGUAGE_MESSAGES } from "../i18n/language.ts";
import { HELP_IMAGE_SIZES, imageOfPart } from "./help-images.ts";

it("maps topics to their pictures", () => {
  expect(imageOfPart("start", 1)).toBe("models");
  expect(imageOfPart("look", 2)).toBe("profiles");
  expect(imageOfPart("trouble", 0)).toBe(undefined);
});

it("has a picture file for every picture in every language", () => {
  expect(
    ALL_LANGUAGE_MESSAGES.flatMap(({ help: { imageFolder } }): string[] =>
      Object.keys(HELP_IMAGE_SIZES)
        .map((name: string): string => `public/help/${imageFolder}/${name}.webp`)
        .filter((file: string): boolean => !existsSync(file)),
    ),
  ).toStrictEqual([]);
});
