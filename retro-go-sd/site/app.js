const $ = (id) => document.getElementById(id);
const indexUrl = new URL('dist/versions.json', location.href);
let versions = [], selected, objectUrl, worker;

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
  selected = null;
  $('convert').disabled = true;
  $('bundle').hidden = true;
  const entry = versions[$('version').selectedIndex];
  const manifestUrl = new URL(entry.manifest, indexUrl);
  const manifest = await json(manifestUrl);
  const tool = manifest.tools.find((item) => item.id === 'smg-assets');
  if (manifest.schemaVersion !== 1 || !tool || tool.processor.extensions?.['range-input'] !== 1)
    throw new Error('This release does not provide the required streaming converter.');
  selected = { tool, wasmUrl: new URL(tool.binary.url, manifestUrl).href };
  if (entry.bundle) {
    $('bundle').href = new URL(entry.bundle, indexUrl).href;
    $('bundle').hidden = false;
  }
  $('convert').disabled = !$('iso').files.length;
  status('Choose your USA disc image.');
}

function runWorker(script, message, onProgress) {
  return new Promise((resolve, reject) => {
    worker = new Worker(script, { type: 'module' });
    worker.onerror = (event) => { worker.terminate(); reject(new Error(event.message)); };
    worker.onmessage = ({ data }) => {
      if (data.type === 'error') { worker.terminate(); reject(new Error(data.message)); }
      else if (data.type === 'done') { worker.terminate(); resolve(data); }
      else onProgress(data);
    };
    worker.postMessage(message);
  });
}

$('version').addEventListener('change', () => selectVersion().catch((e) => status(e.message, 'bad')));
$('iso').addEventListener('change', () => {
  const file = $('iso').files[0];
  $('filename').textContent = file?.name ?? 'No file selected';
  $('convert').disabled = !file || !selected;
});
$('convert').addEventListener('click', async () => {
  const file = $('iso').files[0];
  if (!file || !selected) return;
  $('convert').disabled = true;
  $('version').disabled = true;
  $('iso').disabled = true;
  $('download').hidden = true;
  $('progress').hidden = false;
  if (objectUrl) URL.revokeObjectURL(objectUrl);
  try {
    const input = selected.tool.inputs.find((item) => item.id === 'disc');
    if (!input || !file.name.toLowerCase().endsWith('.iso') || file.size > input.maxBytes)
      throw new Error('Select a supported USA ISO.');
    status('Verifying the disc image…');
    const verification = await runWorker('verify-worker.js', { file, bufferMiB: 8 }, (data) => {
      if (data.type === 'progress') $('progress').value = data.bytes / data.total * 100;
    });
    const variant = input.variants.find((item) => item.bytes === file.size && item.sha1.toLowerCase() === verification.digest);
    if (!verification.matches || !variant) throw new Error('The disc image does not match a supported USA version.');
    $('progress').value = 0;
    status('Converting assets…');
    const binary = selected.tool.binary;
    const result = await runWorker('extract-worker.js', {
      file, bufferMiB: 8, wasmUrl: selected.wasmUrl, wasmSha256: binary.sha256, wasmBytes: binary.bytes,
    }, (data) => {
      if (data.type === 'progress' || data.type === 'range') $('progress').value = data.files / data.totalFiles * 100;
    });
    const output = selected.tool.outputs.find((item) => item.filename === result.outputName);
    if (!output || result.outputBytes > output.maxBytes || result.outputBytes > selected.tool.limits.maxOutputBytes)
      throw new Error('Converter output does not match the release manifest.');
    objectUrl = URL.createObjectURL(new Blob([result.output], { type: 'application/octet-stream' }));
    $('download').href = objectUrl;
    $('download').download = result.outputName;
    $('download').textContent = `Download ${result.outputName}`;
    $('download').hidden = false;
    $('progress').value = 100;
    status('Conversion complete.', 'good');
    $('status').dataset.maxRangeBytes = String(result.maxRange);
    $('status').dataset.wasmMemoryBytes = String(result.memoryBytes);
  } catch (error) {
    status(error.message, 'bad');
  } finally {
    $('convert').disabled = false;
    $('version').disabled = false;
    $('iso').disabled = false;
  }
});

try {
  const index = await json(indexUrl);
  if (index.schemaVersion !== 1 || !index.versions?.length) throw new Error('No compatible releases are available.');
  versions = index.versions;
  for (const entry of versions) $('version').add(new Option(entry.tag, entry.tag));
  $('version').disabled = false;
  await selectVersion();
} catch (error) { status(error.message, 'bad'); }
