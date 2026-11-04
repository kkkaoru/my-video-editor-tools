// Runs with bun: proves the claims at the top of styles.css for both colour schemes (WCAG 1.4.6, 1.4.11).
import { expect, it } from "vitest";
import styles from "./styles.css?raw";

const DARK_MARKER: string = "@media (prefers-color-scheme: dark)";
const TOKEN: RegExp = /--([\w-]+):\s*(#[0-9a-f]{6});/g;
const RED_START: number = 1;
const GREEN_START: number = 3;
const BLUE_START: number = 5;
const CHANNEL_MAX: number = 255;

const tokensOf = (css: string): Map<string, string> =>
  new Map(
    Array.from(css.matchAll(TOKEN), ([, name, value]: RegExpExecArray): [string, string] => [
      name ?? "",
      value ?? "",
    ]),
  );

function linear(hex: string, start: number): number {
  const channel: number = Number.parseInt(hex.slice(start, start + 2), 16) / CHANNEL_MAX;
  return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
}

const luminance = (hex: string): number =>
  0.2126 * linear(hex, RED_START) + 0.7152 * linear(hex, GREEN_START) + 0.0722 * linear(hex, BLUE_START);

const contrastOfHex = (first: string, second: string): number =>
  (Math.max(luminance(first), luminance(second)) + 0.05) /
  (Math.min(luminance(first), luminance(second)) + 0.05);

const contrast = (theme: Map<string, string>, foreground: string, background: string): number =>
  contrastOfHex(theme.get(foreground) ?? "", theme.get(background) ?? "");

const LIGHT: Map<string, string> = tokensOf(styles.slice(0, styles.indexOf(DARK_MARKER)));
const DARK: Map<string, string> = tokensOf(styles.slice(styles.indexOf(DARK_MARKER)));

it("measures contrast the way WCAG defines it", () => {
  expect(contrastOfHex("#000000", "#ffffff")).toBeCloseTo(21);
  expect(contrastOfHex("#ffffff", "#ffffff")).toBe(1);
  expect(contrastOfHex("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
});

it("reads every colour token of each theme from the stylesheet", () => {
  expect(Array.from(LIGHT.keys())).toStrictEqual([
    "background",
    "foreground",
    "card",
    "card-foreground",
    "popover",
    "popover-foreground",
    "primary",
    "primary-hover",
    "primary-foreground",
    "muted",
    "muted-foreground",
    "accent",
    "accent-foreground",
    "destructive",
    "success",
    "border",
    "ring",
    "page",
  ]);
  expect(Array.from(DARK.keys())).toStrictEqual([
    "background",
    "foreground",
    "card",
    "card-foreground",
    "popover",
    "popover-foreground",
    "primary",
    "primary-hover",
    "primary-foreground",
    "muted",
    "muted-foreground",
    "accent",
    "accent-foreground",
    "destructive",
    "success",
    "border",
    "ring",
    "page",
  ]);
  expect(LIGHT.get("background")).toBe("#ffffff");
  expect(DARK.get("background")).toBe("#0a0a0a");
});

it("gives light text at least 7:1", () => {
  expect(contrast(LIGHT, "foreground", "background")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "foreground", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "foreground", "page")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "foreground", "muted")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "card-foreground", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "popover-foreground", "popover")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "muted-foreground", "background")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "muted-foreground", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "muted-foreground", "page")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "muted-foreground", "muted")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "destructive", "background")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "destructive", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "destructive", "page")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "success", "background")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "success", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "success", "page")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "primary-foreground", "primary")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "primary-foreground", "primary-hover")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "accent-foreground", "accent")).toBeGreaterThanOrEqual(7);
});

it("gives light lines, the focus ring and solid controls at least 3:1", () => {
  expect(contrast(LIGHT, "border", "background")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "border", "card")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "border", "page")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "ring", "background")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "ring", "card")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "ring", "page")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "primary", "background")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "primary", "card")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "primary", "page")).toBeGreaterThanOrEqual(3);
});

it("gives dark text at least 7:1", () => {
  expect(contrast(DARK, "foreground", "background")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "foreground", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "foreground", "page")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "foreground", "muted")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "card-foreground", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "popover-foreground", "popover")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "muted-foreground", "background")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "muted-foreground", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "muted-foreground", "page")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "muted-foreground", "muted")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "destructive", "background")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "destructive", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "destructive", "page")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "success", "background")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "success", "card")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "success", "page")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "primary-foreground", "primary")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "primary-foreground", "primary-hover")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "accent-foreground", "accent")).toBeGreaterThanOrEqual(7);
});

it("gives dark lines, the focus ring and solid controls at least 3:1", () => {
  expect(contrast(DARK, "border", "background")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "border", "card")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "border", "page")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "ring", "background")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "ring", "card")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "ring", "page")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "primary", "background")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "primary", "card")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "primary", "page")).toBeGreaterThanOrEqual(3);
});
