#!/usr/bin/env python3
"""Compare reconstructed Tower additions with independently decoded Honeyhive faces."""
import argparse
import struct
from pathlib import Path

GROUPS = {'fr_top', 'fr_belly', 'fr_columns'}


def read_obj(path):
    vertices, uvs, faces = [], [], []
    obj = material = ''
    def single(value):
        return struct.pack('<f', float(value))
    for line in path.read_text().splitlines():
        fields = line.split()
        if not fields:
            continue
        if fields[0] == 'v':
            vertices.append(tuple(map(single, fields[1:4])))
        elif fields[0] == 'vt':
            uvs.append(tuple(map(single, fields[1:3])))
        elif fields[0] == 'o':
            obj = fields[1]
        elif fields[0] == 'usemtl':
            material = fields[1]
        elif fields[0] == 'f':
            refs = [tuple(map(int, x.split('/')[:2])) for x in fields[1:]]
            corners = tuple((vertices[v-1], uvs[t-1]) for v, t in refs)
            faces.append((obj, material, corners))
    return faces


def materials(path):
    result = {}; name = None
    for line in path.read_text().splitlines():
        fields = line.split()
        if not fields:
            continue
        if fields[0] == 'newmtl':
            name = fields[1]
        elif fields[0] == 'map_Kd':
            result[name] = fields[1]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('candidate', type=Path, help='Reconstructed Tower stage directory')
    parser.add_argument('--constructed', type=Path, default=Path('build/constructed/assets/stages'))
    args = parser.parse_args()
    tower = args.constructed/'honeyhive_tower'; honey = args.constructed/'honeyhive'
    have_baseline = tower.resolve() != args.candidate.resolve()
    for suffix, expected_count in (('', 532), ('_far', 57)):
        target = 'honeyhive_tower' + suffix + '.obj'
        after = read_obj(args.candidate/target)
        source = read_obj(honey/('honeyhive'+suffix+'.obj'))
        if suffix:
            expected = [('fr_restored_far', mat, corners) for _, mat, corners in source if mat.startswith('fr_')]
        else:
            expected = [face for face in source if face[0] in GROUPS]
        assert len(expected) == expected_count, (suffix, len(expected))
        assert after[-len(expected):] == expected, target + ': added geometry differs'
        if have_baseline:
            original = (tower/target).read_bytes(); new = (args.candidate/target).read_bytes()
            assert new.startswith(original), target + ': original bytes changed'
            before = read_obj(tower/target)
            assert after[:len(before)] == before and after[len(before):] == expected, target + ': geometry differs'
        srcm = materials(honey/('honeyhive'+suffix+'.mtl'))
        destm = materials(args.candidate/('honeyhive_tower'+suffix+'.mtl'))
        for _, material, _ in expected:
            assert srcm[material] == destm[material], material
        print(target, len(expected), 'added faces match source positions, UVs and materials')
    for name in ('fr_cup.png', 'fr_soil.png'):
        assert (honey/name).read_bytes() == (args.candidate/name).read_bytes(), name
    print('New Tower texture aliases match their source images')
    if have_baseline:
        for name in ('honeyhive_tower.galaxy', 'honeyhive_tower.stage'):
            assert (tower/name).read_bytes() == (args.candidate/name).read_bytes(), name
        print('Original OBJ bytes and stage descriptors preserved')
    else:
        print('Added geometry checked; pass pre-repair --constructed stages to check the original prefix')


if __name__ == '__main__':
    main()
