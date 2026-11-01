#!/usr/bin/env python3
"""Verify the loaded ITCM image against exact ELF pointer relocation sites.

Uses GWProv QMP bindings; never rewrites code. Compares every byte, including
opaque constants and fixed-address call veneers. Hardware timing is separate.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys
import time
from types import SimpleNamespace
from elftools.elf.elffile import ELFFile
from gwprov.debug_shell import DebugSession
from gwprov.qmp import QMPConnection

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'retro-go-sd/scripts'))
from gen_asset_relocs import asset_reference_sites


def audit_loaded_image(d, read_memory, elf):
    """Compare immutable ITCM bytes at a running or halted API boundary."""
    _,sites=asset_reference_sites(elf)
    with elf.open('rb') as f:
        e=ELFFile(f);section=e.get_section_by_name('.core_itcm')
        assert section and section['sh_size']>0,'ELF has no ITCM image'
        begin,size=section['sh_addr'],section['sh_size'];raw=section.data()
        asset_section=e.get_section_by_name('.xip_smg_assets');code_section=e.get_section_by_name('.xip_smg_code')
        targets=[('ram',asset_section['sh_addr'],asset_section['sh_size'],'smg_assets_xip_runtime_base'),
                 ('ram_code',code_section['sh_addr'],code_section['sh_size'],'smg_code_xip_runtime_base')]
    expected=bytearray(raw);changes=[];fixed=[]
    for kind,base,length,pointer in targets:
        runtime=d.read_path(pointer);assert 0x90000000<=runtime<0xa0000000
        for site in sites[kind]:
            if not begin<=site<begin+size:continue
            offset=site-begin;value=struct.unpack_from('<I',raw,offset)[0];bare=value&~1
            if base<=bare<base+length:
                new=(value+runtime-base)&0xffffffff;struct.pack_into('<I',expected,offset,new)
                changes.append({'site':site,'old':value,'new':new,'kind':kind})
            else:fixed.append({'site':site,'value':value,'kind':kind})
    actual=read_memory(begin,size)
    differences=[i for i,(a,b) in enumerate(zip(actual,expected)) if a!=b]
    report={'region_start':begin,'bytes':size,
        'expected_sha256':hashlib.sha256(expected).hexdigest(),
        'actual_sha256':hashlib.sha256(actual).hexdigest(),
        'patches':changes,'fixed_sites':fixed,'different_bytes':len(differences),
        'first_differences':differences[:20]}
    assert len(actual)==size and not differences, 'Loaded ITCM differs from relocated ELF'
    return report


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('device');parser.add_argument('elf',type=Path);parser.add_argument('output',type=Path)
    args=parser.parse_args()
    with QMPConnection(args.device,timeout=20) as q:
        d=DebugSession(SimpleNamespace(read_memory=q.read_memory),'gwemu',qmp_socket=args.device)
        d.symbols.load(args.elf)
        report=audit_loaded_image(d,q.read_memory,args.elf)
        before=d.read_path('smg_render_stats');time.sleep(.25);after=d.read_path('smg_render_stats')
        report.update(device=args.device,elf=str(args.elf.resolve()),before=before,after=after,
            faults={hex(a):q.u32(a) for a in [0xe000ed28,0xe000ed2c]})
        args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(report,indent=2)+'\n')
        assert not any(report['faults'].values()),report['faults']
        assert after['frames']>before['frames'],'No advancing gameplay; resume startup pause'
        print(json.dumps(report,indent=2))


if __name__=='__main__':main()
