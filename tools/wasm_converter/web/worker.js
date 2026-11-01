self.onmessage = async ({ data: { file, bufferMiB = 64 } }) => {
  try {
    const response = await fetch("smg_iso_verifier.wasm", { cache: "no-cache" });
    if (!response.ok) throw new Error(`Could not load verifier (${response.status})`);
    const { instance } = await WebAssembly.instantiateStreaming(response, {});
    const e = instance.exports;
    const size = BigInt(file.size);
    const start = e.verify_begin(Number(size & 0xffffffffn), Number(size >> 32n), bufferMiB);
    if (start === 3) throw new Error("Wrong image size. Select the 4,699,979,776-byte USA disc image.");
    if (start === 4) throw new Error(`Could not allocate a ${bufferMiB} MiB WASM input buffer. Try a smaller size.`);
    if (start !== 0) throw new Error(`Could not start verifier (status ${start}).`);

    for (;;) {
      const status = e.verify_step();
      if (status === 0) break;
      if (status === 1) {
        const bytes = (BigInt(e.bytes_processed_hi() >>> 0) << 32n) | BigInt(e.bytes_processed_lo() >>> 0);
        const processed = Number(bytes);
        self.postMessage({ type: "progress", bytes: processed, total: file.size });
        continue;
      }
      if (status !== 2) throw new Error("The image does not contain the expected RMGE01 disc header.");

      const offset = (BigInt(e.verify_request_offset_hi() >>> 0) << 32n) | BigInt(e.verify_request_offset_lo() >>> 0);
      const length = e.verify_request_len();
      if (length === 0 || length > bufferMiB * 1024 * 1024) throw new Error("The verifier requested an invalid range.");
      if (offset + BigInt(length) > size) throw new Error("The verifier requested a range beyond the image.");
      const pointer = e.verify_request_buffer_ptr();
      if (pointer + length > e.memory.buffer.byteLength) throw new Error("The verifier requested a range outside WASM memory.");
      const start = Number(offset);
      const range = new Uint8Array(await file.slice(start, start + length).arrayBuffer());
      if (range.byteLength !== length) throw new Error("The selected file returned a short range.");
      new Uint8Array(e.memory.buffer, pointer, length).set(range);
      if (e.verify_input_supply(length, 1) !== 0) throw new Error("The verifier rejected a file range.");
    }

    const digest = [...new Uint8Array(e.memory.buffer, e.result_digest_ptr(), e.result_digest_len())]
      .map((b) => b.toString(16).padStart(2, "0")).join("");
    self.postMessage({ type: "done", digest, matches: e.result_matches_expected() === 1, bytes: file.size });
  } catch (error) {
    self.postMessage({ type: "error", message: String(error?.message ?? error) });
  }
};
