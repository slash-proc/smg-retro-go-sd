#!/usr/bin/env node
import { readFile } from 'node:fs/promises';

const bytes = await readFile(process.argv[2]);
const { instance } = await WebAssembly.instantiate(bytes, {});
const e = instance.exports;
const size = 4699979776n;
if (e.verify_begin(Number(size & 0xffffffffn), Number(size >> 32n), 8) !== 0)
  throw new Error('verifier does not accept 8 MiB windows');
if (e.verify_step() !== 2 || e.verify_request_len() > 8 * 1024 * 1024)
  throw new Error('verifier did not request a bounded range');
if (e.memory.buffer.byteLength > 16 * 1024 * 1024)
  throw new Error('verifier uses more than 16 MiB of linear memory');
console.log(`8 MiB streaming verifier passed; ${e.memory.buffer.byteLength} bytes WASM memory`);
