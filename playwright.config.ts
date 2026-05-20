import { defineConfig, devices } from "@playwright/test";
import { join } from "node:path";

const outputDir = join(
  process.env.TEMP || process.cwd(),
  "meeting-minutes-playwright-results",
);

export default defineConfig({
  testDir: "./tests/e2e",
  outputDir,
  timeout: 30_000,
  workers: 1,
  expect: {
    timeout: 8_000,
  },
  use: {
    baseURL: "http://127.0.0.1:1420",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "npm run dev -- --host 127.0.0.1",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
  projects: [
    {
      name: "chromium-desktop",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1366, height: 768 } },
    },
    {
      name: "chromium-mobile",
      use: { ...devices["Pixel 5"], viewport: { width: 393, height: 851 } },
    },
  ],
});
