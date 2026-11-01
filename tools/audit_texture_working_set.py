#!/usr/bin/env python3
"""Count covered texture samples and existing-cache usage in captured arenas.

Integer barycentric coverage and exact original mip threshold. Counts include
transparent texel reads and overdraw, before depth rejection; not bus accesses,
CPU cycles, cache-line misses, or a hardware performance prediction.
"""
import argparse
import json
from pathlib import Path
import struct
import numpy as np


def audit(directory):
    state=json.loads((directory/'state.json').read_text());arena=(directory/'g_arena.bin').read_bytes()
    width=state['smg_render_stats']['raster_width'];height=state['smg_render_stats']['raster_height']
    cache=state['addresses']['g_texture_cache_reference'];cache_count=state['sizes']['g_texture_cache_reference']//8
    counts=np.zeros((cache_count,2),dtype=np.int64);other=[0,0];triangles=[0,0]
    for index in range(state['g_arena_count']):
        _,mode,_,*words=struct.unpack_from('<HBB7I',arena,index*32)
        if mode not in (1,2,3,4) or not words[6]:continue
        xy=[struct.unpack('<hh',struct.pack('<I',w)) for w in words[:3]];uv=[struct.unpack('<hh',struct.pack('<I',w)) for w in words[3:6]]
        a,b,c=xy;area=(b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])
        if not area:continue
        sign=1 if area>0 else -1
        derivatives=[]
        for axis in range(2):
            derivatives.extend([(uv[1][axis]-uv[0][axis])*(c[1]-a[1])-(uv[2][axis]-uv[0][axis])*(b[1]-a[1]),
                (b[0]-a[0])*(uv[2][axis]-uv[0][axis])-(c[0]-a[0])*(uv[1][axis]-uv[0][axis])])
        # Mip equality is inspected from the actual captured descriptor.
        pointer=words[6]
        if not 0x24000000<=pointer<=0x24100000-8:
            # Immutable texture descriptors reside in XIP. The captured frame
            # has no XIP bytes; resolve their two ELF pointer words below.
            mips=descriptor_from_elf(state,pointer)
        else:
            with (directory/'axi.bin').open('rb') as f:f.seek(pointer-0x24000000);mips=struct.unpack('<2I',f.read(8))
        mip=int(mips[0]!=mips[1] and any(abs(d)>=6*abs(area) for d in derivatives))
        first=max(0,min(x for x,y in xy)//16-1);last=min(width-1,max(x for x,y in xy)//16+1)
        if first>last:continue
        xx=np.arange(first,last+1,dtype=np.int64)*16+8;pixels=0
        for y in range(max(0,min(y for x,y in xy)//16-1),min(height-1,max(y for x,y in xy)//16+1)+1):
            w1=sign*((xx-a[0])*(c[1]-a[1])-(y*16+8-a[1])*(c[0]-a[0]))
            w2=sign*((b[0]-a[0])*(y*16+8-a[1])-(b[1]-a[1])*(xx-a[0]))
            pixels+=int(np.count_nonzero((w1>=0)&(w2>=0)&(abs(area)-w1-w2>=0)))
        triangles[mip]+=1
        slot=(pointer-cache)//8 if cache<=pointer<cache+cache_count*8 and not (pointer-cache)%8 else None
        if slot is None:other[mip]+=pixels
        else:counts[slot,mip]+=pixels
    cached=counts.sum(axis=0).tolist();total=sum(cached)+sum(other)
    return {'capture':str(directory),'triangles_by_mip':triangles,'covered_samples':total,
        'cache_descriptor_samples_by_mip':cached,'other_descriptor_samples_by_mip':other,
        'existing_cache_mip0_fraction':cached[0]/total if total else 0,
        'cache_slots':[{ 'slot':i,'samples_by_mip':v.tolist()} for i,v in enumerate(counts) if v.sum()]}

_ELFS={}
def descriptor_from_elf(state,pointer):
    from elftools.elf.elffile import ELFFile
    path=state['elf']
    if path not in _ELFS:
        with open(path,'rb') as f:
            e=ELFFile(f);_ELFS[path]={name:e.get_section_by_name(name).data() for name in ['.xip_smg_code','.xip_smg_assets']}
    for name,key in [('.xip_smg_code','smg_code_xip_runtime_base'),('.xip_smg_assets','smg_assets_xip_runtime_base')]:
        base=state[key];raw=_ELFS[path][name]
        if base<=pointer<=base+len(raw)-8:return struct.unpack_from('<2I',raw,pointer-base)
    raise ValueError(f'uncaptured texture descriptor {pointer:#x}')


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('output',type=Path);p.add_argument('captures',nargs='+',type=Path);args=p.parse_args()
    r={'note':__doc__.strip(),'reports':[audit(d) for d in args.captures]};args.output.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r,indent=2))
