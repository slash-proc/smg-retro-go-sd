#!/usr/bin/env python3
"""Diagnostic OBJ projection with a depth buffer, independent of runtime culling.

Consumes a passive GWProv camera snapshot. This is an offline coverage oracle,
not the game renderer: no LOD, lighting, dynamic actors or material animation.
"""
import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image


def render(obj, camera, output, cull_backfaces=False):
    positions, uvs, faces = [], [], []
    material = object_name = ""
    for line in obj.read_text().splitlines():
        fields = line.split()
        if not fields:
            continue
        tag = fields[0]
        if tag == "v":
            x, y, z = map(float, fields[1:4])
            positions.append((-x * 1000, z * 1000, y * 1000))
        elif tag == "vt":
            uvs.append(tuple(map(float, fields[1:3])))
        elif tag == "o":
            object_name = fields[1]
        elif tag == "usemtl":
            material = fields[1]
        elif tag == "f":
            indices = [tuple(int(v) - 1 for v in term.split("/")[:2]) for term in fields[1:]]
            for k in range(1, len(indices) - 1):
                faces.append((object_name, material, [indices[0], indices[k], indices[k + 1]]))

    textures = {}
    used_materials = {material for _, material, _ in faces if "invisible" not in material}
    name = ""
    for line in obj.with_suffix(".mtl").read_text().splitlines():
        fields = line.split()
        if fields and fields[0] == "newmtl":
            name = fields[1]
        elif fields and fields[0] == "map_Kd" and name in used_materials:
            textures[name] = np.asarray(Image.open(obj.parent / fields[1]).convert("RGBA"))

    vector = lambda name: np.array([camera[name][k] for k in "xyz"], dtype=float)
    forward, up = vector("front") / 16384, vector("up") / 16384
    left = np.cross(up, forward)
    matrix = np.array([-left * 1648 / 1024, -up * 2472 / 1024, forward])
    points = (np.array(positions) - vector("position")) @ matrix.T
    width, height = 160, 120
    rgb = np.zeros((height, width, 3), dtype=np.uint8)
    rgb[:] = (25, 35, 75)
    depth = np.full((height, width), np.inf)
    ids = np.full((height, width), -1, dtype=int)
    for index, (object_name, material, corners) in enumerate(faces):
        if "invisible" in material:
            continue
        p = points[[c[0] for c in corners]]
        uv = np.array([uvs[c[1]] if len(c) > 1 else (0, 0) for c in corners])
        # Clip the polygon against the near plane, retaining UV attributes.
        poly = [np.r_[q, t] for q, t in zip(p, uv)]
        clipped = []
        for a, b in zip(poly, poly[1:] + poly[:1]):
            if a[2] >= 32:
                clipped.append(a)
            if (a[2] >= 32) != (b[2] >= 32):
                clipped.append(a + (b - a) * ((32 - a[2]) / (b[2] - a[2])))
        for k in range(1, len(clipped) - 1):
            tri = np.array([clipped[0], clipped[k], clipped[k + 1]])
            screen = (tri[:, :2] / tri[:, 2:3] + 1) * [width / 2, height / 2]
            lo = np.maximum(np.floor(screen.min(axis=0)).astype(int), [0, 0])
            hi = np.minimum(np.ceil(screen.max(axis=0)).astype(int), [width - 1, height - 1])
            if np.any(lo > hi):
                continue
            a, b, c = screen
            denominator = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
            if abs(denominator) < 1e-8:
                continue
            if cull_backfaces and denominator < 0 and "double_sided" not in material:
                continue
            yy, xx = np.mgrid[lo[1]:hi[1]+1, lo[0]:hi[0]+1]
            x, y = xx + .5 - a[0], yy + .5 - a[1]
            wb = (x * (c[1]-a[1]) - y * (c[0]-a[0])) / denominator
            wc = ((b[0]-a[0]) * y - (b[1]-a[1]) * x) / denominator
            weights = np.stack([1-wb-wc, wb, wc], axis=-1)
            valid = np.all(weights >= 0, axis=-1)
            reciprocal = weights @ (1 / tri[:, 2])
            z = 1 / np.maximum(reciprocal, 1e-12)
            tex = textures.get(material)
            if tex is None:
                colors = np.full((*xx.shape, 4), 255, dtype=np.uint8)
            else:
                # Affine UVs, as in the runtime, isolate coverage from perspective correction.
                coords = weights @ tri[:, 3:5]
                tx = (np.floor(coords[..., 0] * tex.shape[1]).astype(int)) % tex.shape[1]
                ty = (tex.shape[0] - 1 - np.floor(coords[..., 1] * tex.shape[0]).astype(int)) % tex.shape[0]
                colors = tex[ty, tx]
            valid &= colors[..., 3] >= 128
            valid &= z < depth[yy, xx]
            rgb[yy[valid], xx[valid]] = colors[valid, :3]
            depth[yy[valid], xx[valid]] = z[valid]
            ids[yy[valid], xx[valid]] = index
    Image.fromarray(rgb).resize((640, 480), Image.Resampling.NEAREST).save(output)
    np.save(output.with_suffix(".faces.npy"), ids)
    output.with_suffix(".faces.json").write_text(json.dumps(faces))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("obj", type=Path)
    parser.add_argument("snapshot", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--cull-backfaces", action="store_true")
    args = parser.parse_args()
    state = json.loads(args.snapshot.read_text())
    render(args.obj, state["g_game.camera"], args.output, args.cull_backfaces)
