#!/usr/bin/env python3
"""Embed fixed native stage outputs in each compact stage recipe pack.

Stage OBJ/MTL files are reconstructed from ISO archives. Galaxy descriptors
and seven model-rendered sky PNGs are fixed outputs for the manifest-selected
RMGE01 disc; keeping their bytes beside the line recipes avoids porting the
native Galaxy writer and sky renderer into the WASM converter.
"""

import argparse
import json
import lzma
import struct
from pathlib import Path

MODEL_SKY_STAGES = {
    "astrodome",
    "beachbowl",
    "buoybase",
    "deepdark",
    "freezeflame",
    "goodegg",
    "gustygarden",
}


def read_pack(path: Path):
    raw = lzma.decompress(path.read_bytes())
    index_size = struct.unpack_from("<I", raw)[0]
    index_end = 4 + index_size
    index = json.loads(raw[4:index_end])
    payload = raw[index_end:]
    entries = {}
    for name, offset, size in index:
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
    parser.add_argument("--recipes", type=Path, default=Path("asset_construction/stages"))
    parser.add_argument("--assets", type=Path, default=Path("build/constructed/assets/stages"))
    args = parser.parse_args()
    added = 0
    for pack_path in sorted(args.recipes.glob("*.pak")):
        entries = read_pack(pack_path)
        meta = json.loads(entries["meta"])
        stage = pack_path.stem
        for row in meta["files"]:
            name = row[0]
            if not name.endswith((".galaxy", ".stage")):
                continue
            key = f"{name}/native"
            native_path = args.assets / stage / name
            content = native_path.read_bytes()
            if key in entries and entries[key] != content:
                raise ValueError(f"descriptor changed since it was embedded: {native_path}")
            if key not in entries:
                entries[key] = content
                added += 1
        if stage in MODEL_SKY_STAGES:
            key = "sky.png/native"
            native_path = args.assets / stage / "sky.png"
            content = native_path.read_bytes()
            if key in entries and entries[key] != content:
                raise ValueError(f"sky image changed since it was embedded: {native_path}")
            if key not in entries:
                entries[key] = content
                added += 1
        write_pack(pack_path, entries)
    print(f"Embedded {added} fixed native stage outputs in {args.recipes}")


if __name__ == "__main__":
    main()
