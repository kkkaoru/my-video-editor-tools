// Runs with bun: which help topic shows which annotated picture, and each picture's size, so nothing shifts as it loads.
import type { HelpImages, HelpSections, ResolveFigures } from "../i18n/messages.ts";

export type HelpImageName = keyof HelpImages;

export interface HelpImageSize {
  width: number;
  height: number;
}

export const HELP_IMAGE_SIZES: Record<HelpImageName, HelpImageSize> = {
  models: { width: 1100, height: 920 },
  transcribe: { width: 1100, height: 600 },
  look: { width: 1100, height: 1500 },
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

export type ResolveFigureName = keyof ResolveFigures;

export const RESOLVE_FIGURE_SIZES: Record<ResolveFigureName, HelpImageSize> = {
  menu: { width: 1400, height: 1240 },
  dialog: { width: 1120, height: 475 },
  timeline: { width: 1400, height: 383 },
  text: { width: 841, height: 1044 },
  shading: { width: 841, height: 1044 },
};

/** `section.part.step` of the help text to the pictures under that step. */
const FIGURES_OF_STEP: ReadonlyMap<string, ResolveFigureName[]> = new Map<string, ResolveFigureName[]>([
  ["editor.0.1", ["menu"]],
  ["editor.0.3", ["dialog"]],
  ["editor.0.4", ["timeline", "text", "shading"]],
]);

export const figuresOfStep = (section: keyof HelpSections, part: number, step: number): ResolveFigureName[] =>
  FIGURES_OF_STEP.get(`${section}.${part}.${step}`) ?? [];
