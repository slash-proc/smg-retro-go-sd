import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';

test('release page verifies the ISO and streams the converter', async ({ page }) => {
  test.skip(process.env.SMG_RELEASE_SITE !== '1', 'requires an assembled release site');
  test.setTimeout(900_000);
  await page.goto('/');
  await expect(page.locator('#version')).toHaveValue('v0.0.1');
  await expect(page.locator('#bundle')).toBeVisible();
  await page.locator('#iso').setInputFiles(process.env.SMG_ISO_PATH || '/workspace/baserom_us.iso');
  if (process.env.SMG_CONVERT_ONLY === '1') {
    await page.locator('#convert-only').click();
  } else {
    await page.locator('#verify').click();
    await expect(page.locator('#status')).toContainText('Verified RMGE01', { timeout: 840_000 });
    await expect(page.locator('#progress')).toHaveJSProperty('value', 100);
    await page.locator('#convert').click();
  }
  await expect(page.locator('#status')).toHaveClass(/good/, { timeout: 840_000 });
  expect(Number(await page.locator('#status').getAttribute('data-max-range-bytes'))).toBeLessThanOrEqual(8 * 1024 * 1024);
  const memory = Number(await page.locator('#status').getAttribute('data-wasm-memory-bytes'));
  expect(memory).toBeGreaterThan(0);
  expect(memory).toBeLessThanOrEqual(768 * 1024 * 1024);
  const output = await page.locator('#download').evaluate(async (anchor) => new Uint8Array(await (await fetch(anchor.href)).arrayBuffer()));
  expect(Buffer.compare(Buffer.from(output), readFileSync('/workspace/retro-go-sd/smg_assets.xip'))).toBe(0);
  console.log(`SITE_WASM_MEMORY_BYTES ${memory}`);
});
