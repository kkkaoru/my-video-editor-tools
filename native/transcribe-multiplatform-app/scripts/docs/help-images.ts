// Runs with bun: draws the help page's annotated pictures from the real UI, for every language; they ship in public/help.
import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { chromium, type Locator, type Page } from "@playwright/test";
import type { DownloadEvent, JobEvent, ModelStatus, Snapshot } from "../../src/backend/types.ts";
import { type Language, languageFor } from "../../src/i18n/language.ts";
import type { Messages } from "../../src/i18n/messages.ts";
import { HELP_IMAGE_SIZES, type HelpImageName } from "../../src/view/help-images.ts";
import { ALL_MODELS, snapshot, WITH_PROFILES } from "../../tests/a11y/fixtures.ts";

interface Mark {
  /** The step number in the help text. */
  step: number;
  target: (page: Page, messages: Messages) => Locator;
}

interface Scene {
  name: HelpImageName;
  tab: number;
  snapshot: Snapshot;
  jobEvents: JobEvent[];
  downloadEvents: DownloadEvent[];
  prepare: (page: Page, messages: Messages) => Promise<void>;
  marks: Mark[];
}

/** Every language the app ships, not only those the accessibility sweep samples. */
const LOCALES: string[] = ["en", "ja", "zh-CN", "zh-TW", "ko", "de", "fr", "es"];
const PORT: number = 4193;
const SERVER_START_MS: number = 1500;
const BASE_URL: string = `http://127.0.0.1:${PORT}`;
const WORK_DIR: string = "node_modules/.cache/help-images";
const WEBP_QUALITY: string = "72";
const BADGE_PX: number = 30;
const FONT_SCENE_ROOM_ABOVE_PX: number = 60;
const VIDEO_PATH: string = "/Videos/talk.mp4";
const TAB_JOBS: number = 0;
const TAB_STYLE: number = 1;
const TAB_MODELS: number = 2;

const nothing = (): Promise<void> => Promise.resolve();

const tab =
  (index: number): Mark["target"] =>
  (page: Page): Locator =>
    page.getByRole("tab").nth(index);

const panel = (page: Page): Locator => page.getByRole("tabpanel");

const missingWhisper: ModelStatus[] = ALL_MODELS.map(
  (row: ModelStatus): ModelStatus => (row.role === "transcribe" ? { ...row, found_at: null } : row),
);

const SCENES: Scene[] = [
  {
    name: "models",
    tab: TAB_MODELS,
    snapshot: snapshot({ models: missingWhisper }),
    jobEvents: [],
    downloadEvents: [
      {
        state: "running",
        id: "nemotron-3.5-transcribe-iq4",
        done_bytes: 800_000_000,
        total_bytes: 1_700_000_000,
      },
    ],
    prepare: nothing,
    marks: [
      { step: 1, target: tab(TAB_MODELS) },
      {
        step: 2,
        target: (page: Page, { downloadAllLabel }: Messages): Locator =>
          panel(page).getByRole("button", { name: downloadAllLabel }),
      },
      { step: 3, target: (page: Page): Locator => panel(page).getByRole("progressbar").first() },
    ],
  },
  {
    name: "transcribe",
    tab: TAB_JOBS,
    snapshot: snapshot({}),
    jobEvents: [],
    downloadEvents: [],
    prepare: nothing,
    marks: [
      { step: 1, target: tab(TAB_JOBS) },
      {
        step: 2,
        target: (page: Page, { speakerCount }: Messages): Locator =>
          panel(page).getByRole("combobox", { name: speakerCount }),
      },
      {
        step: 3,
        target: (page: Page, { dropTitle }: Messages): Locator =>
          panel(page).getByRole("region", { name: dropTitle }),
      },
    ],
  },
  {
    name: "look",
    tab: TAB_STYLE,
    snapshot: snapshot({
      settings: {
        ...snapshot({}).settings,
        palette: snapshot({}).settings.palette.slice(0, 2),
        placement: { anchor: "face_above", offset_x_percent: 0, offset_y_percent: 0 },
      },
    }),
    jobEvents: [],
    downloadEvents: [],
    prepare: nothing,
    marks: [
      { step: 1, target: tab(TAB_STYLE) },
      {
        step: 2,
        target: (page: Page, { positionLabel }: Messages): Locator =>
          panel(page).getByRole("group", { name: positionLabel, exact: true }),
      },
      {
        step: 3,
        target: (page: Page): Locator => panel(page).locator('[data-slot="slider"]').first(),
      },
      {
        step: 4,
        target: (page: Page): Locator => panel(page).locator('input[type="color"]').first().locator(".."),
      },
    ],
  },
  {
    name: "font",
    tab: TAB_STYLE,
    snapshot: snapshot({}),
    jobEvents: [],
    downloadEvents: [],
    prepare: async (page: Page, { fontFamily }: Messages): Promise<void> => {
      const trigger: Locator = panel(page).getByRole("combobox", { name: fontFamily });
      await trigger.evaluate((element: Element, room: number): void => {
        element.scrollIntoView({ block: "start" });
        window.scrollBy(0, -room);
      }, FONT_SCENE_ROOM_ABOVE_PX);
      await trigger.click();
      await page.getByRole("option").first().waitFor();
    },
    marks: [
      {
        step: 1,
        target: (page: Page, { fontFamily }: Messages): Locator =>
          panel(page).getByRole("combobox", { name: fontFamily }),
      },
      {
        step: 2,
        target: (page: Page, { fontSearch }: Messages): Locator =>
          page.getByRole("combobox", { name: fontSearch }),
      },
      { step: 3, target: (page: Page): Locator => page.getByRole("option").nth(1) },
    ],
  },
  {
    name: "profiles",
    tab: TAB_STYLE,
    snapshot: WITH_PROFILES,
    jobEvents: [],
    downloadEvents: [],
    prepare: nothing,
    marks: [
      {
        step: 1,
        target: (page: Page, { profileName }: Messages): Locator =>
          panel(page).getByRole("textbox", { name: profileName }),
      },
      {
        step: 2,
        target: (page: Page, { profileSave }: Messages): Locator =>
          panel(page).getByRole("button", { name: profileSave }),
      },
      {
        step: 3,
        target: (page: Page): Locator =>
          panel(page).getByRole("listitem").first().getByRole("button").first(),
      },
    ],
  },
  {
    name: "rewrite",
    tab: TAB_JOBS,
    snapshot: WITH_PROFILES,
    jobEvents: [
      { state: "queued", id: 1, path: `${VIDEO_PATH.replace(".mp4", "")}.ttml` },
      {
        state: "finished",
        id: 1,
        outcome: {
          outputs: [
            "/Videos/talk-2.ttml",
            "/Videos/talk-2.srt",
            "/Videos/talk-2.fcpxml",
            "/Videos/talk-2.transcript.json",
          ],
          base: "/Videos/talk-2",
          speaker_count: 2,
          requested_speakers: 0,
          cue_count: 120,
          faceless: false,
        },
      },
    ],
    downloadEvents: [],
    prepare: nothing,
    marks: [
      { step: 2, target: tab(TAB_JOBS) },
      {
        step: 3,
        target: (page: Page, { dropTitle }: Messages): Locator =>
          panel(page).getByRole("region", { name: dropTitle }),
      },
      {
        step: 4,
        target: (page: Page): Locator =>
          panel(page).getByRole("listitem").first().locator("[data-slot=card]"),
      },
    ],
  },
];

/** Runs in the page: draws a frame around the target and a number outside it, where it covers no text or control. Returns what stood in the way. */
function drawMark(target: Element, { step, badge }: { step: number; badge: number }): string {
  const box: DOMRect = target.getBoundingClientRect();
  const frameGap: number = 6;
  const seen = (rect: { width: number; height: number }): boolean => rect.width > 2 && rect.height > 2;
  const textBoxes: DOMRect[] = [];
  const walker: TreeWalker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    const range: Range = document.createRange();
    range.selectNodeContents(walker.currentNode);
    textBoxes.push(...Array.from(range.getClientRects()).filter(seen));
  }
  const controls: DOMRect[] = Array.from(
    document.querySelectorAll(
      'button, input:not([type="range"]), [role="combobox"], [role="tab"], [data-slot="slider"]',
    ),
  )
    .map((element: Element): DOMRect => element.getBoundingClientRect())
    .filter(seen);
  const marks: DOMRect[] = Array.from(document.querySelectorAll("[data-help-mark]")).map(
    (element: Element): DOMRect => element.getBoundingClientRect(),
  );
  const obstacles: DOMRect[] = [...textBoxes, ...controls, ...marks];
  const free = (left: number, top: number): boolean =>
    left >= 0 &&
    top >= 0 &&
    left + badge <= window.innerWidth &&
    top + badge <= window.innerHeight &&
    !obstacles.some(
      (other: DOMRect): boolean =>
        left < other.right && left + badge > other.left && top < other.bottom && top + badge > other.top,
    );
  const middle: number = box.top + box.height / 2 - badge / 2;
  const candidates: [number, number][] = [
    [box.left - frameGap - badge, middle],
    [box.right + frameGap, middle],
    [box.left, box.top - frameGap - badge],
    [box.right - badge, box.top - frameGap - badge],
    [box.left, box.bottom + frameGap],
    [box.right - badge, box.bottom + frameGap],
    [box.left - frameGap - badge, box.top],
    [box.right + frameGap, box.top],
  ];
  const place: [number, number] | undefined = candidates.find(([left, top]: [number, number]): boolean =>
    free(left, top),
  );
  if (place === undefined) {
    return `no free place for mark ${step}`;
  }
  const frame: HTMLElement = document.createElement("div");
  frame.style.cssText = `position:fixed;z-index:9999;left:${box.left - 3}px;top:${box.top - 3}px;width:${box.width + 6}px;height:${box.height + 6}px;border:3px solid #d0021b;outline:2px solid #fff;border-radius:8px;pointer-events:none;box-sizing:border-box`;
  const number: HTMLElement = document.createElement("div");
  number.dataset.helpMark = String(step);
  number.textContent = String(step);
  number.style.cssText = `position:fixed;z-index:10000;left:${place[0]}px;top:${place[1]}px;width:${badge}px;height:${badge}px;border-radius:50%;background:#d0021b;color:#fff;border:2px solid #fff;font:700 16px/${badge - 4}px sans-serif;text-align:center;box-sizing:border-box`;
  document.body.append(frame, number);
  return "";
}

/** Runs in the page: names any text a frame's outline crosses without holding it, or any number over text. */
function overlapsOnPicture(): string[] {
  const seen = (rect: DOMRect): boolean => rect.width > 2 && rect.height > 2;
  const walker: TreeWalker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  const texts: { text: string; rect: DOMRect }[] = [];
  while (walker.nextNode()) {
    const node: Node = walker.currentNode;
    const range: Range = document.createRange();
    range.selectNodeContents(node);
    if (node.parentElement?.closest("[data-help-mark]") === null) {
      Array.from(range.getClientRects())
        .filter(seen)
        .forEach((rect: DOMRect): void => {
          texts.push({ text: node.textContent ?? "", rect });
        });
    }
  }
  return Array.from(document.querySelectorAll("[data-help-mark]")).flatMap((mark: Element): string[] => {
    const box: DOMRect = mark.getBoundingClientRect();
    return texts
      .filter(
        ({ rect }: { rect: DOMRect }): boolean =>
          box.left < rect.right && box.right > rect.left && box.top < rect.bottom && box.bottom > rect.top,
      )
      .map(({ text }: { text: string }): string => `mark ${mark.textContent} covers “${text.slice(0, 30)}”`);
  });
}

async function showScene(page: Page, scene: Scene, locale: string): Promise<Messages> {
  const { messages }: Language = languageFor(locale);
  await page.addInitScript({ path: "tests/a11y/tauri-mock.js" });
  await page.addInitScript(
    (fixture: Snapshot): void => {
      Object.assign(window, { __FIXTURE_SNAPSHOT__: fixture });
    },
    { ...scene.snapshot, locale },
  );
  await page.goto(BASE_URL);
  await page.getByRole("tablist").waitFor();
  await page.evaluate(
    ({ jobEvents, downloadEvents }: Pick<Scene, "jobEvents" | "downloadEvents">): void => {
      const emit = Reflect.get(window, "__emitTauri");
      jobEvents.forEach((event: JobEvent): void => {
        emit("job", event);
      });
      downloadEvents.forEach((event: DownloadEvent): void => {
        emit("download", event);
      });
    },
    { jobEvents: scene.jobEvents, downloadEvents: scene.downloadEvents },
  );
  await page.getByRole("tab").nth(scene.tab).click();
  await scene.prepare(page, messages);
  await page.mouse.move(0, 0);
  return messages;
}

/** A picture that cuts off a numbered target, or covers words with a number, would mislead, so either stops the run. */
async function drawMarks(page: Page, scene: Scene, locale: string, messages: Messages): Promise<void> {
  const { height } = HELP_IMAGE_SIZES[scene.name];
  await scene.marks.reduce(async (previous: Promise<void>, { step, target }: Mark): Promise<void> => {
    await previous;
    const element: Locator = target(page, messages);
    const bottom: number = await element.evaluate(
      (node: Element): number => node.getBoundingClientRect().bottom,
    );
    if (bottom > height) {
      throw new Error(
        `${scene.name}/${locale}: mark ${step} ends at ${bottom}, below the ${height} px picture`,
      );
    }
    const problem: string = await element.evaluate(drawMark, { step, badge: BADGE_PX });
    if (problem !== "") {
      throw new Error(`${scene.name}/${locale}: ${problem}`);
    }
  }, Promise.resolve());
  const overlaps: string[] = await page.evaluate(overlapsOnPicture);
  if (overlaps.length > 0) {
    throw new Error(`${scene.name}/${locale}: ${overlaps.join("; ")}`);
  }
}

function toWebp(png: string, webp: string): void {
  const { status, stderr } = spawnSync("cwebp", ["-quiet", "-q", WEBP_QUALITY, png, "-o", webp]);
  if (status !== 0) {
    throw new Error(`cwebp failed for ${png}: ${String(stderr)}`);
  }
}

async function shootScene(page: Page, scene: Scene, locale: string): Promise<void> {
  const messages: Messages = await showScene(page, scene, locale);
  await drawMarks(page, scene, locale, messages);
  const folder: string = `public/help/${messages.help.imageFolder}`;
  const png: string = `${WORK_DIR}/${messages.help.imageFolder}-${scene.name}.png`;
  mkdirSync(folder, { recursive: true });
  await page.screenshot({ path: png });
  toWebp(png, `${folder}/${scene.name}.webp`);
}

async function main(): Promise<void> {
  mkdirSync(WORK_DIR, { recursive: true });
  const build = spawnSync("bunx", ["vite", "build", "--outDir", `${WORK_DIR}/dist`], { stdio: "inherit" });
  if (build.status !== 0) {
    throw new Error("the build failed");
  }
  const server = spawn("bunx", [
    "vite",
    "preview",
    "--outDir",
    `${WORK_DIR}/dist`,
    "--host",
    "127.0.0.1",
    "--port",
    String(PORT),
    "--strictPort",
  ]);
  try {
    await sleep(SERVER_START_MS);
    const browser = await chromium.launch();
    // One page at a time: each is measured, and a full machine would make pictures differ.
    await LOCALES.flatMap((locale: string): [string, Scene][] =>
      SCENES.map((scene: Scene): [string, Scene] => [locale, scene]),
    ).reduce(async (previous: Promise<void>, [locale, scene]: [string, Scene]): Promise<void> => {
      await previous;
      const page: Page = await browser.newPage({
        viewport: HELP_IMAGE_SIZES[scene.name],
        colorScheme: "light",
        reducedMotion: "reduce",
      });
      await shootScene(page, scene, locale);
      await page.close();
    }, Promise.resolve());
    await browser.close();
  } finally {
    server.kill();
    rmSync(WORK_DIR, { recursive: true, force: true });
  }
}

await main();
