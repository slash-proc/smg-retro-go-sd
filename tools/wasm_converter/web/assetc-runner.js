import {
  WASI,
  WASIProcExit,
  File,
  Directory,
  OpenFile,
  PreopenDirectory,
  ConsoleStdout,
} from "./vendor/wasi/index.js";

function insertFile(root, path, bytes) {
  const parts = path.split("/");
  if (!parts.length || parts.some((part) => !part || part === "." || part === "..")) {
    throw new Error(`Invalid asset compiler input path: ${path}`);
  }
  let directory = root;
  for (const part of parts.slice(0, -1)) {
    let child = directory.contents.get(part);
    if (child === undefined) {
      child = new Directory(new Map());
      directory.contents.set(part, child);
    }
    if (!(child instanceof Directory)) {
      throw new Error(`Asset compiler path crosses a file: ${path}`);
    }
    directory = child;
  }
  const name = parts.at(-1);
  if (directory.contents.has(name)) throw new Error(`Duplicate asset compiler input: ${path}`);
  const file = new File([]);
  file.data = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  file.readonly = true;
  directory.contents.set(name, file);
}

function collectFiles(directory, prefix = "", files = new Map()) {
  for (const [name, entry] of directory.contents) {
    const path = prefix ? `${prefix}/${name}` : name;
    if (entry instanceof Directory) collectFiles(entry, path, files);
    else if (entry instanceof File) files.set(path, entry.data);
  }
  return files;
}

/** Run the canonical C asset compiler against an in-memory relative file tree. */
export async function runAssetCompiler(wasmBytes, inputFiles) {
  const input = new Directory(new Map());
  for (const [path, bytes] of inputFiles) insertFile(input, path, bytes);
  const output = new Directory(new Map());
  const root = new Directory(new Map([["input", input], ["output", output]]));
  const diagnostics = [];
  const log = ConsoleStdout.lineBuffered((line) => {
    diagnostics.push(line);
    if (diagnostics.length > 80) diagnostics.shift();
  });
  const wasi = new WASI(
    ["assetc", "input", "output"],
    [],
    [new OpenFile(new File([])), log, log, new PreopenDirectory(".", root.contents)],
  );
  const { instance } = await WebAssembly.instantiate(wasmBytes, {
    wasi_snapshot_preview1: wasi.wasiImport,
  });
  let exitCode = 0;
  try {
    exitCode = wasi.start(instance);
  } catch (error) {
    if (!(error instanceof WASIProcExit)) throw error;
    exitCode = error.code;
  }
  if (exitCode !== 0) {
    throw new Error(`Asset compiler exited with ${exitCode}:\n${diagnostics.join("\n")}`);
  }
  return collectFiles(output);
}
