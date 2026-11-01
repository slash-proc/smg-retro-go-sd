"""Read mapped payloads and their ABS32 relocation sites from the final ARM ELF."""

from __future__ import annotations

import struct
from pathlib import Path


def elf_payloads(path: Path) -> tuple[dict, dict]:
    elf = path.read_bytes()
    if elf[:6] != b"\x7fELF\x01\x01":
        raise ValueError("expected a little-endian ELF32")
    header = struct.unpack_from("<16sHHIIIIIHHHHHH", elf)
    if header[2] != 40 or header[11] != 40:
        raise ValueError("expected an ARM ELF32 with standard section headers")
    sections = [
        struct.unpack_from("<IIIIIIIIII", elf, header[6] + i * header[11])
        for i in range(header[12])
    ]
    strings = sections[header[13]]
    names_blob = elf[strings[4] : strings[4] + strings[5]]
    names = [names_blob[s[0] :].split(b"\0", 1)[0].decode() for s in sections]
    selected = {
        names.index(".xip_smg_code"): "smg_code.xip",
        names.index(".xip_smg_assets"): "smg_assets.xip",
    }
    payloads = {
        filename: {
            "base": sections[index][3],
            "raw": elf[sections[index][4] : sections[index][4] + sections[index][5]],
        }
        for index, filename in selected.items()
    }
    sites = {filename: {} for filename in selected.values()}
    for section in sections:
        if section[1] != 9 or section[7] not in selected:  # SHT_REL
            continue
        if section[9] != 8 or section[5] % 8:
            raise ValueError("malformed ELF relocation section")
        source_name = selected[section[7]]
        symbols = sections[section[6]]
        for offset in range(section[4], section[4] + section[5], 8):
            address, info = struct.unpack_from("<II", elf, offset)
            if info & 255 != 2:  # R_ARM_ABS32; self branches are PC relative.
                continue
            _, value, _, _, _, target_index = struct.unpack_from(
                "<IIIBBH", elf, symbols[4] + (info >> 8) * 16
            )
            if target_index not in selected:
                continue
            target_name = selected[target_index]
            target = payloads[target_name]
            if not target["base"] <= (value & ~1) < target["base"] + len(target["raw"]):
                raise ValueError("mapped relocation symbol is outside its section")
            local = address - payloads[source_name]["base"]
            if local & 3 or not 0 <= local <= len(payloads[source_name]["raw"]) - 4:
                raise ValueError("mapped ABS32 site is unaligned or out of range")
            previous = sites[source_name].setdefault(local, target_name)
            if previous != target_name:
                raise ValueError("mapped ABS32 site has conflicting targets")
    if any(not records for records in sites.values()):
        raise ValueError("ELF is missing mapped ABS32 relocation records")
    return payloads, sites

