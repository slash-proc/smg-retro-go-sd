#!/usr/bin/env python3
"""Validate and unpack the exact release bundle before publishing it."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

from make_bundle import expected_sizes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    if args.out.exists() and any(args.out.iterdir()):
        parser.error('output directory must be empty')
    with zipfile.ZipFile(args.bundle) as archive:
        entries = archive.infolist()
        names = [item.filename for item in entries]
        if len(names) != len(set(names)) or 'manifest.json' not in names:
            raise ValueError('bundle has duplicate entries or no manifest')
        manifest = json.loads(archive.read('manifest.json'))
        if (manifest['source']['repo'] != 'slash-proc/smg-retro-go-sd' or
                manifest['source']['ref'] != args.tag or manifest['source']['commit'] != args.commit):
            raise ValueError('bundle source does not match the release tag and commit')
        expected = expected_sizes(manifest)
        if set(names) != {'manifest.json', *expected}:
            raise ValueError('bundle entries do not match its manifest')
        for name, (size, digest) in expected.items():
            if Path(name).name != name or name.startswith('.'):
                raise ValueError(f'unsafe release filename: {name!r}')
            info = archive.getinfo(name)
            if info.file_size != size:
                raise ValueError(f'{name}: unexpected size')
            data = archive.read(info)
            if hashlib.sha256(data).hexdigest() != digest:
                raise ValueError(f'{name}: checksum mismatch')
        args.out.mkdir(parents=True, exist_ok=True)
        for name in names:
            (args.out / name).write_bytes(archive.read(name))
    print(f'Validated {args.tag} at {args.commit}; extracted {len(names)} files')


if __name__ == '__main__':
    main()
