#!/usr/bin/env python3
"""Reject accidental XIP outlining of the hot renderer during packaging."""
import argparse
from elftools.elf.elffile import ELFFile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--elf', required=True)
parser.add_argument('--itcm', choices=['0', '1'], required=True)
args = parser.parse_args()
with open(args.elf, 'rb') as stream:
    image = ELFFile(stream)
    symbols = list(image.get_section_by_name('.symtab').iter_symbols())
    outlined = [s.name for s in symbols if s['st_info']['type'] == 'STT_FUNC' and s['st_size']
                and ('raster_perspective_span' in s.name)]
    assert not outlined, f'Perspective span unexpectedly outlined: {outlined}'
    expected = '.core_itcm' if args.itcm == '1' else '.core_entry'
    shaders = [s for s in symbols if s.name.startswith('engine_raster_inner') and s['st_size']]
    names = {s.name for s in shaders}
    assert {'engine_raster_inner_affine_opaque', 'engine_raster_inner_perspective_opaque'} <= names
    for shader in shaders:
        actual = image.get_section(shader['st_shndx']).name
        assert actual == expected, f'{shader.name}: expected {expected}, got {actual}'
    tables = [s for s in symbols if s.name.startswith('inverse_steps.') and s['st_size']]
    sizes = sorted(s['st_size'] for s in tables)
    assert sizes in ([72], [136], [264]), 'Missing/shared-step table schema changed'
    sections = {image.get_section(s['st_shndx']).name for s in tables}
    if args.itcm == '1':
        assert sections == {'.core_itcm'}, f'Span-step table unexpectedly in {sections}'
        mip_tables = [s for s in symbols if s.name == 'engine_recip_table']
        assert len(mip_tables) == 1 and mip_tables[0]['st_size'] == 512, \
            'Expected the 256-entry original mip reciprocal table'
        assert image.get_section(mip_tables[0]['st_shndx']).name == '.core_itcm', \
            'Mip reciprocal table must be in ITCM'
        mip_helpers = [s for s in symbols if s.name == 'raster_mip_reciprocal' and s['st_size']]
        assert len(mip_helpers) == 1 and image.get_section(mip_helpers[0]['st_shndx']).name == '.core_itcm', \
            'Mip reciprocal setup must be in ITCM'
        dispatchers = [s for s in symbols if s.name == 'engine_raster.part.0' and s['st_size']]
        assert len(dispatchers) == 1, 'Expected the per-triangle raster dispatcher'
        assert image.get_section(dispatchers[0]['st_shndx']).name == '.core_itcm', \
            'Per-triangle raster dispatcher must remain in ITCM'
    extra = ' + engine_raster.part.0' if args.itcm == '1' else ''
    print(f'{len(shaders)} hot raster functions{extra} in {expected}; span inline; '
          f'step tables {sizes} in {sorted(sections)}')
