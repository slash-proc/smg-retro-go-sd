const input = document.querySelector("#iso");
const filename = document.querySelector("#filename");
const button = document.querySelector("#extract");
const download = document.querySelector("#download");
const status = document.querySelector("#status");
const progress = document.querySelector("#progress");
const bufferSelect = document.querySelector("#buffer-size");
let downloadUrl;
let worker;

input.addEventListener("change", () => {
  const file = input.files?.[0];
  filename.textContent = file?.name ?? "No file selected";
  button.disabled = !file;
  progress.hidden = true;
  download.hidden = true;
  if (downloadUrl) URL.revokeObjectURL(downloadUrl);
  downloadUrl = undefined;
  status.className = "status";
  status.textContent = file ? `${file.size.toLocaleString()} bytes selected.` : "Waiting for an ISO.";
});

button.addEventListener("click", () => {
  const file = input.files?.[0];
  if (!file) return;
  worker?.terminate();
  worker = new Worker("extract-worker.js", { type: "module" });
  button.disabled = true;
  progress.hidden = false;
  progress.value = 0;
  status.className = "status";
  download.hidden = true;
  if (downloadUrl) URL.revokeObjectURL(downloadUrl);
  const bufferMiB = Number(bufferSelect.value);
  status.textContent = `Building the linked asset XIP with ${bufferMiB} MiB ranges…`;
  worker.onmessage = ({ data }) => {
    if (data.type === "progress") {
      progress.value = data.files / data.totalFiles * 100;
      status.textContent = `${data.files}/${data.totalFiles} files · ${(data.ranges / 1e6).toFixed(1)} MB ranges read`;
      return;
    }
    if (data.type === "range") {
      status.textContent = `${data.files}/${data.totalFiles} files · ${(data.ranges / 1e6).toFixed(1)} MB read · requesting ${Number(data.length / 1e6).toFixed(1)} MB at disc offset ${Number(data.offset).toLocaleString()}`;
      return;
    }
    if (data.type === "phase") {
      status.textContent = {
        compiler: "Building the final asset XIP in WASM…",
        packing: "Applying canonical asset sharing and packing…",
        assembler: "Assembling the mapped asset XIP sidecar…",
      }[data.phase] ?? "Processing generated assets…";
      return;
    }
    worker.terminate();
    button.disabled = false;
    if (data.type === "error") {
      status.className = "status bad";
      status.textContent = data.message;
    } else {
      progress.value = 100;
      status.className = "status good";
      downloadUrl = URL.createObjectURL(new Blob([data.output], { type: "application/octet-stream" }));
      download.href = downloadUrl;
      download.download = data.outputName;
      download.textContent = `Download ${data.outputName} (${(data.outputBytes / 1e6).toFixed(2)} MB)`;
      download.hidden = false;
      status.textContent = `Built ${data.outputName} (${data.outputBytes.toLocaleString()} bytes) from ${data.files} selected source files · ${data.bytes.toLocaleString()} source bytes · ${(data.ranges / 1e6).toFixed(1)} MB ranges · ${(data.memoryBytes / 1e6).toFixed(1)} MB WASM memory · ${data.seconds.toFixed(2)} s.`;
      status.dataset.maxRangeBytes = String(data.maxRange);
      status.dataset.wasmMemoryBytes = String(data.memoryBytes);
    }
  };
  worker.postMessage({ file, bufferMiB });
});
