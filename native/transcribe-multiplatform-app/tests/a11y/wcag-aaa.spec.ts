// Runs with bun: drives every UI state through WCAG AAA checks in real browser engines.
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";
import { languageFor } from "../../src/i18n/language.ts";
import {
  COLOR_SCHEMES,
  INTERACTION_LOCALES,
  LOCALES,
  NARROW_VIEWPORT,
  UI_STATES,
  type UiState,
} from "./fixtures.ts";

interface Measurement {
  label: string;
  width: number;
  height: number;
}

interface FocusReport {
  label: string;
  outlineWidth: number;
  contrast: number;
}

const WCAG_TAGS: string[] = [
  "wcag2a",
  "wcag2aa",
  "wcag2aaa",
  "wcag21a",
  "wcag21aa",
  "wcag22aa",
  "best-practice",
];
const MINIMUM_TARGET_PX: number = 44;
const MINIMUM_FOCUS_WIDTH_PX: number = 2;
const MINIMUM_FOCUS_CONTRAST: number = 3;
const MINIMUM_LINE_HEIGHT_RATIO: number = 1.5;
const MAXIMUM_LINE_EM: number = 40;
const INTERACTIVE_SELECTOR: string =
  'button, [role="tab"], [role="slider"], [role="option"], [role="combobox"], input:not([type="hidden"]), select, a[href]';
const TAB_LIMIT: number = 120;
/** Base UI gives this hidden focus-trap edge role="button" only under WebKit, on purpose, for VoiceOver's cursor. */
const LIBRARY_FOCUS_GUARD: string = "[data-base-ui-focus-guard]";

/** In page order: the face and language selects, then the font list. */
const COMBOBOX_ORDER: string[] = ["open-face-select", "open-language-select", "open-font-list"];
/** Each profile row has overwrite then delete; with no profiles, the first trigger is the models reset. */
const DIALOG_BUTTONS: ReadonlyMap<string, number> = new Map([
  ["open-overwrite-dialog", 0],
  ["open-delete-dialog", 1],
  ["open-reset-dialog", 0],
]);
const DIALOG_TRIGGER: string = 'button[aria-haspopup="dialog"]';
const DROPPED_PATHS: ReadonlyMap<string, string[]> = new Map([
  ["drop-video", ["/a.mp4"]],
  ["drop-subtitles", ["/a.ttml"]],
  ["drop-other", ["/a.txt"]],
]);

async function showState(page: Page, state: UiState, locale: string): Promise<void> {
  await page.addInitScript({ path: "tests/a11y/tauri-mock.js" });
  await page.addInitScript(
    (snapshot: UiState["snapshot"]): void => {
      Object.assign(window, { __FIXTURE_SNAPSHOT__: snapshot });
    },
    { ...state.snapshot, locale },
  );
  await page.goto("/");
  await page.getByRole("tablist").waitFor();
  await page.evaluate(
    ({ jobEvents, downloadEvents }: Pick<UiState, "jobEvents" | "downloadEvents">): void => {
      const emit = Reflect.get(window, "__emitTauri");
      jobEvents.forEach((event): void => {
        emit("job", event);
      });
      downloadEvents.forEach((event): void => {
        emit("download", event);
      });
    },
    { jobEvents: state.jobEvents, downloadEvents: state.downloadEvents },
  );
  await page.getByRole("tab").nth(["jobs", "style", "models", "help"].indexOf(state.tab)).click();
  await applySteps(page, state, locale);
  await page.mouse.move(0, 0);
}

async function chooseToggle(page: Page, groupName: string, index: number): Promise<void> {
  await page.getByRole("group", { name: groupName, exact: true }).getByRole("button").nth(index).click();
}

async function applySteps(page: Page, state: UiState, locale: string): Promise<void> {
  const { messages } = languageFor(locale);
  if (state.anchor !== null) {
    await chooseToggle(
      page,
      messages.positionLabel,
      ["face_above", "face_below", "video_top", "video_bottom"].indexOf(state.anchor),
    );
  }
  if (state.frame !== null) {
    await chooseToggle(
      page,
      messages.previewFrameLabel,
      ["landscape", "vertical", "uhd"].indexOf(state.frame),
    );
  }
  await Promise.all(state.steps.map((step: string) => applyStep(page, step)));
}

async function applyStep(page: Page, step: string): Promise<void> {
  if (step === "hover-drop") {
    await page.evaluate((): void =>
      Reflect.get(window, "__emitTauri")("tauri://drag-enter", {
        paths: ["/a.mp4"],
        position: { x: 1, y: 1 },
      }),
    );
    return;
  }
  const dropped: string[] | undefined = DROPPED_PATHS.get(step);
  if (dropped !== undefined) {
    await page.evaluate(
      (paths: string[]): void =>
        Reflect.get(window, "__emitTauri")("tauri://drag-drop", { paths, position: { x: 1, y: 1 } }),
      dropped,
    );
    return;
  }
  // Kept-mounted panels stay in the DOM; only the open one may be exposed to assistive technology.
  await expect(page.getByRole("tabpanel")).toHaveCount(1);
  const panel = page.getByRole("tabpanel");
  const dialogButton: number | undefined = DIALOG_BUTTONS.get(step);
  if (dialogButton !== undefined) {
    await panel.locator(DIALOG_TRIGGER).nth(dialogButton).click();
    await page.getByRole("alertdialog").waitFor();
    return;
  }
  if (step === "open-speaker-help") {
    await panel.locator(DIALOG_TRIGGER).click();
    await page.getByRole("dialog").waitFor();
    return;
  }
  const comboboxIndex: number = COMBOBOX_ORDER.indexOf(step);
  await panel.getByRole("combobox").nth(comboboxIndex).click();
  await page.getByRole("listbox").waitFor();
}

const smallTargets = (page: Page): Promise<Measurement[]> =>
  page.$$eval(
    INTERACTIVE_SELECTOR,
    (elements: Element[], minimum: number): Measurement[] =>
      elements
        // Base UI's visually hidden range input sits inside the 44 px thumb, which is the real target.
        .map(
          (element: Element): Element =>
            element.closest("[data-base-ui-slider-thumb], [data-index]") ?? element,
        )
        .map((element: Element): Measurement => {
          const { width, height } = element.getBoundingClientRect();
          return { label: element.outerHTML.slice(0, 120), width, height };
        })
        .filter(
          ({ width, height }: Measurement): boolean =>
            width > 1 && height > 1 && (width < minimum || height < minimum),
        ),
    MINIMUM_TARGET_PX,
  );

const unreadableText = (page: Page): Promise<string[]> =>
  page.$$eval(
    "p, li",
    (elements: Element[], ratio: number): string[] =>
      elements
        .filter((element: Element): boolean => {
          const style: CSSStyleDeclaration = getComputedStyle(element);
          return (
            Number.parseFloat(style.lineHeight) / Number.parseFloat(style.fontSize) < ratio ||
            style.textAlign === "justify"
          );
        })
        .map((element: Element): string => element.outerHTML.slice(0, 120)),
    MINIMUM_LINE_HEIGHT_RATIO,
  );

const focusReport = (page: Page): Promise<FocusReport> =>
  page.evaluate((): FocusReport => {
    const parse = (color: string): number[] => (color.match(/[\d.]+/g) ?? []).map(Number);
    const luminance = ([red, green, blue]: number[]): number => {
      const linear = (channel: number | undefined): number => {
        const value: number = (channel ?? 0) / 255;
        return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue);
    };
    const opaqueBackground = (start: Element | null): string => {
      const color: string = start === null ? "rgb(255,255,255)" : getComputedStyle(start).backgroundColor;
      const transparent: boolean =
        color === "rgba(0, 0, 0, 0)" || color === "transparent" || color.endsWith(" / 0)");
      return transparent && start !== null ? opaqueBackground(start.parentElement) : color;
    };
    const withOutline = (start: Element | null): Element | null => {
      if (start === null) {
        return null;
      }
      return getComputedStyle(start).outlineStyle === "none" ? withOutline(start.parentElement) : start;
    };
    const active: Element | null = document.activeElement;
    const ringOwner: Element | null = withOutline(active);
    if (ringOwner === null) {
      return { label: active?.outerHTML.slice(0, 120) ?? "nothing", outlineWidth: 0, contrast: 0 };
    }
    const style: CSSStyleDeclaration = getComputedStyle(ringOwner);
    const first: number = luminance(parse(style.outlineColor));
    const second: number = luminance(parse(opaqueBackground(ringOwner.parentElement)));
    return {
      label: active?.outerHTML.slice(0, 120) ?? "nothing",
      outlineWidth: Number.parseFloat(style.outlineWidth),
      contrast: (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05),
    };
  });

/** Tabs through the page until focus returns to the start, returning a report per stop. */
async function tabStops(page: Page, seen: FocusReport[], remaining: number): Promise<FocusReport[]> {
  await page.keyboard.press("Tab");
  const report: FocusReport = await focusReport(page);
  const finished: boolean = remaining === 0 || report.label.startsWith("<body") || report.label === "nothing";
  return finished ? seen : tabStops(page, [...seen, report], remaining - 1);
}

const failingFocus = (reports: FocusReport[]): FocusReport[] =>
  reports.filter(
    ({ outlineWidth, contrast }: FocusReport): boolean =>
      outlineWidth < MINIMUM_FOCUS_WIDTH_PX || contrast < MINIMUM_FOCUS_CONTRAST,
  );

async function hoverViolations(page: Page): Promise<string[]> {
  // Behind an open alert dialog the page is inert: nothing there can be hovered, or used at all.
  const dialog = page.getByRole("alertdialog");
  const scope = (await dialog.count()) > 0 ? dialog : page.locator("body");
  const buttons = await scope
    .locator('button:visible, [role="tab"]:visible, [role="combobox"]:visible')
    .all();
  const found: string[][] = [];
  await buttons.reduce(async (previous: Promise<void>, button): Promise<void> => {
    await previous;
    await button.hover();
    const { violations } = await new AxeBuilder({ page }).withRules(["color-contrast-enhanced"]).analyze();
    found.push(violations.flatMap((violation) => violation.nodes.map((node) => node.html.slice(0, 120))));
  }, Promise.resolve());
  return found.flat();
}

const contrastUndecided = (incomplete: { id: string; nodes: { html: string }[] }[]): string[] =>
  incomplete
    .filter(({ id }: { id: string }): boolean => id === "color-contrast-enhanced")
    .flatMap(({ nodes }: { nodes: { html: string }[] }): string[] =>
      nodes.map(({ html }: { html: string }): string => html.slice(0, 160)),
    );

test("the sweep detects a low-contrast text, a small target and a missing focus ring", async ({
  page,
}): Promise<void> => {
  const state: UiState | undefined = UI_STATES.find(({ name }: UiState): boolean => name === "jobs empty");
  if (state === undefined) {
    throw new Error("the jobs empty state is missing");
  }
  await showState(page, state, "en");
  await page.evaluate((): void => {
    const host: Element | null = document.querySelector("main");
    const faint: HTMLElement = document.createElement("p");
    faint.textContent = "Barely readable";
    faint.style.color = "#767676";
    faint.style.background = "#ffffff";
    const tiny: HTMLElement = document.createElement("button");
    tiny.textContent = "x";
    tiny.style.cssText = "width:20px;height:20px;padding:0;min-width:0;min-height:0;outline:none !important";
    host?.append(faint, tiny);
    tiny.focus();
  });
  const axe = await new AxeBuilder({ page }).exclude(LIBRARY_FOCUS_GUARD).withTags(WCAG_TAGS).analyze();
  expect(
    axe.violations.map(({ id }) => id).filter((id: string): boolean => id === "color-contrast-enhanced"),
  ).toStrictEqual(["color-contrast-enhanced"]);
  expect((await smallTargets(page)).length).toBe(1);
  expect((await focusReport(page)).outlineWidth).toBe(0);
});

test("the speaker count help does not cover the control it explains", async ({ page }): Promise<void> => {
  const state: UiState | undefined = UI_STATES.find(({ name }: UiState): boolean => name === "jobs empty");
  if (state === undefined) {
    throw new Error("the jobs empty state is missing");
  }
  await showState(page, state, "en");
  const panel = page.getByRole("tabpanel");
  await panel.locator(DIALOG_TRIGGER).hover();
  await page.getByRole("dialog").waitFor();
  await panel.getByRole("combobox").first().hover({ timeout: 2000 });
});

const brokenSpeakerNames = (page: Page): Promise<string[]> =>
  page.$$eval('input[type="color"] ~ span', (elements: Element[]): string[] =>
    elements
      .filter((element: Element): boolean => {
        const { fontSize } = getComputedStyle(element);
        return element.getClientRects().length > 1 || element.clientHeight > 2 * Number.parseFloat(fontSize);
      })
      .map((element: Element): string => element.textContent ?? ""),
  );

/** WCAG 1.4.8: at most 80 Latin characters a line, or 40 CJK; 40 em holds either. */
const overlongLines = (page: Page): Promise<string[]> =>
  page
    .getByRole("tabpanel")
    .locator("p, li")
    .evaluateAll(
      (elements: Element[], limit: number): string[] =>
        elements
          .filter(
            (element: Element): boolean =>
              element.getBoundingClientRect().width / Number.parseFloat(getComputedStyle(element).fontSize) >
              limit,
          )
          .map((element: Element): string => element.textContent ?? ""),
      MAXIMUM_LINE_EM,
    );

LOCALES.forEach((locale: string): void => {
  test(`help text keeps a readable line length, and the version stays in its tab / ${locale}`, async ({
    page,
  }): Promise<void> => {
    const state: UiState | undefined = UI_STATES.find(({ name }: UiState): boolean => name === "help");
    if (state === undefined) {
      throw new Error("the help state is missing");
    }
    await showState(page, state, locale);
    expect(await overlongLines(page)).toStrictEqual([]);
    const version = page.getByRole("tabpanel").locator("p").first();
    await expect(version).toHaveText(
      languageFor(locale).messages.help.version.replace("{version}", state.snapshot.version),
    );
    await page.getByRole("tab").nth(0).click();
    await expect(page.getByText(state.snapshot.version)).toBeHidden();
  });
});

LOCALES.forEach((locale: string): void => {
  test(`speaker names stay on one line / ${locale}`, async ({ page }): Promise<void> => {
    const state: UiState | undefined = UI_STATES.find(
      ({ name }: UiState): boolean => name === "style with profiles",
    );
    if (state === undefined) {
      throw new Error("the profile state is missing");
    }
    await showState(page, state, locale);
    expect(await brokenSpeakerNames(page)).toStrictEqual([]);
    await page.setViewportSize(NARROW_VIEWPORT);
    expect(await brokenSpeakerNames(page)).toStrictEqual([]);
  });
});

COLOR_SCHEMES.forEach((colorScheme: "light" | "dark"): void => {
  LOCALES.forEach((locale: string): void => {
    UI_STATES.forEach((state: UiState): void => {
      test(`${state.name} / ${locale} / ${colorScheme}`, async ({ page }): Promise<void> => {
        await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
        await showState(page, state, locale);
        const axe = await new AxeBuilder({ page }).exclude(LIBRARY_FOCUS_GUARD).withTags(WCAG_TAGS).analyze();
        expect(
          axe.violations.map(
            ({ id, nodes }) => `${id}: ${nodes.map((node) => node.html.slice(0, 160)).join(" | ")}`,
          ),
        ).toStrictEqual([]);
        expect(contrastUndecided(axe.incomplete)).toStrictEqual([]);
        expect(await smallTargets(page)).toStrictEqual([]);
        expect(await unreadableText(page)).toStrictEqual([]);
        expect(await page.evaluate((): number => document.getAnimations().length)).toBe(0);
        if (INTERACTION_LOCALES.has(locale)) {
          expect(failingFocus(await tabStops(page, [], TAB_LIMIT))).toStrictEqual([]);
          expect(await hoverViolations(page)).toStrictEqual([]);
        }
        await page.setViewportSize(NARROW_VIEWPORT);
        // An open popup is repositioned just after the resize.
        await expect
          .poll(() => page.evaluate((): boolean => document.documentElement.scrollWidth <= window.innerWidth))
          .toBe(true);
      });
    });
  });
});
