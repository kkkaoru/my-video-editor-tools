// Runs with bun: Core Web Vitals must stay "good" (LCP <= 2.5 s, INP <= 200 ms, CLS <= 0.1) on a slowed-down CPU.
import { expect, type Page, test } from "@playwright/test";
import { UI_STATES, type UiState } from "./fixtures.ts";

interface Vitals {
  lcp: number;
  cls: number;
  inp: number;
}

interface EventObserverInit extends PerformanceObserverInit {
  durationThreshold: number;
}

const GOOD_LCP_MS: number = 2500;
const GOOD_INP_MS: number = 200;
const GOOD_CLS: number = 0.1;
// Four times slower approximates the slowest supported machines, as Lighthouse's mobile profile does.
const CPU_SLOWDOWN: number = 4;
const SETTLE_MS: number = 300;
const MEASURED_STATES: string[] = [
  "jobs empty",
  "job finished",
  "style face_above landscape",
  "style with profiles",
  "models idle",
  "help",
];

/** Runs in the page. Sums all shifts and keeps the slowest interaction, both stricter than the field definitions. */
function observeVitals(): void {
  const vitals: Vitals = { lcp: 0, cls: 0, inp: 0 };
  const eventOptions: EventObserverInit = { type: "event", buffered: true, durationThreshold: 16 };
  Object.assign(window, { __vitals: vitals });
  new PerformanceObserver((list: PerformanceObserverEntryList): void => {
    vitals.lcp = list.getEntries().at(-1)?.startTime ?? vitals.lcp;
  }).observe({ type: "largest-contentful-paint", buffered: true });
  new PerformanceObserver((list: PerformanceObserverEntryList): void => {
    list.getEntries().forEach((entry: PerformanceEntry): void => {
      const value: unknown = Reflect.get(entry, "value");
      vitals.cls += Reflect.get(entry, "hadRecentInput") === true || typeof value !== "number" ? 0 : value;
    });
  }).observe({ type: "layout-shift", buffered: true });
  new PerformanceObserver((list: PerformanceObserverEntryList): void => {
    vitals.inp = Math.max(
      vitals.inp,
      ...list.getEntries().map(({ duration }: PerformanceEntry): number => duration),
    );
  }).observe(eventOptions);
}

async function exercise(page: Page): Promise<void> {
  await page.getByRole("tab").nth(1).click();
  await page.locator('[role="group"][aria-label] button').first().click();
  await page.getByRole("spinbutton").first().fill("10");
  // The list with thousands of fonts: opening it and narrowing it must stay as quick as anything else.
  await page.getByRole("tabpanel").getByRole("combobox").nth(2).click();
  await page.getByRole("listbox").waitFor();
  await page.keyboard.type("Family 12");
  await page.keyboard.press("Escape");
  await page.getByRole("tab").nth(2).click();
  await page.getByRole("tab").nth(0).click();
}

MEASURED_STATES.forEach((name: string): void => {
  const state: UiState | undefined = UI_STATES.find((candidate: UiState): boolean => candidate.name === name);
  test(`core web vitals stay good: ${name}`, async ({ page }): Promise<void> => {
    await page.addInitScript(observeVitals);
    await page.addInitScript({ path: "tests/a11y/tauri-mock.js" });
    await page.addInitScript((snapshot: UiState["snapshot"] | undefined): void => {
      Object.assign(window, { __FIXTURE_SNAPSHOT__: snapshot });
    }, state?.snapshot);
    const session = await page.context().newCDPSession(page);
    await session.send("Emulation.setCPUThrottlingRate", { rate: CPU_SLOWDOWN });
    await page.goto("/");
    await page.getByRole("tablist").waitFor();
    await page.evaluate((events: UiState["jobEvents"] | undefined): void => {
      (events ?? []).forEach((event): void => {
        Reflect.get(window, "__emitTauri")("job", event);
      });
    }, state?.jobEvents);
    await page.waitForFunction((): boolean => Reflect.get(window, "__vitals").lcp > 0);
    await exercise(page);
    await page.waitForTimeout(SETTLE_MS);
    const { lcp, cls, inp }: Vitals = await page.evaluate((): Vitals => Reflect.get(window, "__vitals"));
    console.log(`${name}: LCP ${Math.round(lcp)} ms, INP ${Math.round(inp)} ms, CLS ${cls.toFixed(3)}`);
    expect(inp).toBeGreaterThan(0);
    expect(lcp).toBeLessThanOrEqual(GOOD_LCP_MS);
    expect(cls).toBeLessThanOrEqual(GOOD_CLS);
    expect(inp).toBeLessThanOrEqual(GOOD_INP_MS);
  });
});
