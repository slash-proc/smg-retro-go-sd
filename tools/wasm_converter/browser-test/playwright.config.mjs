import { defineConfig } from "@playwright/test";

const reportDir = process.env.SMG_BROWSER_REPORT_DIR || "/reports";

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  workers: 1,
  timeout: 180_000,
  expect: { timeout: 30_000 },
  outputDir: `${reportDir}/artifacts`,
  reporter: [
    ["list"],
    ["json", { outputFile: `${reportDir}/results.json` }],
  ],
  use: {
    baseURL: process.env.SMG_BROWSER_URL || "http://127.0.0.1:8000",
    browserName: process.env.SMG_BROWSER || "chromium",
    headless: true,
    viewport: { width: 1280, height: 900 },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
});
