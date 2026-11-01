#!/usr/bin/env python3
"""Compare SMG's immutable texture pool with a virtual NOR image.

The pool has no relocation sites, unlike the rest of the asset XIP. The flash
and asset must be from the same build; obtain the mapped base from the running
game or the installer. This diagnostic never writes to either input.
"""
import argparse
import hashlib
import json
from pathlib import Path

from elftools.elf.elffile import ELFFile


def pool_bounds(elf_path):
    with elf_path.open("rb") as f:
        elf = ELFFile(f)
        section = elf.get_section_by_name(".xip_smg_assets")
        symbols = elf.get_section_by_name(".symtab")
        if section is None or symbols is None:
            raise ValueError("ELF is missing its asset section or symbol table")
        section_index = elf.get_section_index(".xip_smg_assets")
        def offset(name):
            matches = symbols.get_symbol_by_name(name) or []
            matches = [s for s in matches if s["st_shndx"] == section_index]
            if len(matches) != 1:
                raise ValueError(f"expected one asset symbol {name}")
            return matches[0]["st_value"] - section["sh_addr"]
        start, end = offset("texture_mip_raw_pool"), offset("texture_refs_0")
        if not 0 <= start < end <= section["sh_size"]:
            raise ValueError("invalid texture-pool bounds")
        return start, end


def audit(elf, asset, flash, flash_offset):
    start, end = pool_bounds(elf)
    if flash_offset < 0 or asset.stat().st_size < end or flash.stat().st_size < flash_offset + end:
        raise ValueError("asset or flash image is too short for the pool")
    expected_hash, actual_hash = hashlib.sha256(), hashlib.sha256()
    changed = []
    with asset.open("rb") as a, flash.open("rb") as b:
        pos = start
        while pos < end:
            address = flash_offset + pos
            length = min(end - pos, 65536 - (address % 65536))
            a.seek(pos)
            b.seek(address)
            expected, actual = a.read(length), b.read(length)
            if len(expected) != length or len(actual) != length:
                raise ValueError("short read")
            expected_hash.update(expected)
            actual_hash.update(actual)
            count = sum(x != y for x, y in zip(expected, actual))
            if count:
                changed.append({"flash_offset": address, "asset_offset": pos,
                                "bytes": length, "different_bytes": count,
                                "all_erased": actual.count(255) == length})
            pos += length
    return {"pool_asset_offset": start, "pool_bytes": end - start,
            "asset_flash_offset": flash_offset,
            "expected_sha256": expected_hash.hexdigest(),
            "actual_sha256": actual_hash.hexdigest(), "different_blocks": changed}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("elf", type=Path)
    parser.add_argument("asset", type=Path)
    parser.add_argument("flash", type=Path)
    parser.add_argument("flash_offset", type=lambda x: int(x, 0),
                        help="asset XIP offset from the beginning of the flash image")
    args = parser.parse_args()
    result = audit(args.elf, args.asset, args.flash, args.flash_offset)
    print(json.dumps(result, indent=2))
    raise SystemExit(bool(result["different_blocks"]))
