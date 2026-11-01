"""Reproduce the Honeyhive corner repair from its original ISO model.

Writes a candidate recipe and reconstruction to the supplied scratch directory.
Requires the exact pre-repair recipe; never updates the production input itself.
The existing OBJ delta format is preserved.
"""
from pathlib import Path
from collections import defaultdict
import sys, json, hashlib, shutil, argparse
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/asset_construction'))
from common import read_pak, IntReader, Dump
from stage_recipe_descriptors import write_pack
from stages import build_stage
from stages_galaxy import Galaxy, Arc
from stages_pool import Model, ArchiveCache, build_pool
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source_recipe', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--base-sha256', default='4f7ffbf0a95dba934d67821c955aa580a810f2110a43b6688da8aa0a6f969f62')
parser.add_argument('--seed-triangles', type=int, nargs='+', default=[60,61,62,95,96,97,99,174,192])
parser.add_argument('--object', default='ground_corner_restore')
args = parser.parse_args()
root = args.output.resolve()
root.mkdir(parents=True, exist_ok=True)
source = args.source_recipe.resolve()
assert hashlib.sha256(source.read_bytes()).hexdigest() == args.base_sha256, 'Source recipe does not match the pre-repair baseline'
pak = read_pak(str(source))
meta = json.loads(pak['meta'])
dump = Dump(str(ROOT / 'build/orig/files'))
(root / 'input').mkdir(exist_ok=True)
shutil.copyfile(source, root / 'input/honeyhive.pak')
build_stage(dump, str(root / 'input/honeyhive.pak'), str(root / 'before'))
base = root / 'before/assets/stages/honeyhive'
galaxy = Galaxy(dump, meta['galaxy']).build()
pool = build_pool(ArchiveCache(dump), galaxy['objects'], [tuple(s) for s in meta['sources']], galaxy['zone_placements'])
anchors = {tuple((round(x, 9) for x in p)): i for i, p in enumerate(pool.pts)}
model = Model(Arc((ROOT / 'build/orig/files/ObjectData/ForestHomePlanet.arc').read_bytes()).get('foresthomeplanet/foresthomeplanet.bdl'))
shape = model.shapes[14]
points = []
uv = []
assert model.materials[shape['material']]['name'] == 'Soil02_v'
for drw, att in shape['verts']:
    q = model.position(drw, att[9])
    points.append(((q[0] + 30) / 1000, (q[1] - 1780) / 1000, (q[2] - 920) / 1000))
    u, v = model.arrays[13][att[13]]
    uv.append((u, 1 - v))
edges = defaultdict(set)
for i, face in enumerate(shape['tris']):
    for a, b in zip(face, face[1:] + face[:1]):
        edges[tuple(sorted((points[a], points[b])))].add(i)
picked = set(args.seed_triangles)
for i in list(picked):
    for a, b in zip(shape['tris'][i], shape['tris'][i][1:] + shape['tris'][i][:1]):
        picked.update(edges[tuple(sorted((points[a], points[b])))])
picked = sorted(picked)
print('Original patch faces', len(picked), picked)

def read(key):
    r = IntReader(pak.get(key, b''))
    out = []
    while r.pos < len(r.data):
        out.append(r.int())
    return out

def encode(values):
    out = bytearray()
    for n in values:
        v = n * 2 if n >= 0 else -n * 2 - 1
        while v >= 128:
            out.append(v & 127 | 128)
            v >>= 7
        out.append(v)
    return bytes(out)
basefiles = {}
for filename, nlines in meta['files']:
    if filename not in ('honeyhive.obj', 'honeyhive_far.obj'):
        continue
    prefix = filename + '/'
    streams = {k: read(prefix + k) for k in ['o.kind', 'o.run', 'ls.nstr', 'ls.strs', 'ls.ref', 'ls.exact', 'ln.nops', 'ln.op', 'ln.n', 'ln.ins', 'ln.str', 'g.n', 'g.mode', 'g.aff', 'v.anc', 'v.res', 'v.dec', 'v.nz', 'vt.res', 'vt.dec', 'vt.nz', 'f.v', 'f.t']}
    kinds = []
    for k, n in zip(streams['o.kind'], streams['o.run']):
        kinds.extend([k] * n)
    assert len(kinds) == nlines
    vcount = kinds.count(1)
    tcount = kinds.count(2)
    a_prev = sum(streams['v.anc'])
    v_prev = sum(streams['f.v'])
    t_prev = sum(streams['f.t'])
    nstr = streams['ls.nstr'][0]
    for text in ['o ' + args.object, 'usemtl soil']:
        b = text.encode()
        streams['ls.strs'].extend([len(b), *b])
        streams['ls.ref'].append(-(1 << 30))
        streams['ls.exact'].append(0)
        streams['ln.nops'].append(1)
        streams['ln.op'].append(2)
        streams['ln.n'].append(1)
        streams['ln.ins'].append(-1)
        streams['ln.str'].append(nstr)
        nstr += 1
        kinds.append(0)
    streams['ls.nstr'][0] = nstr
    streams['g.n'][0] += 1
    streams['g.mode'].append(3)
    streams['g.aff'].extend([0] * 8)
    for index in picked:
        for corner in shape['tris'][index]:
            p = points[corner]
            t = uv[corner]
            anchor = anchors[tuple((round(x, 9) for x in p))]
            streams['v.anc'].append(anchor - a_prev)
            a_prev = anchor
            streams['v.res'].extend([0, 0, 0])
            streams['v.dec'].append(2457)
            streams['v.nz'].append(0)
            streams['vt.res'].extend((round(x * 1000000000.0) for x in t))
            streams['vt.dec'].append(153)
            streams['vt.nz'].append(0)
            vcount += 1
            tcount += 1
            streams['f.v'].append(vcount - v_prev)
            streams['f.t'].append(tcount - t_prev)
            v_prev = vcount
            t_prev = tcount
            kinds.extend([1, 2])
        kinds.append(3)
    streams['o.kind'] = []
    streams['o.run'] = []
    for k in kinds:
        if streams['o.kind'] and streams['o.kind'][-1] == k:
            streams['o.run'][-1] += 1
        else:
            streams['o.kind'].append(k)
            streams['o.run'].append(1)
    for key, values in streams.items():
        pak[prefix + key] = encode(values)
    for row in meta['files']:
        if row[0] == filename:
            row[1] = len(kinds)
    basefiles[filename] = {'lines_before': nlines, 'lines_after': len(kinds), 'original_bytes': (base / filename).stat().st_size, 'original_sha256': hashlib.sha256((base / filename).read_bytes()).hexdigest()}
pak['meta'] = json.dumps(meta, separators=(',', ':')).encode()
(root / 'recipes').mkdir(exist_ok=True)
candidate = root / 'recipes/honeyhive.pak'
write_pack(candidate, pak)
build_stage(dump, str(candidate), str(root / 'candidate'))
for filename, info in basefiles.items():
    before = (base / filename).read_bytes()
    after = (root / 'candidate/assets/stages/honeyhive' / filename).read_bytes()
    assert after[:len(before)] == before, (filename, 'original prefix changed')
    info['result_sha256'] = hashlib.sha256(after).hexdigest()
(root / 'recipe-report.json').write_text(json.dumps({'source_archive': 'ObjectData/ForestHomePlanet.arc', 'source_model': 'foresthomeplanet/foresthomeplanet.bdl', 'shape': 14, 'source_triangles': picked, 'old_pack_sha256': hashlib.sha256(source.read_bytes()).hexdigest(), 'new_pack_sha256': hashlib.sha256(candidate.read_bytes()).hexdigest(), 'files': basefiles}, indent=2) + '\n')
print('Original OBJ prefixes preserved in both near and far meshes')
for t in (ROOT / 'build/constructed/assets/stages/honeyhive').glob('*.png'):
    (root / 'candidate/assets/stages/honeyhive' / t.name).symlink_to(t.resolve())
(root / 'candidate/assets/stages/honeyhive/honeyhive.mtl').write_bytes((base / 'honeyhive.mtl').read_bytes())
