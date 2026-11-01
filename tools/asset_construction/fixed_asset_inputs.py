#!/usr/bin/env python3
"""Embed fixed native asset-compiler inputs in the UI recipe pack.

These UI and text-atlas binaries are generated from the version-locked RMGE01
source catalog by the existing native construction pipeline. Keeping their
finished bytes in the converter avoids duplicating the layout/font renderer in
WASM while retaining the canonical C asset compiler and linker layout.
"""

import argparse
import json
import lzma
import struct
from pathlib import Path


def read_pack(path: Path):
    raw = lzma.decompress(path.read_bytes())
    index_size = struct.unpack_from("<I", raw)[0]
    index_end = 4 + index_size
    rows = json.loads(raw[4:index_end])
    payload = raw[index_end:]
    entries = {}
    for name, offset, size in rows:
        if name in entries or offset < 0 or size < 0 or offset + size > len(payload):
            raise ValueError(f"invalid pack entry in {path}: {name}")
        entries[name] = payload[offset:offset + size]
    return entries


def write_pack(path: Path, entries):
    index = []
    payload = bytearray()
    for name, content in entries.items():
        index.append([name, len(payload), len(content)])
        payload.extend(content)
    encoded_index = json.dumps(index, separators=(",", ":"), ensure_ascii=False).encode()
    raw = struct.pack("<I", len(encoded_index)) + encoded_index + payload
    path.write_bytes(lzma.compress(raw, format=lzma.FORMAT_XZ, preset=9))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", type=Path, default=Path("assets/assets.cfg"))
    parser.add_argument("--assets", type=Path, default=Path("build/constructed/assets"))
    parser.add_argument(
        "--pack", type=Path, default=Path("asset_construction/ui_headers/ui_headers.pak")
    )
    args = parser.parse_args()

    entries = read_pack(args.pack)
    added = 0
    for line in args.config.read_text().splitlines():
        fields = line.split("#", 1)[0].split()
        if not fields or fields[0] != "blob":
            continue
        if len(fields) != 3:
            raise ValueError(f"malformed blob entry in {args.config}: {line}")
        asset_path = fields[2]
        key = f"asset:{asset_path}"
        content = (args.assets / asset_path).read_bytes()
        if key in entries and entries[key] != content:
            raise ValueError(f"embedded asset changed since it was recorded: {asset_path}")
        if key not in entries:
            entries[key] = content
            added += 1
    write_pack(args.pack, entries)
    print(f"Embedded {added} fixed asset-compiler inputs in {args.pack}")


if __name__ == "__main__":
    main()
