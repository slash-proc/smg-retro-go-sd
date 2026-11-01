#!/usr/bin/env python3
"""Build a local dist directory and GWRG offline bundle from this build."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path

from elf_relocations import elf_payloads


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
    commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    manifest = DIST / "manifest.json"
    shutil.rmtree(DIST, ignore_errors=True)
    DIST.mkdir(parents=True, exist_ok=True)

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
        str(BUILD / "smg_core.elf"),
        "--cover",
        str(BUILD / "cover.jpg"),
        "--tag",
        "local",
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
        BUILD / "smg_core.elf",
        BUILD / "cover.jpg",
    ):
        shutil.copyfile(source, DIST / source.name)

    subprocess.run(
        [
            sys.executable,
            str(BUNDLE_BUILDER),
            "--manifest",
            str(manifest),
            "--dir",
            str(DIST),
            "--out",
            str(BUNDLE),
        ],
        cwd=PROJECT,
        check=True,
    )
    print(f"Local dist: {DIST}")


if __name__ == "__main__":
    main()
