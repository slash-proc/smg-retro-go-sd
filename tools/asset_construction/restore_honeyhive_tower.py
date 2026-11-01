"""Generate isolated shared recipes restoring Honeyhive's missing Tower region.

The input packs are never modified. The output packs use the existing stage and
texture recipe formats, so both the native and WASM decoders consume them.
Requires the reconstructed USA ISO source tree and the two constructed OBJs.
"""
import argparse
import hashlib
import json
from decimal import Decimal
from pathlib import Path

from common import Dump, IntReader, read_pak
from stage_recipe_descriptors import write_pack
from stages_galaxy import Galaxy
from stages_pool import ArchiveCache, build_pool

ROOT = Path(__file__).resolve().parents[2]
STAGE_HASH = '1f7703803eed8e9b0f58dc6f964e55fc6a10e9e505b5830199f8c665b0e96149'
TEXTURE_HASH = 'b0032a6076861cde4ce35109e605a8eabadf7ad973a262813cac4c6572132e67'
GROUPS = {'fr_top', 'fr_belly', 'fr_columns'}
SOURCE_HASHES = {
    'honeyhive.obj': 'a7b0d8635f5066a2f5e8e8e1ea0ea14febf733bdded9f813fb6361c7c04f5f87',
    'honeyhive_far.obj': '59d3f1125ea1b961db0aad27ea897fc53634bf071b957559f1765681d956571b',
    'honeyhive.mtl': '13f511005586d0641d15ca5f3ac1f4da6502701a24214e6c592e30fe5aea6fc4',
}


def integers(payload):
    reader = IntReader(payload)
    values = []
    while reader.pos < len(reader.data):
        values.append(reader.int())
    return values


def encode(values):
    data = bytearray()
    for value in values:
        n = value * 2 if value >= 0 else -value * 2 - 1
        while n >= 128:
            data.append((n & 127) | 128)
            n >>= 7
        data.append(n)
    return bytes(data)


def add_text(streams, text):
    raw = text.encode('utf-8')
    index = streams['ls.nstr'][0]
    streams['ls.strs'].extend([len(raw), *raw])
    streams['ls.ref'].append(-(1 << 30))
    streams['ls.exact'].append(0)
    streams['ln.nops'].append(1)
    streams['ln.op'].append(2)
    streams['ln.n'].append(1)
    streams['ln.ins'].append(-1)
    streams['ln.str'].append(index)
    streams['ls.nstr'][0] += 1


def read_geometry(path, far):
    vertices, uvs, faces = [], [], []
    obj = material = ''
    for line in path.read_text().splitlines():
        fields = line.split()
        if not fields:
            continue
        if fields[0] == 'v':
            vertices.append(fields[1:4])
        elif fields[0] == 'vt':
            uvs.append(fields[1:3])
        elif fields[0] == 'o':
            obj = ' '.join(fields[1:])
        elif fields[0] == 'usemtl':
            material = fields[1]
        elif fields[0] == 'f' and ((far and material.startswith('fr_')) or
                                     (not far and obj in GROUPS)):
            if len(fields) != 4:
                raise ValueError('Expected triangular source geometry')
            corners = [tuple(map(int, term.split('/')[:2])) for term in fields[1:]]
            faces.append((obj if not far else 'fr_restored_far', material, corners))
    return vertices, uvs, faces


def append_obj(pack, meta, filename, source, pool_anchor, far):
    row = next(row for row in meta['files'] if row[0] == filename)
    prefix = filename + '/'
    keys = ('o.kind', 'o.run', 'ls.nstr', 'ls.strs', 'ls.ref', 'ls.exact',
            'ln.nops', 'ln.op', 'ln.n', 'ln.ins', 'ln.str', 'g.n', 'g.mode',
            'g.aff', 'v.anc', 'v.res', 'v.dec', 'v.nz', 'vt.res', 'vt.dec',
            'vt.nz', 'f.v', 'f.t')
    streams = {key: integers(pack.get(prefix + key, b'')) for key in keys}
    kinds = [kind for kind, count in zip(streams['o.kind'], streams['o.run']) for _ in range(count)]
    if len(kinds) != row[1]:
        raise ValueError('Existing OBJ stream length mismatch')
    vertices, uvs, faces = read_geometry(source, far)
    expected = 57 if far else 532
    if len(faces) != expected:
        raise ValueError(f'Unexpected source face count: {len(faces)}')
    vmap, tmap = {}, {}
    for _, _, corners in faces:
        for v, t in corners:
            vmap.setdefault(v, len(vmap) + 1)
            tmap.setdefault(t, len(tmap) + 1)
    vbase, tbase = kinds.count(1), kinds.count(2)
    previous_anchor = sum(streams['v.anc'])
    for source_index in vmap:
        streams['v.anc'].append(-previous_anchor)
        previous_anchor = 0
        negative_zero = 0
        for axis, (val, anchor) in enumerate(zip(vertices[source_index - 1], pool_anchor)):
            decimal = Decimal(val)
            scaled = int(decimal * 10**9)
            streams['v.res'].append(scaled - round(anchor * 10**9))
            desired = decimal.is_zero() and decimal.is_signed()
            if desired != (scaled == 0 and anchor < 0):
                negative_zero |= 1 << axis
        streams['v.dec'].append(0x999)
        streams['v.nz'].append(negative_zero)
        kinds.append(1)
    for source_index in tmap:
        coordinates = [Decimal(val) for val in uvs[source_index - 1]]
        streams['vt.res'].extend(int(val * 10**9) for val in coordinates)
        streams['vt.dec'].append(0x99)
        streams['vt.nz'].append(sum(1 << axis for axis, val in enumerate(coordinates)
                                    if val.is_zero() and val.is_signed()))
        kinds.append(2)
    prev_v, prev_t = sum(streams['f.v']), sum(streams['f.t'])
    current_obj = current_material = None
    seen_groups = set()
    for obj, material, corners in faces:
        if obj != current_obj:
            add_text(streams, 'o ' + obj); kinds.append(0)
            current_obj, current_material = obj, None
        if material != current_material:
            add_text(streams, 'usemtl ' + material); kinds.append(0)
            current_material = material
        key = (obj, material)
        if key not in seen_groups:
            seen_groups.add(key)
            streams['g.mode'].append(3)
            streams['g.aff'].extend([0] * 8)
        for v, t in corners:
            new_v, new_t = vbase + vmap[v], tbase + tmap[t]
            streams['f.v'].append(new_v - prev_v); streams['f.t'].append(new_t - prev_t)
            prev_v, prev_t = new_v, new_t
        kinds.append(3)
    streams['g.n'][0] += len(seen_groups)
    streams['o.kind'], streams['o.run'] = [], []
    for kind in kinds:
        if streams['o.kind'] and streams['o.kind'][-1] == kind:
            streams['o.run'][-1] += 1
        else:
            streams['o.kind'].append(kind); streams['o.run'].append(1)
    for key, values in streams.items():
        pack[prefix + key] = encode(values)
    row[1] = len(kinds)
    return {'faces': len(faces), 'vertices': len(vmap), 'uvs': len(tmap),
            'materials': sorted({material for _, material, _ in faces})}


def append_materials(pack, meta, filename, source_mtl, needed):
    row = next(row for row in meta['files'] if row[0] == filename)
    prefix = filename + '/'
    keys = ('ls.nstr','ls.strs','ls.ref','ls.exact','ln.nops','ln.op','ln.n','ln.ins','ln.str')
    streams = {key: integers(pack.get(prefix + key, b'')) for key in keys}
    blocks = {}; key = None
    for line in source_mtl.read_text().splitlines():
        if line.startswith('newmtl '):
            key = line.split()[1]; blocks[key] = []
        if key is not None:
            blocks[key].append(line)
    for material in sorted(needed):
        for line in blocks[material]:
            add_text(streams, line); row[1] += 1
    for key, values in streams.items():
        pack[prefix + key] = encode(values)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--stage-recipe', type=Path,
                        default=ROOT/'asset_construction/stages/honeyhive_tower.pak')
    parser.add_argument('--texture-recipe', type=Path,
                        default=ROOT/'asset_construction/textures/textures.pak')
    args = parser.parse_args()
    stage_path, texture_path = args.stage_recipe, args.texture_recipe
    for name, source in (('honeyhive_tower.pak', stage_path), ('textures.pak', texture_path)):
        if (args.output/name).resolve() == source.resolve():
            raise ValueError('Output must not overwrite a source recipe: ' + str(source))
    for path, expected in ((stage_path, STAGE_HASH), (texture_path, TEXTURE_HASH)):
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError(f'Unexpected source recipe: {path}')
    pack = read_pak(str(stage_path)); meta = json.loads(pack['meta'])
    dump = Dump(str(ROOT/'build/orig/files')); galaxy = Galaxy(dump, meta['galaxy']).build()
    pool = build_pool(ArchiveCache(dump), galaxy['objects'], [tuple(s) for s in meta['sources']], galaxy['zone_placements'])
    if not pool.pts:
        raise ValueError('Empty source geometry pool')
    assets = ROOT/'build/constructed/assets/stages/honeyhive'
    for name, expected in SOURCE_HASHES.items():
        if hashlib.sha256((assets/name).read_bytes()).hexdigest() != expected:
            raise ValueError(f'Unexpected reconstructed source: {assets/name}')
    report = {}
    for far in (False, True):
        suffix = '_far' if far else ''
        report[suffix or 'main'] = append_obj(pack, meta, f'honeyhive_tower{suffix}.obj', assets/f'honeyhive{suffix}.obj', pool.pts[0], far)
    # The far MTL already contains these names. The main MTL lacks all eight.
    append_materials(pack, meta, 'honeyhive_tower.mtl', assets/'honeyhive.mtl', report['main']['materials'])
    pack['meta'] = json.dumps(meta, separators=(',', ':')).encode()
    args.output.mkdir(parents=True, exist_ok=True)
    write_pack(args.output/'honeyhive_tower.pak', pack)
    textures = read_pak(str(texture_path)); texture_meta = json.loads(textures['meta.json'])
    for name in ('fr_cup.png', 'fr_soil.png'):
        src = 'assets/stages/honeyhive/' + name
        dst = 'assets/stages/honeyhive_tower/' + name
        rows = [image for image in texture_meta['images'] if any(f['path'] == src for f in image['files'])]
        if len(rows) != 1 or any(f['path'] == dst for f in rows[0]['files']):
            raise ValueError('Unexpected texture alias source: ' + name)
        rows[0]['files'].append({'path': dst, 'style': 'blender'})
    textures['meta.json'] = json.dumps(texture_meta, separators=(',', ':')).encode()
    write_pack(args.output/'textures.pak', textures)
    for name in ('honeyhive_tower.pak', 'textures.pak'):
        report[name] = hashlib.sha256((args.output/name).read_bytes()).hexdigest()
    (args.output/'report.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
