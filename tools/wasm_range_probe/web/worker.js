self.onmessage = async ({ data }) => {
  const { file, wasmUrl } = data;
  try {
    const { instance } = await WebAssembly.instantiateStreaming(fetch(wasmUrl), {});
    const e = instance.exports;
    const size = BigInt(file.size);
    e.run_begin(Number(size & 0xffffffffn), Number(size >> 32n));

    for (;;) {
      const status = e.run_step();
      if (status === 0) break;
      if (status === 1) continue;
      if (status !== 2) throw new Error(`probe returned status ${status}`);

      const offset = (BigInt(e.request_offset_hi()) << 32n) | BigInt(e.request_offset_lo());
      const length = e.request_len();
      const start = Number(offset);
      if (offset + BigInt(length) > size) throw new Error("requested range exceeds input");
      const range = new Uint8Array(await file.slice(start, start + length).arrayBuffer());
      if (range.byteLength !== length) throw new Error("short file range read");
      new Uint8Array(e.memory.buffer, e.request_buffer_ptr(), length).set(range);
      if (e.input_supply(length, 1) !== 0) throw new Error("module rejected range");
    }

    self.postMessage({ checksum: e.result_checksum() >>> 0, fileBytes: file.size });
  } catch (error) {
    self.postMessage({ error: String(error) });
  }
};
