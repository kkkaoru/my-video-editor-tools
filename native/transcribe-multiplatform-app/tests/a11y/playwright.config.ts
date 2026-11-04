// Runs with bun: the WCAG AAA state sweep runs in the engines behind Tauri's webviews.
import { defineConfig, devices, type PlaywrightTestConfig } from "@playwright/test";

const PREVIEW_PORT: number = 4173;
const PREVIEW_URL: string = `http://127.0.0.1:${PREVIEW_PORT}`;
const WORKERS: number = 4;

const config: PlaywrightTestConfig = defineConfig({
  testDir: ".",
  outputDir: "../../node_modules/.cache/playwright-results",
  fullyParallel: true,
  workers: WORKERS,
  reporter: [["line"]],
  use: { baseURL: PREVIEW_URL },
  projects: [
    { name: "webkit", use: { ...devices["Desktop Safari"] } },
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],
  webServer: {
    // Its own folder: the app rebuild empties dist/ while a long sweep is still loading pages.
    command: `bunx vite build --outDir node_modules/.cache/a11y-dist && bunx vite preview --outDir node_modules/.cache/a11y-dist --host 127.0.0.1 --port ${PREVIEW_PORT} --strictPort`,
    cwd: "../..",
    url: PREVIEW_URL,
    reuseExistingServer: true,
  },
});

export default config;
