self.onmessage = async ({ data: { file, bufferMiB = 64 } }) => {
  let exports;
  try {
    if (![8, 64, 128, 256].includes(bufferMiB)) throw new Error("Unsupported WASM range size.");
    const wasmName = bufferMiB === 8 ? "smg_asset_converter.wasm" : `smg_asset_converter_${bufferMiB}.wasm`;
    const response = await fetch(wasmName, { cache: "no-cache" });
    if (!response.ok) throw new Error(`Could not load converter (${response.status})`);
    const { instance } = await WebAssembly.instantiateStreaming(response, {});
    const e = exports = instance.exports;
    const size = BigInt(file.size);
    if (e.abi_version() !== 1) throw new Error("Unsupported GWRG processor ABI.");
    const handle = e.input_stream_add(Number(size & 0xffffffffn), Number(size >> 32n));
    if (handle !== 0) throw new Error("Expected a single 4,699,979,776-byte USA ISO.");
    const start = e.run_begin(0);
    if (start !== 0) throw new Error(readError(e, start));

    const began = performance.now();
    let ranges = 0;
    for (;;) {
      const result = e.run_step();
      if (result === 0) break;
      if (result === 1) {
        const stage = readStageName(e);
        if (stage === "Converting assets and assembling final XIP") {
          self.postMessage({ type: "phase", phase: "compiler" });
        } else {
          self.postMessage({ type: "progress", files: e.stage_index(), totalFiles: e.stage_count(), ranges });
        }
        continue;
      }
      if (result !== 2) throw new Error(readError(e, result));
      const offset = (BigInt(e.input_request_offset_hi() >>> 0) << 32n) | BigInt(e.input_request_offset_lo() >>> 0);
      const length = e.input_request_len();
      if (offset + BigInt(length) > size) throw new Error("Extractor requested a range beyond the image.");
      if (e.input_request_index() !== handle) throw new Error("Extractor requested an unknown input.");
      self.postMessage({ type: "range", files: e.stage_index(), totalFiles: e.stage_count(), ranges, offset: offset.toString(), length });
      const range = new Uint8Array(await file.slice(Number(offset), Number(offset) + length).arrayBuffer());
      if (range.byteLength !== length) throw new Error("The selected file returned a short range.");
      new Uint8Array(e.memory.buffer, e.input_request_buffer_ptr(), length).set(range);
      if (e.input_supply(length, 1) !== 0) throw new Error("The extractor rejected a file range.");
      ranges += length;
    }

    if (e.output_count() !== 1) throw new Error(`Expected one linked asset XIP output, received ${e.output_count()} outputs.`);
    const outputName = readOutputName(e);
    if (outputName !== "smg_assets.xip") throw new Error(`Unexpected output ${outputName}.`);
    const outputLength = e.output_len(0);
    if (outputLength === 0) throw new Error("Processor returned an empty asset XIP.");
    const output = new Uint8Array(e.memory.buffer, e.output_ptr(0), outputLength).slice().buffer;
    self.postMessage({
      type: "done", files: e.stage_index(), bytes: 261599168, ranges,
      memoryBytes: e.memory.buffer.byteLength, seconds: (performance.now() - began) / 1000,
      outputName, outputBytes: output.byteLength, output,
    }, [output]);
  } catch (error) {
    const memory = exports?.memory?.buffer?.byteLength;
    const files = exports?.stage_index?.();
    const message = String(error?.message ?? error);
    self.postMessage({
      type: "error",
      message: memory === undefined ? message : `${message} · ${files} files · ${memory} WASM bytes`,
    });
  }
};

function readString(exports, pointer, length) {
  return new TextDecoder("utf-8", { fatal: true })
    .decode(new Uint8Array(exports.memory.buffer, pointer, length));
}

function readStageName(exports) {
  return readString(exports, exports.stage_name_ptr(0), exports.stage_name_len(0));
}

function readOutputName(exports) {
  return readString(exports, exports.output_name_ptr(0), exports.output_name_len(0));
}

function readError(exports, code) {
  const bytes = new Uint8Array(exports.memory.buffer, exports.error_ptr(), exports.error_len());
  return new TextDecoder().decode(bytes) || `Converter failed (${code}).`;
}
