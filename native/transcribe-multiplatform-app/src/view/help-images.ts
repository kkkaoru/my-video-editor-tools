// Runs with bun: which help topic shows which annotated picture, and each picture's size, so nothing shifts as it loads.
import type { HelpImages, HelpSections } from "../i18n/messages.ts";

export type HelpImageName = keyof HelpImages;

export interface HelpImageSize {
  width: number;
  height: number;
}

export const HELP_IMAGE_SIZES: Record<HelpImageName, HelpImageSize> = {
  models: { width: 1100, height: 920 },
  transcribe: { width: 1100, height: 630 },
  look: { width: 1100, height: 1400 },
  font: { width: 1100, height: 640 },
  profiles: { width: 1100, height: 400 },
  rewrite: { width: 1100, height: 800 },
};

/** `section.part` of the help text to its picture; the picture's numbers are that part's step numbers. */
const IMAGE_OF_PART: ReadonlyMap<string, HelpImageName> = new Map<string, HelpImageName>([
  ["start.1", "models"],
  ["transcribe.0", "transcribe"],
  ["look.0", "look"],
  ["look.1", "font"],
  ["look.2", "profiles"],
  ["rewrite.0", "rewrite"],
]);

export const imageOfPart = (section: keyof HelpSections, part: number): HelpImageName | undefined =>
  IMAGE_OF_PART.get(`${section}.${part}`);
