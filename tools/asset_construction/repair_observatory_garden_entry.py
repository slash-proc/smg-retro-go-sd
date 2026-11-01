"""Align Garden's existing menu trigger with its authored doorway.

Preserves the recipe format and changes one descriptor row in both native
line-delta reconstruction and the WASM literal descriptor. Writes a candidate
pack only; the production input is never updated in place.
"""
import argparse
import hashlib
import json
from pathlib import Path
from fixed_asset_inputs import read_pack, write_pack

BASE_SHA256 = '7c5c92332a970cffe45a6132507c85b35535b0f9df351a6b7d35229545c96f8b'
OLD = 'object dome 0.7521 0.0749 24.0801  0.0000 0.0000 1.0000  0.1908 -0.9816 0.0000 a0=0 a1=6 scen=ff'
NEW = 'object dome 2.1407 0.0000 24.6797  0.0000 0.0000 1.0000  0.1908 -0.9816 0.0000 a0=0 a1=6 scen=ff'


def repair(source, destination):
    if source.resolve() == destination.resolve():
        raise ValueError('Preserve the original recipe; use a separate output')
    if hashlib.sha256(source.read_bytes()).hexdigest() != BASE_SHA256:
        raise ValueError('Source recipe does not match the audited baseline')
    before = read_pack(source)
    after = dict(before)
    key = 'observatory.stage/native'
    rows = before[key].decode().split('\n')
    assert len(rows) == 44 and rows[39] == OLD and rows.count(OLD) == 1
    rows[39] = NEW
    after[key] = '\n'.join(rows).encode()
    key = 'observatory.stage/ln.res'
    payload = bytearray(before[key])
    assert len(payload) == 752 and payload[698:701] == b'\0\0\0'
    for offset, replacement in [(700, bytes.fromhex('c0ed05')), (699, bytes.fromhex('c85d')), (698, bytes.fromhex('e0c70d'))]:
        payload[offset:offset+1] = replacement
    after[key] = bytes(payload)
    assert len(payload) == 757
    destination.parent.mkdir(parents=True, exist_ok=True)
    write_pack(destination, after)
    verified = read_pack(destination)
    changed = [k for k in before if before[k] != verified[k]]
    assert set(changed) == {'observatory.stage/native', 'observatory.stage/ln.res'}
    return dict(source_sha256=BASE_SHA256,
                result_sha256=hashlib.sha256(destination.read_bytes()).hexdigest(),
                changed_entries=changed, descriptor_row=39, runtime_object=36,
                old=OLD, corrected=NEW, old_runtime_position=[192,19,6164],
                corrected_runtime_position=[548,0,6318], radius_unchanged=170,
                reason='Original cube origin used by a fixed sphere misses authored doorway; align existing menu trigger with doorway body height')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--ledger', type=Path, required=True)
    args = parser.parse_args()
    report = repair(args.source, args.destination)
    args.ledger.parent.mkdir(parents=True, exist_ok=True)
    args.ledger.write_text(json.dumps(report, indent=2)+'\n')
