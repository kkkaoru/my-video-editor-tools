// Runs with bun: Core Web Vitals must stay "good" (LCP <= 2.5 s, INP <= 200 ms, CLS <= 0.1) on a slowed-down CPU.
import { readFile } from "node:fs/promises";
import { expect, type Page, test } from "@playwright/test";
import { UI_STATES, type UiState } from "./fixtures.ts";

interface Vitals {
  LCP: number;
  CLS: number;
  INP: number;
}

const GOOD_LCP_MS: number = 2500;
const GOOD_INP_MS: number = 200;
const GOOD_CLS: number = 0.1;
// Four times slower approximates the slowest supported machines, as Lighthouse's mobile profile does.
const CPU_SLOWDOWN: number = 4;
const WEB_VITALS_BUNDLE: string = "node_modules/web-vitals/dist/web-vitals.iife.js";
const HEAVIEST_STATES: ReadonlySet<string> = new Set([
  "jobs empty",
  "job finished",
  "style face_above landscape",
  "models idle",
]);

async function collectVitals(page: Page): Promise<void> {
  await page.addInitScript({ content: await readFile(WEB_VITALS_BUNDLE, "utf8") });
  await page.addInitScript((): void => {
    const vitals: Record<string, number> = { LCP: 0, CLS: 0, INP: 0 };
    const library = Reflect.get(window, "webVitals");
    const record = ({ name, value }: { name: string; value: number }): void => {
      vitals[name] = value;
    };
    Object.assign(window, { __vitals: vitals });
    library.onLCP(record, { reportAllChanges: true });
    library.onCLS(record, { reportAllChanges: true });
    library.onINP(record, { reportAllChanges: true, durationThreshold: 16 });
  });
}

const readVitals = (page: Page): Promise<Vitals> =>
  page.evaluate((): Vitals => {
    document.dispatchEvent(new Event("visibilitychange"));
    return Reflect.get(window, "__vitals");
  });

async function exercise(page: Page): Promise<void> {
  await page.getByRole("tab").nth(1).click();
  await page.getByRole("group").nth(0).getByRole("button").nth(0).click();
  await page.getByRole("spinbutton").nth(0).fill("10");
  await page.getByRole("tab").nth(2).click();
  await page.getByRole("tab").nth(0).click();
}

HEAVIEST_STATES.forEach((name: string): void => {
  const state: UiState | undefined = UI_STATES.find((candidate: UiState): boolean => candidate.name === name);
  test(`core web vitals stay good: ${name}`, async ({ page, browserName }): Promise<void> => {
    test.skip(browserName !== "chromium", "LCP and INP are only exposed by Chromium's performance APIs");
    expect(state).not.toBe(undefined);
    await collectVitals(page);
    await page.addInitScript({ path: "tests/a11y/tauri-mock.js" });
    await page.addInitScript((snapshot: UiState["snapshot"] | undefined): void => {
      Object.assign(window, { __FIXTURE_SNAPSHOT__: snapshot });
    }, state?.snapshot);
    const session = await page.context().newCDPSession(page);
    await session.send("Emulation.setCPUThrottlingRate", { rate: CPU_SLOWDOWN });
    await page.goto("/");
    await page.getByRole("tablist").waitFor();
    await page.evaluate((events: UiState["jobEvents"] | undefined): void => {
      (events ?? []).map((event) => Reflect.get(window, "__emitTauri")("job", event));
    }, state?.jobEvents);
    await exercise(page);
    const { LCP, CLS, INP }: Vitals = await readVitals(page);
    expect(LCP).toBeGreaterThan(0);
    expect(LCP).toBeLessThanOrEqual(GOOD_LCP_MS);
    expect(CLS).toBeLessThanOrEqual(GOOD_CLS);
    expect(INP).toBeLessThanOrEqual(GOOD_INP_MS);
  });
});
