#!/usr/bin/env python3
"""Audit compiled stage materials, UV limits and linked mip pixels.

Read-only diagnostic. Original J3D opacity checks are deliberately conservative:
both alpha comparisons must be ALWAYS and blending must be disabled. Findings
identify affected assets/faces; they do not claim those faces are on screen.
"""
import argparse
import json
import struct
import sys
from pathlib import Path

from elftools.elf.elffile import ELFFile
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools/asset_construction"))
from common import Dump, read_pak, u16, u32  # noqa: E402


def source_alpha_findings(constructed, original):
    metadata = json.loads(read_pak(ROOT / "asset_construction/textures/textures.pak")["meta.json"])
    dump = Dump(str(original))
    models = {}
    policies = {}
    stage = None
    for line in (ROOT / "assets/assets.cfg").read_text().splitlines():
        fields = line.split("#", 1)[0].split()
        if not fields:
            continue
        if fields[0] in ("stage", "figure"):
            stage = fields[1]
        elif fields[0] == "end":
            stage = None
        elif stage and fields[0] == "texture_alpha":
            policies[stage, fields[1]] = fields[2]
    findings = []
    for image in metadata["images"]:
        recipe = image["pred"]
        source = recipe.get("src")
        paths = [f["path"] for f in image["files"] if f["path"].startswith("assets/")]
        if source is None or not paths or recipe.get("dither") or "lumcut" in recipe:
            continue
        archive, inner, kind, material, _ = metadata["sources"][source]
        if kind != "tev":
            continue
        key = archive, inner
        if key not in models:
            data = dump.load(archive + ":" + inner)
            at = 32
            sections = {}
            for _ in range(u32(data, 12)):
                sections[data[at:at + 4]] = at
                at += u32(data, at + 4)
            base = sections.get(b"MAT3")
            if base is None:
                continue
            tables = [base + u32(data, base + 12 + i * 4)
                      if u32(data, base + 12 + i * 4) else None for i in range(30)]
            models[key] = data, tables
        data, tables = models[key]
        at = tables[0] + u16(data, tables[1] + material * 2) * 0x14C
        alpha_index, blend_index = u16(data, at + 0x146), u16(data, at + 0x148)
        if not tables[24] or not tables[25] or 65535 in (alpha_index, blend_index):
            continue
        ac = list(data[tables[24] + alpha_index * 8:tables[24] + alpha_index * 8 + 5])
        blend = list(data[tables[25] + blend_index * 4:tables[25] + blend_index * 4 + 4])
        opaque = ac[0] == ac[3] == 7 and ac[2] in (0, 1) and blend[0] == 0
        if not opaque:
            continue
        for relative in paths:
            with Image.open(constructed / relative) as png:
                rgba = png.convert("RGBA")
            alpha = list(rgba.getchannel("A").getdata())
            transparent = sum(a < 128 for a in alpha)
            if transparent:
                findings.append(dict(path=relative, source=metadata["sources"][source],
                                     alpha_compare=ac, blend=blend,
                                     transparent_pixels=transparent, pixels=len(alpha),
                                     alpha_range=[min(alpha), max(alpha)],
                                     compiler_alpha_policy=policies.get((Path(relative).parts[2], Path(relative).name))))
    return findings


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assets", type=Path, default=ROOT / "build/assets")
    parser.add_argument("--constructed", type=Path, default=ROOT / "build/constructed")
    parser.add_argument("--original", type=Path, help="Extracted ISO files directory; enables J3D opacity checks")
    parser.add_argument("--elf", type=Path, required=True)
    args = parser.parse_args()
    names = [line.split()[1] for line in (ROOT / "assets/assets.cfg").read_text().splitlines()
             if line.startswith("stage ")]
    rows = []
    checked = 0
    figure_rows = []
    figure_materials = 0
    special_figure_banks = []
    with args.elf.open("rb") as stream:
        elf = ELFFile(stream)
        section = elf.get_section_by_name(".xip_smg_assets")
        base, linked = section["sh_addr"], section.data()
        symbols = elf.get_section_by_name(".symtab")

        def read(address, size):
            offset = address - base
            if not 0 <= offset <= len(linked) - size:
                raise ValueError(f"Linked asset read out of bounds: {address:#x}+{size}")
            return linked[offset:offset + size]

        for name in names:
            source = (args.assets / f"stage_{name}.bin").read_bytes()
            points, faces, materials = struct.unpack_from("<3I", source)
            point_at = struct.unpack_from("<I", source, 28)[0]
            triangle_at, texture_at = struct.unpack_from("<2I", source, 32)
            origin = symbols.get_symbol_by_name(f"stage_{name}")[0]["st_value"]
            table = (origin + struct.unpack("<I", read(origin + 36, 4))[0]) & 0xFFFFFFFF
            empty = []
            for material in range(materials):
                raw = source[texture_at + material * 2048:texture_at + (material + 1) * 2048]
                assert len(raw) == 2048
                first, second = struct.unpack("<2I", read(table + material * 8, 8))
                assert read(first, 1024) == raw[:1024], (name, material, "mip0")
                for row in range(16):
                    assert read(second + row * 64, 32) == raw[1024 + row * 64:1056 + row * 64], (name, material, "mip1", row)
                checked += 1
                if not any(raw[:1024]):
                    empty.append(material)
            overflow = saturated = invisible = invalid_alpha = 0
            winding_disagreements = zero_area = 0
            for face in range(faces):
                at = triangle_at + face * 24
                vertices = [struct.unpack_from("<Hhh", source, at + i * 6) for i in range(3)]
                texture, flags = struct.unpack_from("<HB", source, at + 18)
                material = texture & 1023
                assert material < materials and all(v[0] < points for v in vertices), (name, face)
                a, b, c = [struct.unpack_from("<3h", source, point_at + vertex[0] * 6)
                           for vertex in vertices]
                ab = [a[k] - b[k] for k in range(3)]
                cb = [c[k] - b[k] for k in range(3)]
                cross = [ab[1] * cb[2] - ab[2] * cb[1],
                         ab[2] * cb[0] - ab[0] * cb[2],
                         ab[0] * cb[1] - ab[1] * cb[0]]
                normal = struct.unpack_from("<3b", source, at + 21)
                winding_disagreements += sum(x * y for x, y in zip(cross, normal)) < 0
                zero_area += not any(cross)
                overflow += any(not -32768 <= vertices[i][c] + vertices[(i + 1) % 3][c] <= 32767
                                for i in range(3) for c in (1, 2))
                saturated += any(v in (-32768, 32767) for vertex in vertices for v in vertex[1:])
                if material in empty:
                    invisible += bool(flags & 8)
                    invalid_alpha += not bool(flags & 8)
            obj = args.constructed / "assets/stages" / name / f"{name}.obj"
            outside = 0
            if obj.exists():
                outside = sum(not -32768 <= float(value) * 512 <= 32767
                              for line in obj.read_text().splitlines() if line.startswith("vt ")
                              for value in line.split()[1:3])
            rows.append(dict(stage=name, faces=faces, materials=materials,
                             empty_materials=empty, invisible_faces=invisible,
                             empty_opaque_faces=invalid_alpha,
                             normal_winding_disagreements=winding_disagreements,
                             zero_area_faces=zero_area,
                             uv_midpoint_overflow_faces=overflow,
                             uv_saturated_faces=saturated, source_uv_coordinates_outside_range=outside))
        for line in (ROOT / "assets/assets.cfg").read_text().splitlines():
            if not line.startswith("figure "):
                continue
            name = line.split()[1]
            symbol = "figure_" + name
            source = (args.assets / (symbol + ".bin")).read_bytes()
            faces, materials = struct.unpack_from("<2H", source, 2)
            triangle_at = struct.unpack_from("<I", source, 16)[0]
            texture_at = struct.unpack_from("<I", source, 24)[0]
            # A few custom procedural/shared banks use a different source
            # representation. Do not treat them as ordinary inline mip banks.
            if texture_at + materials * 2048 > len(source):
                special_figure_banks.append(name)
                continue
            origin = symbols.get_symbol_by_name(symbol)[0]["st_value"]
            table = (origin + struct.unpack("<I", read(origin + 24, 4))[0]) & 0xFFFFFFFF
            empty = []
            for material in range(materials):
                raw = source[texture_at + material * 2048:texture_at + (material + 1) * 2048]
                first, second = struct.unpack("<2I", read(table + material * 8, 8))
                assert read(first, 1024) == raw[:1024], (name, material, "figure mip0")
                for row in range(16):
                    assert read(second + row * 64, 32) == raw[1024 + row * 64:1056 + row * 64], (name, material, "figure mip1", row)
                figure_materials += 1
                if not any(raw[:1024]):
                    empty.append(material)
            affected = sum(struct.unpack_from("<H", source, triangle_at + face * 22 + 18)[0] in empty
                           for face in range(faces))
            figure_rows.append(dict(figure=name, materials=materials, faces=faces,
                                    empty_materials=empty, empty_material_faces=affected))
    result = dict(stages=rows, linked_main_materials_checked=checked,
                  linked_mip_pixels_exact=True, figures=figure_rows,
                  linked_figure_materials_checked=figure_materials,
                  special_figure_banks_not_checked=special_figure_banks)
    if args.original:
        result["opaque_source_with_transparent_output"] = source_alpha_findings(args.constructed, args.original)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
