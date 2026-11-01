import { test, expect } from "@playwright/test";
import { performance } from "node:perf_hooks";

const EXPECTED_SHA1 = "a36105c9fbfff6041dc5babf4318c6a216ad470a";
const ISO_PATH = process.env.SMG_ISO_PATH || "/workspace/baserom_us.iso";
const ISO_BYTES = 4_699_979_776;

test("verify the supplied ISO in the selected browser at every WASM buffer size", async ({ page, browser }, testInfo) => {
  await page.goto("/");
  await page.locator("#iso").setInputFiles(ISO_PATH);
  await expect(page.locator("#status")).toContainText("bytes selected");

  const engine = browser.browserType().name();
  expect(engine).toBe(process.env.SMG_BROWSER || "chromium");
  const runs = [];
  for (const bufferMiB of [64, 128, 256]) {
    await page.locator("#buffer-size").selectOption(String(bufferMiB));
    const started = performance.now();
    await page.getByRole("button", { name: "Verify source image" }).click();
    await expect(page.locator("#status")).toHaveClass(/good/, { timeout: 150_000 });
    const elapsedMs = performance.now() - started;
    const result = (await page.locator("#status").textContent()).trim();
    expect(result).toContain(`SHA-1 ${EXPECTED_SHA1}`);

    const elapsedSeconds = elapsedMs / 1000;
    runs.push({
      browser: `${engine} ${browser.version()}`,
      bufferMiB,
      elapsedSeconds: Number(elapsedSeconds.toFixed(2)),
      throughputMiBps: Number((ISO_BYTES / 1048576 / elapsedSeconds).toFixed(1)),
      ranges: Math.ceil(ISO_BYTES / (bufferMiB * 1048576)),
      sha1: EXPECTED_SHA1,
    });
  }

  await testInfo.attach("chromium-buffer-timings.json", {
    body: JSON.stringify(runs, null, 2),
    contentType: "application/json",
  });
  console.log(`SMG_BROWSER_TIMINGS ${JSON.stringify(runs)}`);
});
