#!/usr/bin/env python3
"""Check the real palm recipe against the Wii model UVs and atlas alpha."""
from pathlib import Path
import hashlib
import json
import sys
import tempfile
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/asset_construction'))
from common import Dump
from stages import build_stage
from stages_galaxy import Arc
from stages_pool import Model


def main():
    ledger = json.loads((ROOT/'asset_construction/stages/observatory-palm-uv-fix.json').read_text())
    archive = ROOT/'build/orig/files/ObjectData/AstroBaseB.arc'
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == ledger['source_archive_sha256']
    arc = Arc(archive.read_bytes())
    model = Model(arc.get('astrobaseb/astrobaseb.bdl'))
    source = []
    for shape in model.shapes:
        if model.materials[shape['material']]['name'] != 'SolidPalmMat_v':
            continue
        for draw, attributes in shape['verts']:
            position = np.array(model.position(draw, attributes[9])) / 1000
            u, v = model.arrays[13][attributes[13]]
            source.append((position, np.array([u, 1-v])))
    pack = ROOT/'asset_construction/stages/observatory.pak'
    assert hashlib.sha256(pack.read_bytes()).hexdigest() == ledger['pack_sha256_after']
    with tempfile.TemporaryDirectory(prefix='palm-uv-test-',dir=ROOT/'tmp') as temporary:
        build_stage(Dump(str(ROOT/'build/orig/files')),str(pack),temporary)
        raw=(Path(temporary)/'assets/stages/observatory/observatory.obj').read_bytes()
        assert hashlib.sha256(raw).hexdigest() == ledger['obj_sha256_after']
        rows=raw.decode().splitlines(keepends=True)
        before=list(rows)
        for change in ledger['changes']:
            index=change['line']-1
            assert rows[index].rstrip('\n') == change['after']
            assert rows[index].startswith(('vt ', 'f '))
            if rows[index].startswith('f '):
                assert [c.split('/')[0] for c in rows[index].split()[1:]] == [c.split('/')[0] for c in change['before'].split()[1:]]
            before[index]=change['before']+ ('\n' if rows[index].endswith('\n') else '')
        assert hashlib.sha256(''.join(before).encode()).hexdigest() == ledger['obj_sha256_before']
        vertices=[];uvs=[];faces=[];active=False
        for row in rows:
            parts=row.split()
            if not parts:continue
            if parts[0]=='v':vertices.append(np.array(list(map(float,parts[1:4]))))
            elif parts[0]=='vt':uvs.append(np.array(list(map(float,parts[1:3]))))
            elif parts[0]=='o':active=parts[1]=='palm'
            elif active and parts[0]=='f':faces.append([tuple(int(v)-1 for v in corner.split('/')[:2]) for corner in parts[1:]])
        for face in faces:
            for vertex,uv in face:
                assert any(np.linalg.norm(p-vertices[vertex])<1e-7 and
                           np.max(abs(t-uvs[uv]))<1e-8 for p,t in source),(vertex,uv,vertices[vertex],uvs[uv],[(p,t) for p,t in source if np.linalg.norm(p-vertices[vertex])<1e-7])
        alpha=np.asarray(Image.open(ROOT/'build/constructed/assets/stages/observatory/t_SolidPalm.png'))[:,:,3]
        holes=[0,0];opaque=[0,0]
        for index,face in enumerate(faces):
            coords=np.array([uvs[t] for v,t in face])
            for a in range(21):
                for b in range(21-a):
                    uv=np.array([a,b,20-a-b])@coords/20
                    x=int(np.floor(uv[0]*32))%32;y=(31-int(np.floor(uv[1]*32)))%32
                    group=int(index>=8)
                    if alpha[y,x]<128:holes[group]+=1
                    else:opaque[group]+=1
        assert holes[0]==0 and opaque[0]>0,(holes,opaque)
        assert holes[1]>0 and opaque[1]>0,(holes,opaque)
        print(f'{len(ledger["changes"])} UV/UV-reference edits; every corner matches original model; trunk holes={holes[0]}, leaf cutouts={holes[1]}')


if __name__=='__main__':main()
