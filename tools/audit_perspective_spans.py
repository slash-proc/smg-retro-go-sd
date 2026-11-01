#!/usr/bin/env python3
"""Measure horizontal perspective span error on captured stage triangles.

This is a floating-point quality/cost model, not production fixed-point code
or a hardware benchmark. Cost counts endpoint UV divisions, excluding setup.
"""
import argparse
import json
from pathlib import Path
import struct
import numpy as np


def audit(directory):
    state=json.loads((directory/'state.json').read_text())
    assert state['g_depth_enabled'], 'valid reciprocal depths required'
    arena=(directory/'g_arena.bin').read_bytes();depths=(directory/'g_arena_depth.bin').read_bytes()
    reports=[]
    for span in (4,8,16):
        errors=[];pixels=divisions=rows=0
        for index in range(state['g_arena_count']):
            _,mode,colour,*words=struct.unpack_from('<HBB7I',arena,index*32)
            if mode not in (1,2):continue
            screen=np.array([struct.unpack('<hh',struct.pack('<I',w)) for w in words[:3]],dtype=float)/16
            uv=np.array([struct.unpack('<hh',struct.pack('<I',w)) for w in words[3:6]],dtype=float)/16
            q=np.array(struct.unpack_from('<3I',depths,index*12),dtype=float)
            if min(q)<=0:raise ValueError('missing triangle depth')
            a,b,c=screen;det=np.linalg.det(np.array([b-a,c-a]))
            if abs(det)<1e-8:continue
            lo=np.maximum(np.floor(screen.min(axis=0)).astype(int),[0,0]);hi=np.minimum(np.ceil(screen.max(axis=0)).astype(int),[159,119])
            for y in range(lo[1],hi[1]+1):
                xx=np.arange(lo[0],hi[0]+1)+.5;x=xx-a[0];dy=y+.5-a[1]
                wb=(x*(c[1]-a[1])-dy*(c[0]-a[0]))/det
                wc=((b[0]-a[0])*dy-(b[1]-a[1])*x)/det
                weights=np.array([1-wb-wc,wb,wc]).T
                inside=np.all(weights>=-1e-10,axis=1)
                weights=weights[inside]
                if not len(weights):continue
                exact=(weights@(uv*q[:,None]))/(weights@q)[:,None]
                rows+=1;pixels+=len(exact)
                for start in range(0,len(exact),span):
                    values=exact[start:start+span];n=len(values);divisions+=2 if n==1 else 4
                    approximated=values[0]+np.arange(n)[:,None]*(values[-1]-values[0])/max(n-1,1)
                    errors.extend(np.max(abs(approximated-values),axis=1).tolist())
        reports.append({'span_pixels':span,'covered_stage_pixels_including_overdraw':pixels,
                        'rows':rows,'endpoint_attribute_divisions':divisions,'exact_attribute_divisions':pixels*2,
                        'max_error_source_texels':max(errors,default=0),
                        'p99_error_source_texels':float(np.quantile(errors,.99)) if errors else 0,
                        'fraction_above_one_eighth_texel':sum(e>.125 for e in errors)/len(errors) if errors else 0})
    return {'note':__doc__.strip(),'capture':str(directory),'reports':reports}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('directory',type=Path);parser.add_argument('output',type=Path);args=parser.parse_args()
    result=audit(args.directory);args.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
