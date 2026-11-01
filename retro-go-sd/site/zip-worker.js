// Build an uncompressed ZIP from the selected release and generated assets.
// Blob parts avoid copying the large generated asset into another byte array.
const encoder = new TextEncoder();
const crcTable = Uint32Array.from({ length: 256 }, (_, n) => {
  for (let i = 0; i < 8; i++) n = (n >>> 1) ^ (0xedb88320 & -(n & 1));
  return n >>> 0;
});

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) crc = (crc >>> 8) ^ crcTable[(crc ^ byte) & 255];
  return (crc ^ 0xffffffff) >>> 0;
}

function header(size, fields) {
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  for (const [offset, value, width] of fields) {
    if (width === 2) view.setUint16(offset, value, true);
    else view.setUint32(offset, value, true);
  }
  return bytes;
}

self.onmessage = async ({ data: { artifacts, output, outputName } }) => {
  try {
    const files = [];
    for (const artifact of artifacts) {
      const response = await fetch(artifact.url, { cache: 'no-cache' });
      if (!response.ok) throw new Error(`Could not download ${artifact.filename} (${response.status}).`);
      const bytes = new Uint8Array(await response.arrayBuffer());
      const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))]
        .map((byte) => byte.toString(16).padStart(2, '0')).join('');
      if (bytes.length !== artifact.bytes || digest !== artifact.sha256.toLowerCase())
        throw new Error(`${artifact.filename} does not match the release manifest.`);
      files.push({ name: artifact.filename, bytes });
    }
    files.push({ name: outputName, bytes: new Uint8Array(output) });
    const parts = [], directory = [];
    let offset = 0;
    for (const file of files) {
      if (!/^[A-Za-z0-9_.-]+$/.test(file.name) || file.bytes.length > 0xffffffff)
        throw new Error('Invalid ZIP entry.');
      const name = encoder.encode(file.name), crc = crc32(file.bytes), length = file.bytes.length;
      const local = header(30, [[0, 0x04034b50, 4], [4, 20, 2], [14, crc, 4], [18, length, 4], [22, length, 4], [26, name.length, 2]]);
      const central = header(46, [[0, 0x02014b50, 4], [4, 20, 2], [6, 20, 2], [16, crc, 4], [20, length, 4], [24, length, 4], [28, name.length, 2], [42, offset, 4]]);
      parts.push(local, name, file.bytes);
      directory.push(central, name);
      offset += local.length + name.length + length;
    }
    const directorySize = directory.reduce((sum, part) => sum + part.length, 0);
    const end = header(22, [[0, 0x06054b50, 4], [8, files.length, 2], [10, files.length, 2], [12, directorySize, 4], [16, offset, 4]]);
    const zip = new Blob([...parts, ...directory, end], { type: 'application/zip' });
    self.postMessage({ type: 'done', zip, output: new Blob([output], { type: 'application/octet-stream' }) });
  } catch (error) {
    self.postMessage({ type: 'error', message: String(error?.message ?? error) });
  }
};
