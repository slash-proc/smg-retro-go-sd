import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

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
  const zipLink = page.locator('#download-zip');
  await expect(zipLink).toBeVisible();
  await expect(zipLink).toHaveAttribute('download', 'smg-v0.0.1-install.zip');
  const downloaded = page.waitForEvent('download');
  await zipLink.click();
  const zipPath = await (await downloaded).path();
  execFileSync('python3', ['-c', `
import hashlib, json, sys, urllib.request, zipfile
with urllib.request.urlopen(sys.argv[3] + '/dist/v0.0.1/manifest.json') as response:
    manifest = json.load(response)
artifacts = {item['filename']: item for item in manifest['targets'][0]['artifacts']}
with zipfile.ZipFile(sys.argv[1]) as archive:
    assert set(archive.namelist()) == {'SMG.bin', 'smg_code.xip', 'smg_assets.xip'}
    assert archive.testzip() is None
    for name in ('SMG.bin', 'smg_code.xip'):
        data = archive.read(name)
        assert len(data) == artifacts[name]['bytes']
        assert hashlib.sha256(data).hexdigest() == artifacts[name]['sha256']
    with open(sys.argv[2], 'rb') as native:
        assert archive.read('smg_assets.xip') == native.read()
`, zipPath, '/workspace/retro-go-sd/smg_assets.xip', process.env.SMG_BROWSER_URL]);
  const output = await page.locator('#download').evaluate(async (anchor) => new Uint8Array(await (await fetch(anchor.href)).arrayBuffer()));
  expect(Buffer.compare(Buffer.from(output), readFileSync('/workspace/retro-go-sd/smg_assets.xip'))).toBe(0);
  console.log(`SITE_WASM_MEMORY_BYTES ${memory}`);
});
