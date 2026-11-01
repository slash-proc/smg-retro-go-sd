#!/usr/bin/env python3
"""Validate the repaired corner against original ISO vertices and UVs."""
from pathlib import Path
import hashlib
import json
import sys
import tempfile
import numpy as np

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools/asset_construction'))
from common import Dump
from stages import build_stage
from stages_galaxy import Arc
from stages_pool import Model


def validate(ledger, temporary):
    archive=ROOT/'build/orig/files'/ledger['source_archive']
    assert hashlib.sha256(archive.read_bytes()).hexdigest()==ledger['source_archive_sha256']
    model=Model(Arc(archive.read_bytes()).get(ledger['source_model']))
    shape=model.shapes[ledger['shape']]
    assert model.materials[shape['material']]['name']=='Soil02_v'
    expected=[]
    for index in ledger['source_triangles']:
        vertices=[]
        for corner in shape['tris'][index]:
            draw,attributes=shape['verts'][corner]
            point=np.array(model.position(draw,attributes[9]))+np.array(ledger['source_world']['pos'])
            u,v=model.arrays[13][attributes[13]]
            vertices.append((point/1000,np.array([u,1-v])))
        expected.append(vertices)
    for filename,info in ledger['files'].items():
        raw=(Path(temporary)/'assets/stages/honeyhive'/filename).read_bytes()
        assert hashlib.sha256(raw[:info['result_bytes']]).hexdigest()==info['result_sha256']
        assert hashlib.sha256(raw[:info['original_bytes']]).hexdigest()==info['original_sha256']
        vertices=[];uvs=[];faces=[];active=False
        for row in raw.decode().splitlines():
            parts=row.split()
            if not parts:continue
            if parts[0]=='v':vertices.append(np.array(list(map(float,parts[1:4]))))
            elif parts[0]=='vt':uvs.append(np.array(list(map(float,parts[1:3]))))
            elif parts[0]=='o':active=parts[1]==ledger['object']
            elif active and parts[0]=='usemtl':assert parts[1]==ledger['material']
            elif active and parts[0]=='f':faces.append([tuple(int(v)-1 for v in corner.split('/')[:2]) for corner in parts[1:]])
        assert len(faces)==len(expected)
        for face,original in zip(faces,expected):
            for (vertex,uv),(point,texcoord) in zip(face,original):
                assert np.max(abs(vertices[vertex]-point))<=6e-10
                assert np.max(abs(uvs[uv]-texcoord))<=6e-10
    print(f"Near/far {ledger['object']}: original OBJ prefixes unchanged; {len(expected)} appended triangles match ISO positions, winding and UVs")


def main():
    ledgers=[json.loads((ROOT/'asset_construction/stages'/name).read_text())
             for name in ['honeyhive-corner-fix.json','honeyhive-seam-fix.json']]
    assert ledgers[0]['new_pack_sha256']==ledgers[1]['old_pack_sha256']
    pack=ROOT/'asset_construction/stages/honeyhive.pak'
    assert hashlib.sha256(pack.read_bytes()).hexdigest()==ledgers[-1]['new_pack_sha256']
    with tempfile.TemporaryDirectory(prefix='corner-test-',dir=ROOT/'tmp') as temporary:
        build_stage(Dump(str(ROOT/'build/orig/files')),str(pack),temporary)
        for ledger in ledgers:validate(ledger,temporary)


if __name__=='__main__':main()
