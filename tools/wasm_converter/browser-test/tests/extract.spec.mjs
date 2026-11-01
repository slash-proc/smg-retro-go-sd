import { test, expect } from "@playwright/test";
import { performance } from "node:perf_hooks";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const ISO_PATH = process.env.SMG_ISO_PATH || "/workspace/baserom_us.iso";
const SOURCE_BYTES = 261_599_168;
const WINDOWS = (process.env.SMG_EXTRACT_WINDOWS || "8").split(",").map(Number);
const NATIVE_XIP = readFileSync("/workspace/retro-go-sd/smg_assets.xip");
const NATIVE_XIP_SHA256 = createHash("sha256").update(NATIVE_XIP).digest("hex");

test("convert the verified USA ISO into the native linked asset XIP", async ({ page, browser }) => {
  test.setTimeout(900_000);
  const engine = browser.browserType().name();
  expect(engine).toBe(process.env.SMG_BROWSER || "chromium");
  await page.goto("/extract.html");
  await page.locator("#iso").setInputFiles(ISO_PATH);
  await expect(page.locator("#status")).toContainText("bytes selected");

  for (const bufferMiB of WINDOWS) {
    expect([8, 64, 128, 256]).toContain(bufferMiB);
    await page.locator("#buffer-size").selectOption(String(bufferMiB));
    const started = performance.now();
    await page.getByRole("button", { name: "Convert ISO to linked asset XIP" }).click();
    const deadline = Date.now() + 840_000;
    let lastReport = 0;
    for (;;) {
      const status = page.locator("#status");
      const state = await status.getAttribute("class");
      const result = (await status.textContent()).trim();
      if (state.includes("good")) break;
      if (state.includes("bad")) throw new Error(`${bufferMiB} MiB conversion failed: ${result}`);
      if (Date.now() - lastReport >= 10_000) {
        console.log(`SMG_XIP_STATUS ${bufferMiB} MiB ${result}`);
        lastReport = Date.now();
      }
      if (Date.now() >= deadline) throw new Error(`${bufferMiB} MiB conversion timed out: ${result}`);
      await page.waitForTimeout(1000);
    }

    const elapsedSeconds = (performance.now() - started) / 1000;
    const result = (await page.locator("#status").textContent()).trim();
    console.log(`SMG_XIP_RESULT ${result}`);
    expect(result).toContain("Built smg_assets.xip");
    expect(result).toContain("from 597 selected source files");
    expect(result).toContain(`${SOURCE_BYTES.toLocaleString()} source bytes`);
    await expect(page.locator("#download")).toHaveAttribute("download", "smg_assets.xip");
    await expect(page.locator("#download")).toBeVisible();

    const output = await page.locator("#download").evaluate(async (anchor) => {
      const response = await fetch(anchor.href);
      if (!response.ok) throw new Error(`Could not read generated XIP (${response.status})`);
      return new Uint8Array(await response.arrayBuffer());
    });
    expect(output.byteLength).toBe(NATIVE_XIP.byteLength);
    expect(Buffer.compare(Buffer.from(output), NATIVE_XIP)).toBe(0);
    const outputHash = createHash("sha256").update(output).digest("hex");
    expect(outputHash).toBe(NATIVE_XIP_SHA256);
    const memoryMatch = result.match(/· ([\d.]+) MB WASM memory ·/);
    expect(memoryMatch).not.toBeNull();
    const maxRangeBytes = Number(await page.locator("#status").getAttribute("data-max-range-bytes"));
    const memoryBytes = Number(await page.locator("#status").getAttribute("data-wasm-memory-bytes"));
    expect(maxRangeBytes).toBeGreaterThan(0);
    expect(maxRangeBytes).toBeLessThanOrEqual(bufferMiB * 1024 * 1024);
    if (bufferMiB === 8) expect(memoryBytes).toBeLessThanOrEqual(768 * 1024 * 1024);

    console.log(`SMG_XIP_TIMING ${JSON.stringify({
      browser: `${engine} ${browser.version()}`,
      bufferMiB,
      elapsedSeconds: Number(elapsedSeconds.toFixed(2)),
      selectedSourceBytes: SOURCE_BYTES,
      extractorMemoryMiB: Number(memoryMatch[1]) * 1e6 / (1024 * 1024),
      maxRangeBytes,
      outputBytes: output.byteLength,
      outputSha256: outputHash,
    })}`);
  }
});
