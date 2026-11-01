#!/usr/bin/env node

// Structural gate for the published SMG converter. Keep this aligned with
// gwprov's processor ABI checks and the range-input v1 extension.
import { readFileSync } from "node:fs";

export const DEFAULT_POLICY = {
  maxMemoryPages: 16384,
  maxModuleBytes: 32 * 1024 * 1024,
};

const baseFunctions = [
  "abi_version", "alloc", "input_clear", "input_add", "run", "run_begin",
  "run_step", "stage_count", "stage_index", "stage_name_ptr", "stage_name_len",
  "output_count", "output_name_ptr", "output_name_len", "output_ptr", "output_len",
  "error_ptr", "error_len", "warnings_ptr", "warnings_len",
];
const rangeFunctions = [
  "input_stream_add", "input_request_index", "input_request_offset_lo",
  "input_request_offset_hi", "input_request_len", "input_request_buffer_ptr",
  "input_supply",
];

function readU32(bytes, cursor) {
  let value = 0;
  let shift = 0;
  for (let count = 0; count < 5; count++) {
    if (cursor.offset >= bytes.length) throw new Error("truncated unsigned LEB128");
    const byte = bytes[cursor.offset++];
    value |= (byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return value >>> 0;
    shift += 7;
  }
  throw new Error("invalid unsigned LEB128");
}

function inspectMemory(bytes) {
  const cursor = { offset: 8 };
  let memories = 0;
  let minimum = 0;
  let maximum = 0;
  let hasMaximum = false;
  let shared = false;
  let hasStart = false;
  while (cursor.offset < bytes.length) {
    const id = bytes[cursor.offset++];
    const size = readU32(bytes, cursor);
    const end = cursor.offset + size;
    if (end > bytes.length) throw new Error("section extends past end of module");
    if (id === 8) hasStart = true;
    if (id === 5) {
      const count = readU32(bytes, cursor);
      memories += count;
      if (count !== 1) throw new Error(`expected one linear memory, found ${count}`);
      const flags = readU32(bytes, cursor);
      hasMaximum = (flags & 1) !== 0;
      shared = (flags & 2) !== 0;
      minimum = readU32(bytes, cursor);
      if (hasMaximum) maximum = readU32(bytes, cursor);
      if (cursor.offset !== end) throw new Error("malformed memory section");
    }
    cursor.offset = end;
  }
  if (memories !== 1) throw new Error(`expected one linear memory, found ${memories}`);
  if (!hasMaximum) throw new Error("linear memory must declare a maximum");
  if (shared) throw new Error("shared memory is not permitted");
  if (minimum > DEFAULT_POLICY.maxMemoryPages || maximum > DEFAULT_POLICY.maxMemoryPages) {
    throw new Error("linear memory exceeds the declared page ceiling");
  }
  if (hasStart) throw new Error("start sections are not permitted");
}

function verify(path, extensionArg) {
  const bytes = readFileSync(path);
  if (bytes.length > DEFAULT_POLICY.maxModuleBytes) {
    throw new Error(`module is ${bytes.length} bytes, over the ${DEFAULT_POLICY.maxModuleBytes} byte limit`);
  }
  const extensions = JSON.parse(extensionArg ?? "{}");
  if (extensions === null || typeof extensions !== "object" || Array.isArray(extensions)) {
    throw new Error("processor extensions must be an object");
  }
  if (Object.keys(extensions).some((name) => name !== "range-input" || extensions[name] !== 1)) {
    throw new Error(`unsupported processor extensions: ${JSON.stringify(extensions)}`);
  }
  const module = new WebAssembly.Module(bytes);
  const imports = WebAssembly.Module.imports(module);
  if (imports.length) throw new Error(`module imports ${imports.length} host functions`);
  const functions = [...baseFunctions, ...(extensions["range-input"] === 1 ? rangeFunctions : [])];
  const expected = new Map([
    ["memory", "memory"],
    ...functions.map((name) => [name, "function"]),
  ]);
  const optionalGlobals = new Set(["__data_end", "__heap_base"]);
  const actual = WebAssembly.Module.exports(module);
  for (const item of actual) {
    if (optionalGlobals.has(item.name) && item.kind === "global") continue;
    if (expected.get(item.name) !== item.kind) {
      throw new Error(`unexpected or malformed export ${item.name} (${item.kind})`);
    }
    expected.delete(item.name);
  }
  if (expected.size) throw new Error(`missing exports: ${[...expected.keys()].join(", ")}`);
  inspectMemory(bytes);
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  try {
    verify(process.argv[2], process.argv[3]);
    console.log("converter module structure passed");
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
