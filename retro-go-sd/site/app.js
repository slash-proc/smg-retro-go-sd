const $ = (id) => document.getElementById(id);
const indexUrl = new URL('dist/versions.json', location.href);
let versions = [], selected, objectUrl, zipUrl, worker, verifiedFile, verifiedVersion;

function resetResult() {
  verifiedFile = null;
  verifiedVersion = null;
  $('progress').hidden = true;
  $('download').hidden = true;
  $('download-zip').hidden = true;
  if (objectUrl) URL.revokeObjectURL(objectUrl);
  if (zipUrl) URL.revokeObjectURL(zipUrl);
  objectUrl = null;
  zipUrl = null;
}

function status(text, kind = '') {
  $('status').textContent = text;
  $('status').className = `status ${kind}`;
}

async function json(url) {
  const response = await fetch(url, { cache: 'no-cache' });
  if (!response.ok) throw new Error(`Could not load release information (${response.status}).`);
  return response.json();
}

async function selectVersion() {
  resetResult();
  selected = null;
  $('convert').disabled = true;
  $('convert-only').disabled = true;
  $('verify').disabled = true;
  $('bundle').hidden = true;
  const entry = versions[$('version').selectedIndex];
  const manifestUrl = new URL(entry.manifest, indexUrl);
  const manifest = await json(manifestUrl);
  const tool = manifest.tools.find((item) => item.id === 'smg-assets');
  if (manifest.schemaVersion !== 1 || !tool || tool.processor.extensions?.['range-input'] !== 1)
    throw new Error('This release does not provide the required streaming converter.');
  const target = manifest.targets.find((item) => item.id === 'gnw-retro-go');
  const artifacts = ['SMG.bin', 'smg_code.xip'].map((name) => target?.artifacts.find((item) => item.filename === name));
  if (artifacts.some((item) => !item)) throw new Error('This release is missing required install files.');
  selected = { tool, tag: entry.tag, artifacts: artifacts.map((item) => ({ ...item, url: new URL(item.url, manifestUrl).href })), wasmUrl: new URL(tool.binary.url, manifestUrl).href };
  if (entry.bundle) {
    $('bundle').href = new URL(entry.bundle, indexUrl).href;
    $('bundle').hidden = false;
  }
  $('convert').disabled = !$('iso').files.length;
  $('convert-only').disabled = !$('iso').files.length;
  $('verify').disabled = !$('iso').files.length;
  status('Choose your USA disc image.');
}

function runWorker(script, message, onProgress, transfer = []) {
  return new Promise((resolve, reject) => {
    worker = new Worker(script, { type: 'module' });
    worker.onerror = (event) => { worker.terminate(); reject(new Error(event.message)); };
    worker.onmessage = ({ data }) => {
      if (data.type === 'error') { worker.terminate(); reject(new Error(data.message)); }
      else if (data.type === 'done') { worker.terminate(); resolve(data); }
      else onProgress(data);
    };
    worker.postMessage(message, transfer);
  });
}

$('version').addEventListener('change', () => selectVersion().catch((e) => status(e.message, 'bad')));
$('iso').addEventListener('change', () => {
  const file = $('iso').files[0];
  resetResult();
  $('filename').textContent = file?.name ?? 'No file selected';
  $('convert').disabled = !file || !selected;
  $('convert-only').disabled = !file || !selected;
  $('verify').disabled = !file || !selected;
  status(file ? `${file.size.toLocaleString()} bytes selected.` : 'Choose your USA disc image.');
});
async function processFile(mode) {
  const file = $('iso').files[0];
  if (!file || !selected) return;
  $('convert').disabled = true;
  $('convert-only').disabled = true;
  $('verify').disabled = true;
  $('version').disabled = true;
  $('iso').disabled = true;
  $('buffer-size').disabled = true;
  $('download').hidden = true;
  $('download-zip').hidden = true;
  $('progress').hidden = false;
  if (objectUrl) URL.revokeObjectURL(objectUrl);
  if (zipUrl) URL.revokeObjectURL(zipUrl);
  try {
    const input = selected.tool.inputs.find((item) => item.id === 'disc');
    if (!input || !file.name.toLowerCase().endsWith('.iso') || file.size > input.maxBytes)
      throw new Error('Select a supported USA ISO.');
    if (mode !== 'convert-only' && (verifiedFile !== file || verifiedVersion !== selected)) {
      $('progress').value = 0;
      status('Verifying the disc image…');
      const verification = await runWorker('verify-worker.js', { file, bufferMiB: Number($('buffer-size').value) }, (data) => {
        if (data.type === 'progress') {
          $('progress').value = data.bytes / data.total * 100;
          status(`Verifying: ${(data.bytes / data.total * 100).toFixed(1)}% · ${(data.bytes / 1e9).toFixed(2)} / ${(data.total / 1e9).toFixed(2)} GB read`);
        }
      });
      const variant = input.variants.find((item) => item.bytes === file.size && item.sha1.toLowerCase() === verification.digest);
      if (!verification.matches || !variant) throw new Error(`The disc image does not match a supported USA version. SHA-1: ${verification.digest}`);
      verifiedFile = file;
      verifiedVersion = selected;
      status(`Verified RMGE01 · SHA-1 ${verification.digest}`, 'good');
    }
    if (mode === 'verify') { $('progress').value = 100; return; }
    $('progress').value = 0;
    status('Converting assets…');
    const binary = selected.tool.binary;
    const result = await runWorker('extract-worker.js', {
      file, bufferMiB: 8, wasmUrl: selected.wasmUrl, wasmSha256: binary.sha256, wasmBytes: binary.bytes,
    }, (data) => {
      if (data.type === 'progress' || data.type === 'range') {
        $('progress').value = data.files / data.totalFiles * 100;
        status(`Converting: ${data.files}/${data.totalFiles} files · ${(data.ranges / 1e6).toFixed(1)} MB read`);
      } else if (data.type === 'phase') {
        $('progress').removeAttribute('value');
        status('Building the final asset XIP in WASM…');
      }
    });
    const output = selected.tool.outputs.find((item) => item.filename === result.outputName);
    if (!output || result.outputBytes > output.maxBytes || result.outputBytes > selected.tool.limits.maxOutputBytes)
      throw new Error('Converter output does not match the release manifest.');
    $('progress').removeAttribute('value');
    status('Preparing install ZIP and checking release files…');
    const packaged = await runWorker('zip-worker.js', { artifacts: selected.artifacts, output: result.output, outputName: result.outputName }, () => {}, [result.output]);
    objectUrl = URL.createObjectURL(packaged.output);
    zipUrl = URL.createObjectURL(packaged.zip);
    $('download-zip').href = zipUrl;
    $('download-zip').download = `smg-${selected.tag}-install.zip`;
    $('download-zip').textContent = `Download install ZIP (${(packaged.zip.size / 1e6).toFixed(2)} MB)`;
    $('download-zip').hidden = false;
    $('download').href = objectUrl;
    $('download').download = result.outputName;
    $('download').textContent = `Download ${result.outputName} (${(result.outputBytes / 1e6).toFixed(2)} MB)`;
    $('download').hidden = false;
    $('progress').value = 100;
    status(`Built ${result.outputName} from ${result.files} files · ${(result.ranges / 1e6).toFixed(1)} MB read · ${(result.memoryBytes / 1e6).toFixed(1)} MB WASM memory · ${result.seconds.toFixed(2)} s.`, 'good');
    $('status').dataset.maxRangeBytes = String(result.maxRange);
    $('status').dataset.wasmMemoryBytes = String(result.memoryBytes);
  } catch (error) {
    status(error.message, 'bad');
  } finally {
    $('convert').disabled = false;
    $('convert-only').disabled = false;
    $('verify').disabled = false;
    $('version').disabled = false;
    $('iso').disabled = false;
    $('buffer-size').disabled = false;
  }
}
$('verify').addEventListener('click', () => processFile('verify'));
$('convert').addEventListener('click', () => processFile('verify-and-convert'));
$('convert-only').addEventListener('click', () => processFile('convert-only'));

try {
  const index = await json(indexUrl);
  if (index.schemaVersion !== 1 || !index.versions?.length) throw new Error('No compatible releases are available.');
  versions = index.versions;
  for (const entry of versions) $('version').add(new Option(entry.tag, entry.tag));
  $('version').disabled = false;
  await selectVersion();
} catch (error) { status(error.message, 'bad'); }
