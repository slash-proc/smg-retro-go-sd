import { open, readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import process from "node:process";

const [wasmPath, inputPath] = process.argv.slice(2);
if (!wasmPath || !inputPath) {
  throw new Error("usage: node test-node.mjs <converter.wasm> <large-input-file>");
}

const wasmBytes = await readFile(wasmPath);
const module = await WebAssembly.compile(wasmBytes);
const imports = WebAssembly.Module.imports(module);
if (imports.length !== 0) throw new Error(`expected zero imports, found ${imports.length}`);

const instance = await WebAssembly.instantiate(module, {});
const e = instance.exports;
const file = await open(inputPath, "r");
try {
  const { size } = await file.stat({ bigint: true });
  const sizeLo = Number(size & 0xffffffffn);
  const sizeHi = Number(size >> 32n);
  if (e.run_begin(sizeLo, sizeHi) !== 0) throw new Error("run_begin rejected input size");

  for (;;) {
    const status = e.run_step();
    if (status === 0) break;
    if (status === 1) continue;
    if (status !== 2) throw new Error(`unexpected run_step status ${status}`);

    const offset = (BigInt(e.request_offset_hi()) << 32n) | BigInt(e.request_offset_lo());
    const length = e.request_len();
    const range = Buffer.alloc(length);
    const { bytesRead } = await file.read(range, 0, length, Number(offset));
    if (bytesRead !== length) throw new Error(`short range read: ${bytesRead}/${length}`);
    new Uint8Array(e.memory.buffer, e.request_buffer_ptr(), length).set(range);
    if (e.input_supply(bytesRead, 1) !== 0) throw new Error("module rejected range");
  }

  let checksum = 2_166_136_261;
  const offset = Number((BigInt(e.request_offset_hi()) << 32n) | BigInt(e.request_offset_lo()));
  const expectedBytes = Buffer.alloc(e.request_len());
  const { bytesRead } = await file.read(expectedBytes, 0, expectedBytes.length, offset);
  if (bytesRead !== expectedBytes.length) throw new Error("could not reread requested range");
  for (const byte of expectedBytes) checksum = Math.imul(checksum ^ byte, 16_777_619) >>> 0;
  const actual = e.result_checksum() >>> 0;
  if (actual !== checksum) throw new Error(`checksum mismatch: ${actual} != ${checksum}`);

  console.log(JSON.stringify({
    imports,
    fileBytes: size.toString(),
    requestedOffset: offset,
    requestedBytes: bytesRead,
    checksum: actual,
    wasmMemoryBytes: e.memory.buffer.byteLength,
  }, null, 2));
} finally {
  await file.close();
}
