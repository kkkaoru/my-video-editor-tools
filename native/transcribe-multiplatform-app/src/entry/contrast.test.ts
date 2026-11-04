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

const LIGHT: Map<string, string> = tokensOf(styles.slice(0, styles.indexOf(DARK_MARKER)));
const DARK: Map<string, string> = tokensOf(styles.slice(styles.indexOf(DARK_MARKER)));

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

it("measures contrast the way WCAG defines it", () => {
  expect(contrastOfHex("#000000", "#ffffff")).toBeCloseTo(21);
  expect(contrastOfHex("#ffffff", "#ffffff")).toBe(1);
  expect(contrastOfHex("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
});

it("reads the nine tokens of each theme from the stylesheet", () => {
  expect(Array.from(LIGHT.keys())).toStrictEqual([
    "bg",
    "panel",
    "ink",
    "muted",
    "line",
    "accent",
    "accent-ink",
    "error",
    "ok",
  ]);
  expect(Array.from(DARK.keys())).toStrictEqual([
    "bg",
    "panel",
    "ink",
    "muted",
    "line",
    "accent",
    "accent-ink",
    "error",
    "ok",
  ]);
  expect(LIGHT.get("bg")).toBe("#f4f5f7");
  expect(DARK.get("bg")).toBe("#15171b");
});

it("gives light text at least 7:1", () => {
  expect(contrast(LIGHT, "ink", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "ink", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "muted", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "muted", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "error", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "error", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "ok", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "ok", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "accent", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "accent", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(LIGHT, "accent-ink", "accent")).toBeGreaterThanOrEqual(7);
});

it("gives light lines and the focus ring at least 3:1", () => {
  expect(contrast(LIGHT, "line", "bg")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "line", "panel")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "ink", "bg")).toBeGreaterThanOrEqual(3);
  expect(contrast(LIGHT, "ink", "panel")).toBeGreaterThanOrEqual(3);
});

it("gives dark text at least 7:1", () => {
  expect(contrast(DARK, "ink", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "ink", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "muted", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "muted", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "error", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "error", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "ok", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "ok", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "accent", "bg")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "accent", "panel")).toBeGreaterThanOrEqual(7);
  expect(contrast(DARK, "accent-ink", "accent")).toBeGreaterThanOrEqual(7);
});

it("gives dark lines and the focus ring at least 3:1", () => {
  expect(contrast(DARK, "line", "bg")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "line", "panel")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "ink", "bg")).toBeGreaterThanOrEqual(3);
  expect(contrast(DARK, "ink", "panel")).toBeGreaterThanOrEqual(3);
});
