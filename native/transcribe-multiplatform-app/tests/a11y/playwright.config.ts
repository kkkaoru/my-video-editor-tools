// Runs with bun: the WCAG AAA state sweep runs in the engines behind Tauri's webviews.
import { defineConfig, devices, type PlaywrightTestConfig } from "@playwright/test";

const PREVIEW_PORT: number = 4173;
const PREVIEW_URL: string = `http://127.0.0.1:${PREVIEW_PORT}`;
// A CI runner has 3 slow cores: fewer workers and more time, the same checks and thresholds.
const ON_CI: boolean = process.env.CI !== undefined;
const WORKERS: number = ON_CI ? 2 : 4;
const VITALS_SPEC: RegExp = /web-vitals\.spec\.ts/;
const TEST_TIMEOUT_MS: number = ON_CI ? 120_000 : 30_000;

const config: PlaywrightTestConfig = defineConfig({
  testDir: ".",
  outputDir: "../../node_modules/.cache/playwright-results",
  fullyParallel: true,
  workers: WORKERS,
  timeout: TEST_TIMEOUT_MS,
  retries: ON_CI ? 1 : 0,
  reporter: [["line"]],
  use: { baseURL: PREVIEW_URL },
  projects: [
    { name: "webkit", testIgnore: VITALS_SPEC, use: { ...devices["Desktop Safari"] } },
    { name: "chromium", testIgnore: VITALS_SPEC, use: { ...devices["Desktop Chrome"] } },
    // Timings mean nothing beside other work, so `test:vitals` runs this alone, one page at a time.
    { name: "vitals", testMatch: VITALS_SPEC, workers: 1, use: { ...devices["Desktop Chrome"] } },
  ],
  webServer: {
    // Its own folder: the app rebuild empties dist/ while a long sweep is still loading pages.
    command: `bunx vite build --outDir node_modules/.cache/a11y-dist && bunx vite preview --outDir node_modules/.cache/a11y-dist --host 127.0.0.1 --port ${PREVIEW_PORT} --strictPort`,
    cwd: "../..",
    url: PREVIEW_URL,
    // Never reuse: a preview left running serves an old build, and the sweep would pass or fail on old code.
    reuseExistingServer: false,
  },
});

export default config;
