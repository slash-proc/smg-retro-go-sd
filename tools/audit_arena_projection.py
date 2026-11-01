#!/usr/bin/env python3
"""Inspect captured stage draw entries against the installed flash/texture cache.

This diagnostic omits actors and water tint, and uses floating-point affine
interpolation. It is intended to locate a bad pixel's entry/material, not as a
bit-exact renderer oracle. Capture files must come from the same build/profile.
"""
import argparse
import json
import mmap
import struct
from pathlib import Path

import numpy as np
from PIL import Image
from elftools.elf.elffile import ELFFile


def render(directory, flash_path, elf_path, asset_base, perspective=False):
    state = json.loads((directory / "state.json").read_text())
    arena = (directory / "g_arena.bin").read_bytes()
    depths = (directory / "g_arena_depth.bin").read_bytes() if perspective else None
    if perspective and not state.get("g_depth_enabled"):
        raise ValueError("perspective diagnostic requires a capture with valid per-triangle depths")
    heads = struct.unpack("<256H", (directory / "g_triangle_bin_heads.bin").read_bytes())
    blocks = [(state["addresses"][name], (directory / (name + ".bin")).read_bytes())
              for name in ("g_texture_cache", "g_texture_cache_reference")]
    with flash_path.open("rb") as handle, mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as flash:
        def memory(address, count):
            if 0x90000000 <= address and address + count <= 0x90000000 + len(flash):
                return flash[address - 0x90000000:address - 0x90000000 + count]
            for start, block in blocks:
                if start <= address and address + count <= start + len(block):
                    return block[address - start:address - start + count]
            raise ValueError(f"uncaptured memory {address:#x}+{count}")

        with elf_path.open("rb") as handle:
            elf = ELFFile(handle)
            address = elf.get_section_by_name(".symtab").get_symbol_by_name("game_palette")[0]["st_value"]
        palette = np.frombuffer(memory(asset_base + address - 0xBEEF0000, 512), dtype="<u2").astype(int)
        rgb = np.array([((palette >> shift) & 31) * 255 // 31 for shift in (0, 5, 10)]).T.astype(np.uint8)
        pixels = np.zeros((120, 160), dtype=np.uint8)
        ids = np.full((120, 160), -1, dtype=int)
        entries = {}
        for head in heads[:128]:
            visited = set()
            while head != 65535:
                if head in visited or head >= state["g_arena_count"]:
                    raise ValueError("inconsistent snapshot chain")
                visited.add(head)
                index = head
                head, mode, color, *words = struct.unpack_from("<HBB7I", arena, 32 * head)
                s = np.array([struct.unpack("<hh", struct.pack("<I", w)) for w in words[:3]]) / 16
                uv = np.array([struct.unpack("<hh", struct.pack("<I", w)) for w in words[3:6]])
                reference = words[6]
                a, b, c = s
                denominator = np.linalg.det(np.array([b-a, c-a]))
                if abs(denominator) < 1e-8:
                    continue
                lo = np.maximum(np.floor(s.min(axis=0)).astype(int), [0,0])
                hi = np.minimum(np.ceil(s.max(axis=0)).astype(int), [159,119])
                if np.any(lo > hi):
                    continue
                yy, xx = np.mgrid[lo[1]:hi[1]+1, lo[0]:hi[0]+1]
                x, y = xx + .5 - a[0], yy + .5 - a[1]
                wb = (x*(c[1]-a[1])-y*(c[0]-a[0]))/denominator
                wc = ((b[0]-a[0])*y-(b[1]-a[1])*x)/denominator
                weights = np.stack([1-wb-wc,wb,wc],axis=-1)
                valid = np.all(weights >= -1e-10,axis=-1)
                texel = np.full(xx.shape,color,dtype=np.uint8)
                mips = None
                mip1 = False
                if mode in (1,2):
                    mips = struct.unpack("<II",memory(reference,8))
                    gradients = np.linalg.solve(np.array([b-a,c-a]), np.array([uv[1]-uv[0],uv[2]-uv[0]]))
                    mip1 = mips[0] != mips[1] and np.max(np.abs(gradients)) >= 96
                    coords = weights @ uv
                    if perspective:
                        q = np.array(struct.unpack_from("<3I", depths, 12 * index), dtype=float)
                        if np.any(q <= 0):
                            raise ValueError(f"entry {index} has no captured depth")
                        coords = (weights @ (uv * q[:,None])) / np.maximum(weights @ q, 1e-30)[...,None]
                    tx = np.floor(coords[...,0]/16).astype(int) & 31
                    ty = np.floor(coords[...,1]/(32 if mip1 else 16)).astype(int) & (15 if mip1 else 31)
                    offset = ty * (64 if mip1 else 32) + tx
                    texture = np.frombuffer(memory(mips[int(mip1)],1024 if not mip1 else 992),dtype=np.uint8)
                    texel = texture[offset]
                    if mode == 2:
                        valid &= texel != 0
                elif mode != 0:
                    continue
                pixels[yy[valid],xx[valid]] = texel[valid]
                ids[yy[valid],xx[valid]] = index
                entries[index] = dict(mode=mode,color=color,reference=reference,mips=mips,mip1=bool(mip1),screen=s.tolist(),uv=uv.tolist())
        output = rgb[pixels]
        output[ids < 0] = (25,35,75)
        stem = "arena-perspective" if perspective else "arena"
        Image.fromarray(output).resize((640,480),Image.Resampling.NEAREST).save(directory / (stem + "-projection.png"))
        np.save(directory / (stem + "-pixels.npy"), pixels)
        np.save(directory / (stem + "-ids.npy"),ids)
        (directory / (stem + "-entries.json")).write_text(json.dumps(entries,indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory",type=Path)
    parser.add_argument("flash",type=Path)
    parser.add_argument("elf",type=Path)
    parser.add_argument("asset_base",type=lambda value:int(value,0))
    parser.add_argument("--perspective", action="store_true",
                        help="diagnostic UV/Z interpolation; retains affine mip choice and painter order")
    args=parser.parse_args()
    render(args.directory,args.flash,args.elf,args.asset_base,args.perspective)
