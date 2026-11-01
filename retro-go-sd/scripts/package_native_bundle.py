#!/usr/bin/env python3
"""Package already-converted local assets for repeatable runtime tests.

Uses the ordinary GWRG installer and exact relocation declarations. The large
base ISO and WASM conversion are unnecessary when only runtime code changes.
"""
import argparse
import hashlib
import json
import shutil
from pathlib import Path
from elftools.elf.elffile import ELFFile

from make_bundle import build
from package_local_bundle import add_exact_relocations, ROOT, PROJECT, BUILD


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--elf", type=Path, help="Archived ELF; requires its matching --homebrew-bin")
    parser.add_argument("--homebrew-bin", type=Path, help="Packed homebrew matching --elf")
    args = parser.parse_args()
    if (args.elf is None) != (args.homebrew_bin is None):
        parser.error("--elf and --homebrew-bin must be supplied together")
    elf = args.elf or BUILD / "smg_core.elf"
    homebrew = args.homebrew_bin or PROJECT / "SMG.bin"
    with homebrew.open("rb") as stream:
        if stream.read(4) != b"GWHB":
            parser.error("--homebrew-bin must be the packed GWHB executable (SMG.bin), not smg_core.bin")
    directory = args.out.resolve().parent / "native-inputs"
    directory.mkdir(parents=True, exist_ok=True)
    archived = {}
    if args.elf:
        with elf.open("rb") as stream:
            image = ELFFile(stream)
            for name, section in (("smg_code.xip", ".xip_smg_code"), ("smg_assets.xip", ".xip_smg_assets")):
                payload = image.get_section_by_name(section)
                if payload is None:
                    raise ValueError(f"Missing {section} in {elf}")
                archived[name] = payload.data()
    # The generated manifest supplies the project's normal identity and ABI.
    manifest = json.loads((BUILD / "dist/local/manifest.json").read_text())
    manifest["tools"] = []
    for target in manifest["targets"]:
        target.pop("uses", None)
        target["artifacts"] = []
        for name in ("SMG.bin", "smg_code.xip", "smg_assets.xip"):
            destination = directory / name
            if name in archived:
                destination.write_bytes(archived[name])
            else:
                shutil.copyfile(homebrew if name == "SMG.bin" else PROJECT / name, destination)
            source = destination
            target["artifacts"].append({"filename": name, "url": name,
                "bytes": source.stat().st_size,
                "sha256": hashlib.sha256(source.read_bytes()).hexdigest()})
        for symbol in target.get("symbols", []):
            source = elf
            symbol.update(bytes=source.stat().st_size,
                sha256=hashlib.sha256(source.read_bytes()).hexdigest())
            shutil.copyfile(source, directory / Path(symbol["url"]).name)
    if manifest.get("cover"):
        shutil.copyfile(BUILD / "cover.jpg", directory / manifest["cover"]["url"])
    add_exact_relocations(manifest, elf)
    path = directory / "manifest.json"
    path.write_text(json.dumps(manifest, indent=2) + "\n")
    build(manifest_path=path, src_dir=directory, out=args.out)


if __name__ == "__main__":
    main()
