#!/usr/bin/env python3
"""Run production perspective spans on captured triangles using Docker.

Counts overdraw-inclusive work; this is not a hardware timing measurement.
Coverage rows are selected independently using integer barycentric tests.
Long-double rational per-pixel UVs check the production helper's error bound.
"""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import numpy as np
ROOT=Path(__file__).resolve().parents[1]


def rows(directory):
    state=json.loads((directory/'state.json').read_text())
    width=state['smg_render_stats']['raster_width'];height=state['smg_render_stats']['raster_height']
    arena=(directory/'g_arena.bin').read_bytes();depths=(directory/'g_arena_depth.bin').read_bytes()
    assert state.get('g_depth_enabled') or state.get('g_perspective_enabled'), 'depth collection must be enabled'
    for index in range(state['g_arena_count']):
        _,mode,_,*words=struct.unpack_from('<HBB7I',arena,index*32)
        if mode not in (1,2):continue
        xy=np.array([struct.unpack('<hh',struct.pack('<I',w)) for w in words[:3]],dtype=np.int64)
        uv=np.array([struct.unpack('<hh',struct.pack('<I',w)) for w in words[3:6]],dtype=float)
        q=np.array(struct.unpack_from('<3I',depths,index*12),dtype=float)
        assert min(q)>0, f'missing depth for triangle {index}'
        a,b,c=xy;area=int((b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0]))
        if not area:continue
        sign=1 if area>0 else -1;area=abs(area)
        first=max(0,int(xy[:,0].min()//16)-1);last=min(width-1,int(xy[:,0].max()//16)+1)
        if first>last:continue
        xx=np.arange(first,last+1,dtype=np.int64)*16+8
        attributes=np.array([q,q*uv[:,0],q*uv[:,1]])
        for y in range(max(0,int(xy[:,1].min()//16)-1),min(height-1,int(xy[:,1].max()//16)+1)+1):
            dy=y*16+8-a[1]
            w1=sign*((xx-a[0])*(c[1]-a[1])-dy*(c[0]-a[0]))
            w2=sign*((b[0]-a[0])*dy-(b[1]-a[1])*(xx-a[0]))
            covered=np.flatnonzero((w1>=0)&(w2>=0)&(area-w1-w2>=0))
            if not len(covered):continue
            left=int(covered[0]);assert covered[-1]-left+1==len(covered)
            values=area*attributes[:,0]+float(w1[left])*(attributes[:,1]-attributes[:,0])+float(w2[left])*(attributes[:,2]-attributes[:,0])
            gradient=sign*16*((c[1]-a[1])*(attributes[:,1]-attributes[:,0])-(b[1]-a[1])*(attributes[:,2]-attributes[:,0]))
            yield struct.pack('<6dI',*values,*gradient,len(covered))


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('output',type=Path);parser.add_argument('captures',type=Path,nargs='+');parser.add_argument('--reuse',action='store_true');parser.add_argument('--precheck',choices=['on','off'],default='on');parser.add_argument('--max-span',type=int,choices=(8,16),default=8);args=parser.parse_args()
    source=(ROOT/'src/graphics/engine.c').read_text();helper=source[source.index('/* Perspective attributes'):source.index('static bool raster_span_bound(')]
    helper=helper.replace('inverse = 1 / q;','divisions++; inverse = 1 / q;').replace('inverse = 1 / q1;','divisions++; inverse = 1 / q1;')
    prefix='#include <stdint.h>\n#include <stdbool.h>\n#include <stdio.h>\n#include <stdlib.h>\n#include <math.h>\nstatic unsigned long divisions;\n'
    suffix=r'''
int main(void){
 g_perspective_precheck_enabled=USE_PRECHECK;
 double row[6];uint32_t count;unsigned long histogram[MAX_SPAN+1]={0},pixels=0,rows=0;long double worst=0;
 while(fread(row,sizeof row,1,stdin)==1){
  if(fread(&count,4,1,stdin)!=1)abort();unsigned at=0;PerspectiveEndpoint endpoint={0};rows++;
  double curvature=raster_perspective_curvature(row[0],row[1],row[2],row[3],row[4],row[5]);
  while(at<count){
   PerspectiveSpan s=raster_perspective_span(row[0]+at*row[3],row[1]+at*row[4],row[2]+at*row[5],row[3],row[4],row[5],count-at,USE_REUSE?&endpoint:NULL,USE_PRECHECK?&curvature:NULL);
   if(s.count<1||s.count>MAX_SPAN||s.count>count-at)abort();histogram[s.count]++;
   uint32_t u=s.u,v=s.v;
   for(unsigned j=0;j<s.count;j++){
    long double q=(long double)row[0]+(at+j)*(long double)row[3];
    long double eu=((long double)row[1]+(at+j)*(long double)row[4])/q;
    long double ev=((long double)row[2]+(at+j)*(long double)row[5])/q;
    long double error=fmaxl(fabsl(remainderl((long double)u/65536-eu,65536)),fabsl(remainderl((long double)v/65536-ev,65536)));
    if(error>worst)worst=error;if(error>1.001L){fprintf(stderr,"UV error %.9Lf\n",error);abort();}
    u+=s.du;v+=s.dv;pixels++;
   }
   at+=s.count;
  }
 }
 printf("{\"rows\":%lu,\"pixels_including_overdraw\":%lu,\"q_divisions\":%lu,\"worst_uv_error\":%.12Lf,\"span_histogram\":[",rows,pixels,divisions,worst);
 for(unsigned i=1;i<=MAX_SPAN;i++)printf("%s%lu",i>1?",":"",histogram[i]);puts("]}");return 0;
}
'''
    (ROOT/'tmp').mkdir(exist_ok=True);reports=[]
    with tempfile.TemporaryDirectory(prefix='span-capture-',dir=ROOT/'tmp') as directory:
        work=Path(directory);(work/'test.c').write_text(prefix+f'#define SMG_PERSPECTIVE_SPAN_MAX {args.max_span}\n#define MAX_SPAN {args.max_span}\n'+helper+suffix)
        for i,capture in enumerate(args.captures):
            with (work/f'{i}.bin').open('wb') as f:
                for row in rows(capture):f.write(row)
        command=f'cc -O3 -DSMG_PERSPECTIVE_SPAN_MAX={args.max_span} -DMAX_SPAN={args.max_span} -DUSE_REUSE={int(args.reuse)} -DUSE_PRECHECK={int(args.precheck=="on")} -no-pie -fsanitize=address,undefined,float-cast-overflow test.c -lm -o test'
        command+=''.join(f' && ./test < {i}.bin' for i in range(len(args.captures)))
        try:
            result=subprocess.run(['docker','run','--rm','--name',work.name,'--user','1000:1000','-v',f'{work}:/test','-w','/test','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c',command],capture_output=True,text=True,check=True,timeout=90)
            lines=result.stdout.splitlines();assert len(lines)==len(args.captures),result.stdout
            for capture,line in zip(args.captures,lines):
                report=json.loads(line);report['capture']=str(capture);report['mean_span_pixels']=report['pixels_including_overdraw']/sum(report['span_histogram']);reports.append(report)
        except subprocess.CalledProcessError as e:
            print(e.stdout,e.stderr);raise
        finally:subprocess.run(['docker','rm','-f',work.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    result={'note':__doc__.strip(),'reuse_endpoints':args.reuse,'precheck':args.precheck,'max_span':args.max_span,'reports':reports};args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))


if __name__=='__main__':main()
