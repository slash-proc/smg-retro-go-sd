#!/usr/bin/env python3
"""Build a local dist directory and GWRG offline bundle from this build."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

from elf_relocations import elf_payloads
from make_manifest import GWHB_V1_SIZE, pick_gwhb


ROOT = Path(__file__).resolve().parents[2]
PROJECT = ROOT / "retro-go-sd"
BUILD = PROJECT / "build/smg"
DIST = BUILD / "dist/local"
BUNDLE = BUILD / "local-bundle/smg-local-bundle.zip"
MANIFEST_BUILDER = PROJECT / "scripts/make_manifest.py"
BUNDLE_BUILDER = PROJECT / "scripts/make_bundle.py"


def add_exact_relocations(manifest: dict, elf: Path) -> None:
    """Publish source-relative ABS32 sites for static and converter-produced XIPs."""
    payloads, sites = elf_payloads(elf)
    tables = {}
    for source_name, records in sites.items():
        groups = []
        for target_name in sorted(set(records.values())):
            groups.append({
                "target": target_name,
                "relocBase": payloads[target_name]["base"],
                "offsets": sorted(offset for offset, target in records.items() if target == target_name),
            })
        tables[source_name] = {"method": "exact-relocation-sites", "version": 1, "groups": groups}
    for target in manifest["targets"]:
        for artifact in target["artifacts"]:
            if artifact["filename"] in tables:
                artifact.update(mapped=True, relocations=tables[artifact["filename"]])
    for tool in manifest["tools"]:
        for output in tool["outputs"]:
            if output.get("filename") in tables:
                size = len(payloads[output["filename"]]["raw"])
                bounds = [value for value in (output.get("maxBytes"), tool.get("limits", {}).get("maxOutputBytes")) if value is not None]
                limit = min(bounds) if bounds else None
                if limit is not None and size > limit:
                    raise ValueError(f"{output['filename']}: native size {size} exceeds declared processor/output budget {limit}")
                output.update(mapped=True, relocations=tables[output["filename"]])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", default="local", help="release tag, or local for development")
    parser.add_argument("--dist-dir", type=Path, default=DIST, help="empty output directory for release files")
    parser.add_argument("--bundle", type=Path, default=BUNDLE, help="offline bundle output path")
    args = parser.parse_args()
    if args.tag != "local":
        match = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", args.tag)
        if not match:
            parser.error("--tag must be local or a vMAJOR.MINOR.PATCH release")
        data = (PROJECT / "SMG.bin").read_bytes()
        header_length = int.from_bytes(data[6:8], "little")
        metadata_size = pick_gwhb(data, header_length, PROJECT / "SMG.bin")
        version_offset = 8 + (60 if metadata_size == GWHB_V1_SIZE else 104)
        if tuple(data[version_offset:version_offset + 3]) != tuple(map(int, match.groups())):
            parser.error(f"SMG.bin version does not match {args.tag}; rebuild with CORE_VERSION={args.tag}")
    commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    dist = args.dist_dir.resolve()
    if dist == DIST.resolve():
        shutil.rmtree(dist, ignore_errors=True)
    elif dist.exists() and any(dist.iterdir()):
        parser.error("--dist-dir must be empty")
    dist.mkdir(parents=True, exist_ok=True)
    manifest = dist / "manifest.json"
    # A full ELF includes the generated game assets. Publish debug symbols with
    # allocated sections represented as NOBITS, while deriving the relocation
    # tables from the original ELF below.
    symbols = dist / "smg_core.elf"
    subprocess.run([
        os.environ.get("OBJCOPY", "arm-none-eabi-objcopy"), "--only-keep-debug",
        str(BUILD / "smg_core.elf"), str(symbols),
    ], check=True)

    command = [
        sys.executable,
        str(MANIFEST_BUILDER),
        "--bin",
        str(PROJECT / "SMG.bin"),
        "--artifact",
        str(PROJECT / "smg_code.xip"),
        "--wasm",
        str(ROOT / "tools/wasm_converter/web/smg_asset_converter.wasm"),
        "--elf",
        str(symbols),
        "--cover",
        str(BUILD / "cover.jpg"),
        "--tag",
        args.tag,
        "--repo",
        "slash-proc/smg-retro-go-sd",
        "--commit",
        commit,
        "--out",
        str(manifest),
    ]
    subprocess.run(command, cwd=PROJECT, check=True)
    prepared = json.loads(manifest.read_text())
    add_exact_relocations(prepared, BUILD / "smg_core.elf")
    manifest.write_text(json.dumps(prepared, indent=2) + "\n")

    for source in (
        PROJECT / "SMG.bin",
        PROJECT / "smg_code.xip",
        ROOT / "tools/wasm_converter/web/smg_asset_converter.wasm",
        BUILD / "cover.jpg",
    ):
        shutil.copyfile(source, dist / source.name)

    # The Pages host verifies the complete ISO before conversion. This helper
    # is a release attachment, not an installable artifact in the GWRG bundle.
    verifier = ROOT / "tools/wasm_converter/web/smg_iso_verifier.wasm"
    shutil.copyfile(verifier, dist / verifier.name)

    subprocess.run(
        [
            sys.executable,
            str(BUNDLE_BUILDER),
            "--manifest",
            str(manifest),
            "--dir",
            str(dist),
            "--out",
            str(args.bundle),
        ],
        cwd=PROJECT,
        check=True,
    )
    print(f"Distribution files: {dist}")


if __name__ == "__main__":
    main()
