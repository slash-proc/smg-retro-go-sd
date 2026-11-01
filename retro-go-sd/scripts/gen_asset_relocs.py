#!/usr/bin/env python3
"""Emit runtime ABS32 patch locations for both mapped XIP sections."""

from __future__ import annotations

import argparse
import struct
from pathlib import Path


SHT_SYMTAB = 2
SHT_REL = 9
SHF_ALLOC = 2
R_ARM_ABS32 = 2
ASSET_BASE = 0xBEEF0000


def c_string(blob: bytes, offset: int) -> str:
    end = blob.find(b"\0", offset)
    if end < 0:
        raise ValueError("unterminated ELF string")
    return blob[offset:end].decode("ascii", errors="strict")


def asset_reference_sites(path: Path) -> tuple[int, dict[str, list[int]]]:
    elf = path.read_bytes()
    if len(elf) < 52 or elf[:4] != b"\x7fELF" or elf[4:6] != b"\x01\x01":
        raise ValueError("expected a 32-bit little-endian ELF")
    header = struct.unpack_from("<16sHHIIIIIHHHHHH", elf)
    if header[2] != 40:
        raise ValueError("expected an ARM ELF")
    shoff, shentsize, shnum, shstrndx = header[6], header[11], header[12], header[13]
    if shentsize != 40 or not shnum or shstrndx >= shnum:
        raise ValueError("unsupported ELF section table")
    if shoff + shentsize * shnum > len(elf):
        raise ValueError("ELF section table extends beyond the file")

    sections = [
        struct.unpack_from("<IIIIIIIIII", elf, shoff + i * shentsize)
        for i in range(shnum)
    ]
    names_section = sections[shstrndx]
    names_blob = elf[names_section[4] : names_section[4] + names_section[5]]
    names = [c_string(names_blob, section[0]) for section in sections]
    try:
        assets_index = names.index(".xip_smg_assets")
        code_index = names.index(".xip_smg_code")
    except ValueError as exc:
        raise ValueError("ELF is missing an SMG XIP output section") from exc
    assets = sections[assets_index]
    code = sections[code_index]
    asset_base, asset_size = assets[3], assets[5]
    code_base, code_size = code[3], code[5]

    symbols_by_table: dict[int, list[tuple[int, int]]] = {}
    veneers: list[tuple[int, int]] = []
    for index, section in enumerate(sections):
        if section[1] != SHT_SYMTAB:
            continue
        if section[9] != 16 or section[5] % 16 or section[6] >= shnum:
            raise ValueError("malformed ELF32 symbol table")
        strings_section = sections[section[6]]
        strings = elf[strings_section[4] : strings_section[4] + strings_section[5]]
        symbols = []
        for offset in range(section[4], section[4] + section[5], 16):
            name, value, size, info, other, shndx = struct.unpack_from("<IIIBBH", elf, offset)
            symbols.append((value, shndx))
            if (info & 15) == 2 and size == 8 and name and shndx < shnum:
                if c_string(strings, name).endswith("_veneer"):
                    veneers.append((value & ~1, shndx))
        symbols_by_table[index] = symbols

    sites: dict[str, set[int]] = {
        "data": set(), "code": set(), "ram": set(),
        "code_self": set(), "ram_code": set(),
    }
    for section in sections:
        if section[1] != SHT_REL or section[7] >= shnum:
            continue
        target = sections[section[7]]
        if not target[2] & SHF_ALLOC or section[9] != 8 or section[5] % 8:
            continue
        symbols = symbols_by_table.get(section[6])
        if symbols is None:
            continue
        if section[4] + section[5] > len(elf):
            raise ValueError("relocation section extends beyond the ELF")
        for offset in range(section[4], section[4] + section[5], 8):
            location, info = struct.unpack_from("<II", elf, offset)
            if info & 0xFF != R_ARM_ABS32:
                continue
            symbol_index = info >> 8
            if symbol_index >= len(symbols):
                raise ValueError("relocation refers past its symbol table")
            value, _ = symbols[symbol_index]
            asset_target = asset_base <= (value & ~1) < asset_base + asset_size
            code_target = code_base <= (value & ~1) < code_base + code_size
            if asset_target or code_target:
                if location & 3:
                    raise ValueError(f"unaligned ABS32 relocation site {location:#x}")
                if not target[3] <= location < target[3] + target[5]:
                    raise ValueError(f"relocation site outside target section: {location:#x}")
                target_name = names[section[7]]
                if asset_target:
                    kind = "code" if target_name == ".xip_smg_code" else (
                        "data" if target_name == ".xip_smg_assets" else "ram"
                    )
                elif target_name == ".xip_smg_assets":
                    raise ValueError("assets cannot reference code loaded after them")
                else:
                    kind = "code_self" if target_name == ".xip_smg_code" else "ram_code"
                sites[kind].add(location)

    # GNU ld synthesizes far-call veneers without R_ARM_ABS32 records. A Thumb
    # veneer is exactly `ldr.w pc, [pc]` plus its destination literal. Recognize
    # that linker symbol and opcode instead of scanning unrelated RAM words.
    for address, index in veneers:
        section = sections[index]
        local = address - section[3]
        if local < 0 or local + 8 > section[5]:
            raise ValueError("linker veneer extends outside its section")
        offset = section[4] + local
        if elf[offset : offset + 4] != b"\x5f\xf8\x00\xf0":
            raise ValueError(f"unsupported linker veneer at {address:#x}")
        destination = struct.unpack_from("<I", elf, offset + 4)[0]
        if index not in (assets_index, code_index):
            # Moving a callee to ITCM introduces a fixed-address veneer among
            # the XIP veneers. ld may reorder them when the generated-table
            # object is added on the final link. Whitelist every recognized
            # loaded-code veneer literal, keeping this address set stable.
            # Runtime still range-checks each pointer before rebasing; fixed
            # AXI/ITCM destinations remain unchanged. Never scan opaque words.
            sites["ram_code"].add(address + 4)
        elif code_base <= (destination & ~1) < code_base + code_size:
            if index == assets_index:
                raise ValueError("asset section contains a code veneer")
            kind = "code_self" if index == code_index else "ram_code"
            sites[kind].add(address + 4)

    if not any(sites.values()):
        raise ValueError("ELF contains no ABS32 references into the asset XIP")
    return asset_size, {kind: sorted(entries) for kind, entries in sites.items()}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--elf", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--check", action="store_true", help="verify an existing table against this ELF")
    args = parser.parse_args()
    asset_size, sites = asset_reference_sites(args.elf)
    lines = [
        "/* Generated from final-link ABS32 relocation records. */",
        ".balign 4",
    ]
    for kind in ("data", "code", "ram", "code_self", "ram_code"):
        lines.append(f'.section .smg_asset_relocs.{kind}, "a", %progbits')
        lines.extend(f".word 0x{site:08x}" for site in sites[kind])
    lines.append("")
    generated = "\n".join(lines)
    if args.check:
        if args.out.read_text() != generated:
            raise ValueError("final ELF relocation sites differ from the generated table")
    else:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(generated)
    print(
        f"asset XIP {asset_size} bytes; emitted exact ABS32 sites: "
        + ", ".join(f"{kind}={len(entries)}" for kind, entries in sites.items())
    )


if __name__ == "__main__":
    main()
