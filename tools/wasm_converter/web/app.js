const input = document.querySelector("#iso");
const filename = document.querySelector("#filename");
const button = document.querySelector("#verify");
const status = document.querySelector("#status");
const progress = document.querySelector("#progress");
const bufferSelect = document.querySelector("#buffer-size");
let worker;

input.addEventListener("change", () => {
  const file = input.files?.[0];
  filename.textContent = file?.name ?? "No file selected";
  button.disabled = !file;
  progress.hidden = true;
  status.className = "status";
  status.textContent = file ? `${file.size.toLocaleString()} bytes selected.` : "Waiting for an ISO.";
});

button.addEventListener("click", () => {
  const file = input.files?.[0];
  if (!file) return;
  worker?.terminate();
  worker = new Worker("worker.js");
  button.disabled = true;
  progress.hidden = false;
  progress.value = 0;
  status.className = "status";
  const bufferMiB = Number(bufferSelect.value);
  status.textContent = `Reading and hashing in ${bufferMiB} MiB WASM chunks…`;
  worker.onmessage = ({ data }) => {
    if (data.type === "progress") {
      progress.value = data.bytes;
      status.textContent = `${(data.bytes / data.total * 100).toFixed(1)}% · ${(data.bytes / 1e9).toFixed(2)} / ${(data.total / 1e9).toFixed(2)} GB read`;
      return;
    }
    worker.terminate();
    button.disabled = false;
    if (data.type === "error") {
      status.className = "status bad";
      status.textContent = data.message;
    } else if (!data.matches) {
      status.className = "status bad";
      status.textContent = `SHA-1 mismatch: ${data.digest}`;
    } else {
      progress.value = progress.max;
      status.className = "status good";
      status.textContent = `Verified RMGE01 · SHA-1 ${data.digest}`;
    }
  };
  worker.postMessage({ file, bufferMiB });
});
