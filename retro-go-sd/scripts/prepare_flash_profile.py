#!/usr/bin/env python3
"""Audit SMG mapped payloads against ELF exact relocation records in GWProv.

Normal installers use the bundle's declared exact tables. The optional --apply
mode repairs legacy installs from preserved sources without changing FrogFS
placement or firmware; routine validation is read-only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import zlib
from pathlib import Path

from elf_relocations import elf_payloads



def prepare(profile_path: str, *, apply: bool) -> dict:
    from gwprov.filesystem import _sync_flash_map_component
    from gwprov.profiles import DeviceProfile
    from gwprov.vendor.retrogo_sd.scripts.frogfs_pico8_ro import find_uncompressed_file_payload

    profile = DeviceProfile.load(profile_path)
    metadata = json.loads((profile.root / "provision.json").read_text())
    matches = [
        project for project in metadata["projects"].values()
        if project.get("repo") == "slash-proc/smg-retro-go-sd"
    ]
    if len(matches) != 1:
        raise ValueError("profile must contain exactly one SMG project")
    project = matches[0]
    symbols = [profile.root / "debug" / name for name in project["symbols"]]
    if len(symbols) != 1:
        raise ValueError("SMG project must have one final ELF")
    payloads, sites = elf_payloads(symbols[0])
    offset = metadata["layout"]["frogfsOffset"]
    size = metadata["layout"]["frogfsBytes"]
    original = profile.extflash.read_bytes()
    image = bytearray(original[offset : offset + size])
    if len(image) != size or zlib.crc32(image[:-4]) != struct.unpack_from("<I", image, size - 4)[0]:
        raise ValueError("profile FrogFS has an invalid length or CRC")
    for item in project["mapped"]:
        name = Path(item["path"]).name
        if name not in payloads:
            continue
        payload = payloads[name]
        raw_source = (profile.root / item["source"]).resolve()
        if profile.root.resolve() not in raw_source.parents:
            raise ValueError("preserved sidecar escapes the profile")
        if raw_source.read_bytes() != payload["raw"]:
            raise ValueError(f"preserved {name} does not match the project ELF")
        found = find_uncompressed_file_payload(image, item["path"])
        if found is None or found[1] != len(payload["raw"]):
            raise ValueError(f"missing, compressed, or wrong-size mapped payload: {name}")
        payload["offset"], _ = found
        payload["runtime"] = 0x90000000 + offset + payload["offset"]
    if any("runtime" not in payload for payload in payloads.values()):
        raise ValueError("profile must declare both SMG sidecars as mapped")

    report = {"profile": str(profile.root), "applied": False, "payloads": {}}
    for name, payload in payloads.items():
        expected = bytearray(payload["raw"])
        records = sites[name]
        for local, target_name in records.items():
            target = payloads[target_name]
            value = struct.unpack_from("<I", expected, local)[0]
            if not target["base"] <= (value & ~1) < target["base"] + len(target["raw"]):
                raise ValueError(f"ABS32 at {name}+{local:#x} is outside its target")
            relocated = (value + target["runtime"] - target["base"]) & 0xFFFFFFFF
            struct.pack_into("<I", expected, local, relocated)
        position = payload["offset"]
        current = image[position : position + len(expected)]
        differing = [
            i for i in range(0, len(expected), 4)
            if current[i : i + 4] != expected[i : i + 4]
        ]
        # Every byte outside an actual pointer must remain identical to the ELF.
        for i in range(0, len(expected), 4):
            if i not in records and expected[i : i + 4] != payload["raw"][i : i + 4]:
                raise AssertionError("non-pointer bytes changed")
        report["payloads"][name] = {
            "runtime": f"0x{payload['runtime']:08x}",
            "pointer_sites": len(records),
            "different_words": len(differing),
            "non_pointer_differences": sum(i not in records for i in differing),
            "sha256": hashlib.sha256(expected).hexdigest(),
        }
        image[position : position + len(expected)] = expected
    struct.pack_into("<I", image, size - 4, zlib.crc32(image[:-4]))
    updated = original[:offset] + image + original[offset + size :]
    if updated[:offset] != original[:offset] or updated[offset + size :] != original[offset + size :]:
        raise AssertionError("data outside FrogFS changed")
    report["frogfs_sha256"] = hashlib.sha256(image).hexdigest()
    if apply:
        # Keep the deployable component and its hash in sync with the virtual image.
        profile.extflash.write_bytes(updated)
        try:
            _sync_flash_map_component(
                profile.root, offset=offset, data=bytes(image),
                label="Retro-Go FrogFS", filename="smg-frogfs.bin",
            )
        except BaseException:
            profile.extflash.write_bytes(original)
            raise
        report["applied"] = True
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profile", help="GWProv profile name or directory")
    parser.add_argument("--apply", action="store_true", help="write the reconstructed FrogFS to the profile")
    args = parser.parse_args()
    print(json.dumps(prepare(args.profile, apply=args.apply), indent=2))


if __name__ == "__main__":
    main()
