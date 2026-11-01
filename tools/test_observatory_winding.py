#!/usr/bin/env python3
"""Validate the platform repair against reconstructed geometry and shared edges.

This decodes the real ISO-backed recipe. It checks that exactly the twelve
listed face windings changed, all other OBJ lines stay byte-identical, and
adjoining triangles no longer disagree about their common edge direction.
"""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
from collections import defaultdict

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/asset_construction'))
from common import Dump
from stages import build_stage


def inconsistent_edges(lines, object_name):
    vertices = []
    edges = defaultdict(list)
    active = False
    for line in lines:
        parts = line.split()
        if not parts:
            continue
        if parts[0] == 'v':
            vertices.append(tuple(map(float, parts[1:4])))
        elif parts[0] == 'o':
            active = parts[1] == object_name
        elif active and parts[0] == 'f':
            points = [vertices[int(c.split('/')[0])-1] for c in parts[1:]]
            for a, b in zip(points, points[1:]+points[:1]):
                if a != b:
                    edges[tuple(sorted((a, b)))].append(a < b)
    return sum(len(directions) == 2 and directions[0] == directions[1]
               for directions in edges.values())


def main():
    ledger = json.loads((ROOT / 'asset_construction/stages/observatory-winding-fix.json').read_text())
    pack = ROOT / 'asset_construction/stages/observatory.pak'
    palm_path = ROOT / 'asset_construction/stages/observatory-palm-uv-fix.json'
    palm = json.loads(palm_path.read_text()) if palm_path.exists() else None
    assert hashlib.sha256(pack.read_bytes()).hexdigest() == (palm or ledger)['pack_sha256_after']
    with tempfile.TemporaryDirectory(prefix='winding-test-', dir=ROOT / 'tmp') as temporary:
        build_stage(Dump(str(ROOT / 'build/orig/files')), str(pack), temporary)
        obj = Path(temporary) / 'assets/stages/observatory/observatory.obj'
        repaired = obj.read_bytes()
        if palm:
            # Undo the separately validated, later UV-only repair before
            # checking this winding repair against its original input.
            assert hashlib.sha256(repaired).hexdigest() == palm['obj_sha256_after']
            assert palm['obj_sha256_before'] == ledger['obj_sha256_after']
            lines = repaired.decode().splitlines(keepends=True)
            for change in palm['changes']:
                index = change['line'] - 1
                assert lines[index].rstrip('\n') == change['after']
                lines[index] = change['before'] + ('\n' if lines[index].endswith('\n') else '')
            repaired = ''.join(lines).encode()
        assert hashlib.sha256(repaired).hexdigest() == ledger['obj_sha256_after']
        rows = repaired.decode().splitlines()
        before = list(rows)
        for change in ledger['changes']:
            index = change['line'] - 1
            assert rows[index] == change['after']
            parts = change['before'].split()
            assert rows[index].split() == [parts[0], parts[1], parts[3], parts[2]]
            before[index] = change['before']
        original = ('\n'.join(before) + ('\n' if repaired.endswith(b'\n') else '')).encode()
        assert hashlib.sha256(original).hexdigest() == ledger['obj_sha256_before']
        assert inconsistent_edges(before, ledger['object']) == 6
        assert inconsistent_edges(rows, ledger['object']) == 0
        assert len(ledger['changes']) == 12
        print('ISO recipe: exactly 12 reversed faces; all positions/UVs/other lines unchanged; conflicting shared edges 6 -> 0')


if __name__ == '__main__':
    main()
