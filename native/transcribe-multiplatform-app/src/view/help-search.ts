// Runs with bun: which parts of the help page a search finds, in the words the page really shows.
import { format, type HelpPart, type HelpSections, type Params } from "../i18n/messages.ts";

export interface FoundSection {
  key: keyof HelpSections;
  partIndexes: number[];
}

const WIDTH_AND_CASE = (text: string): string => text.normalize("NFKC").toLowerCase();

const partText = ({ heading, intro, steps, notes }: HelpPart, labels: Params): string =>
  [heading, intro, ...steps, ...notes].map((text: string): string => format(text, labels)).join("\n");

const terms = (query: string): string[] =>
  WIDTH_AND_CASE(query)
    .split(/\s+/)
    .filter((term: string): boolean => term.length > 0);

/** Every word must appear in one part; a section's heading finds all its parts. An empty search finds all. */
export function findHelp(
  query: string,
  order: (keyof HelpSections)[],
  sections: HelpSections,
  labels: Params,
): FoundSection[] {
  const wanted: string[] = terms(query);
  const hasAll = (text: string): boolean => {
    const plain: string = WIDTH_AND_CASE(text);
    return wanted.every((term: string): boolean => plain.includes(term));
  };
  return order.flatMap((key: keyof HelpSections): FoundSection[] => {
    const { heading, parts } = sections[key];
    const partIndexes: number[] = parts.flatMap((part: HelpPart, index: number): number[] =>
      hasAll(`${heading}\n${partText(part, labels)}`) ? [index] : [],
    );
    return partIndexes.length > 0 ? [{ key, partIndexes }] : [];
  });
}

export const foundPartCount = (found: FoundSection[]): number =>
  found.reduce((total: number, { partIndexes }: FoundSection): number => total + partIndexes.length, 0);
