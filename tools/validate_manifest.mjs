#!/usr/bin/env node
import { readFile } from 'node:fs/promises';
import { validate } from './dist-spec/site/validate.js';

const [manifestPath, schemaPath] = process.argv.slice(2);
if (!manifestPath || !schemaPath) throw new Error('usage: node tools/validate_manifest.mjs MANIFEST SCHEMA');
const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
const schema = JSON.parse(await readFile(schemaPath, 'utf8'));
const errors = validate(manifest, schema);
if (errors.length) {
  for (const error of errors) console.error(error);
  process.exitCode = 1;
} else console.log('Manifest matches the distribution schema');
