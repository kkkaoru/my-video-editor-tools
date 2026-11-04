// Runs with bun: Vite build and Vitest configuration for the Preact UI.
import preact from "@preact/preset-vite";
import tailwindcss from "@tailwindcss/vite";
import type { UserConfig } from "vite";
import { defineConfig } from "vitest/config";

const COVERAGE_MINIMUM: number = 95;

const config: UserConfig = defineConfig({
  plugins: [preact({ prefreshEnabled: false }), tailwindcss()],
  resolve: {
    alias: {
      react: "preact/compat",
      "react-dom": "preact/compat",
      "react/jsx-runtime": "preact/jsx-runtime",
    },
  },
  build: { outDir: "dist", emptyOutDir: true },
  test: {
    environment: "happy-dom",
    // The Preact alias only reaches inlined packages; contrast.test.ts reads the stylesheet as raw text.
    alias: [{ find: /^use-sync-external-store\/shim$/, replacement: "preact/compat" }],
    server: { deps: { inline: [/@base-ui/, /@floating-ui/, /@testing-library\/preact/] } },
    css: { include: [/\.css\?raw$/] },
    include: ["src/**/*.test.{ts,tsx}"],
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/**/*.test.{ts,tsx}", "src/backend/types.ts"],
      reporter: ["text", "json-summary"],
      thresholds: {
        perFile: true,
        statements: COVERAGE_MINIMUM,
        branches: COVERAGE_MINIMUM,
        functions: COVERAGE_MINIMUM,
        lines: COVERAGE_MINIMUM,
      },
    },
  },
});

export default config;
