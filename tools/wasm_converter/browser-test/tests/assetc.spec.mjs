import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";

test("browser WASI shim runs the asset compiler and returns generated files", async ({ page }) => {
  await page.goto("/");
  const shadow = [...readFileSync("/workspace/build/constructed/assets/shadow.png")];
  const result = await page.evaluate(async (shadowBytes) => {
    const response = await fetch("/assetc.wasm");
    if (!response.ok) throw new Error(`Could not load asset compiler (${response.status})`);
    const wasmBytes = await response.arrayBuffer();
    const { runAssetCompiler } = await import("/assetc-runner.js");
    const files = await runAssetCompiler(wasmBytes, new Map([
      ["assets.cfg", new TextEncoder().encode("music title title.pcm8 0\nsound click click.pcm8\n")],
      ["audio/title.pcm8", Uint8Array.from({ length: 1024 }, (_, i) => (i * 17) & 0xff)],
      ["audio/click.pcm8", Uint8Array.from({ length: 1024 }, (_, i) => (i * 29) & 0xff)],
      ["palette.rgba", new Uint8Array(1024)],
      ["shadow.png", new Uint8Array(shadowBytes)],
    ]));
    const header = new TextDecoder().decode(files.get("assets_gen.h"));
    const assembly = new TextDecoder().decode(files.get("assets.s"));
    return {
      count: files.size,
      hasPalette: files.has("game_palette.bin"),
      hasShadow: files.has("shadow_texture.bin"),
      hasHeader: header.includes("#define OBJ_TYPE_COUNT"),
      hasAssembly: assembly.includes("lut_sin_cos:"),
      hasMusicId: header.includes("#define MUSIC_TITLE 0"),
      hasSoundId: header.includes("#define SFX_CLICK 0"),
      hasMusicPayload: files.has("music_title.bin"),
      hasSoundPayload: files.has("sfx_click.bin"),
    };
  }, shadow);

  expect(result).toEqual({
    count: 19,
    hasPalette: true,
    hasShadow: true,
    hasHeader: true,
    hasAssembly: true,
    hasMusicId: true,
    hasSoundId: true,
    hasMusicPayload: true,
    hasSoundPayload: true,
  });
});
