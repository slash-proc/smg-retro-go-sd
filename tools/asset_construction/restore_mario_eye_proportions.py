"""Write the source-scale Mario eye bake from the exact inherited recipe pack."""
import argparse
import copy
import hashlib
import json
from pathlib import Path

from fixed_asset_inputs import read_pack, write_pack

BASE_SHA256 = '146fd6421e190bb240893565dd29be48635c79e0ba3292c0bc2852a580dc8c50'
RECIPE = 'figures/mario/marioeye.png'


def restore(source, output):
    if source.resolve() == output.resolve():
        raise ValueError('Use a separate output; preserve the original recipe')
    if hashlib.sha256(source.read_bytes()).hexdigest() != BASE_SHA256:
        raise ValueError('Source does not match the inherited texture pack')
    entries = read_pack(source)
    specs = json.loads(entries['bake_specs.json'])
    previous = copy.deepcopy(specs)
    for layer in specs[RECIPE]['layers'][:2]:
        assert layer['lowxf'] == [-1.9, 114, .8, .7, 0]
        layer['lowxf'][2:4] = [1, 1]
    # Both native and WASM constructors consume this shared recipe. These
    # atlases have no residual stream that could reintroduce old eye pixels.
    assert 'r/202' not in entries and 'r/208' not in entries
    assert [key for key in specs if specs[key] != previous[key]] == [RECIPE]
    updated = dict(entries)
    updated['bake_specs.json'] = json.dumps(specs, separators=(',', ':')).encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    write_pack(output, updated)
    verified = read_pack(output)
    assert [key for key in entries if entries[key] != verified[key]] == ['bake_specs.json']
    return dict(source_sha256=BASE_SHA256,
                result_sha256=hashlib.sha256(output.read_bytes()).hexdigest(),
                recipe=RECIPE, original=previous[RECIPE], corrected=specs[RECIPE],
                changed_entry='bake_specs.json',
                affected_images=['figures/mario/marioeye.png', 'figures/mariobee/marioeye.png',
                                 'figures/mariofire/marioeye.png'])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--ledger', type=Path, required=True)
    args = parser.parse_args()
    report = restore(args.source, args.output)
    args.ledger.write_text(json.dumps(report, indent=2) + '\n')
    print('Restored source-scale Mario eye bake; all other recipes and pack entries preserved')
