#!/usr/bin/env node
// Run the shared GWRG checker against an assembled local Pages tree.
import { createServer } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { check } from './dist-spec/site/check.js';

const root = resolve(process.argv[2] || '_site');
const schemas = fileURLToPath(new URL('./dist-spec/schema/', import.meta.url));
const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, 'http://localhost');
    const schema = url.pathname.startsWith('/schema/');
    const base = schema ? schemas : root;
    const name = decodeURIComponent(schema ? url.pathname.slice(8) : url.pathname.slice(1));
    const file = resolve(base, name);
    if (!file.startsWith(resolve(base) + sep)) throw new Error('invalid path');
    const info = await stat(file);
    if (!info.isFile()) throw new Error('not a file');
    res.writeHead(200, { 'content-length': info.size });
    if (req.method === 'HEAD') return res.end();
    res.end(await readFile(file));
  } catch { res.writeHead(404).end(); }
});
await new Promise((resolveListen) => server.listen(0, '127.0.0.1', resolveListen));
try {
  const base = `http://127.0.0.1:${server.address().port}`;
  const report = await check('slash-proc/smg-retro-go-sd', {
    base: `${base}/dist/`, schemaBase: `${base}/schema/`, hash: true,
  });
  for (const row of report.checks) console.log(`${row.level}: ${row.label}${row.detail ? ` — ${row.detail}` : ''}`);
  if (!report.summary.conformant) process.exitCode = 1;
} finally { server.close(); }
