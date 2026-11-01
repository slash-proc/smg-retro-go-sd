#!/usr/bin/env python3
"""Read real stage home tables with the production C header layout in Docker."""
import json
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
source=r'''
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include "assets/asset_format.h"
int main(int argc,char**argv){
 printf("{\"header_bytes\":%zu,\"home_field_offset\":%zu,\"rows\":[",sizeof(StageHeader),offsetof(StageHeader,point_home));
 for(int i=1;i<argc;i++){
  FILE*f=fopen(argv[i],"rb");if(!f)return 2;StageHeader h;
  if(fread(&h,sizeof h,1,f)!=1)return 3;fseek(f,0,SEEK_END);long size=ftell(f);
  uint64_t bytes=h.point_home?(uint64_t)h.point_count*2:0;
  if(h.point_home && (uint64_t)h.point_home+bytes>(uint64_t)size)return 4;
  unsigned runs=0,largest=0,run=0,unused=0,render_refs=0;uint16_t previous=UINT16_MAX,highest=0;
  if(bytes){
   uint16_t* homes=malloc((size_t)bytes);if(!homes)return 5;
   if(fseek(f,h.point_home,SEEK_SET)||fread(homes,2,h.point_count,f)!=h.point_count)return 5;
   for(uint32_t p=0;p<h.point_count;p++){
    uint16_t leaf=homes[p];
    if(leaf==UINT16_MAX){unused++;continue;}
    if(leaf>=h.leaf_count)return 6;
    if(leaf!=previous){runs++;run=0;}run++;if(run>largest)largest=run;
    if(leaf>highest)highest=leaf;previous=leaf;
   }
   // number_points_by_leaf deliberately puts unreferenced source points at
   // the tail with homeFFFF. Render leaf records must never reference them.
   for(uint32_t t=0;t<h.leaf_triangle_count;t++){
    uint32_t offset=(h.surface_features&STAGE_INDEXED_LEAVES)?h.triangles+t*sizeof(StageTriangle):h.leaf_triangles+t*sizeof(LeafTriangle);
    uint16_t points[3];
    if(fseek(f,offset,SEEK_SET))return 7;
    if(h.surface_features&STAGE_INDEXED_LEAVES){StageTriangle tri;if(fread(&tri,sizeof tri,1,f)!=1)return 7;for(unsigned k=0;k<3;k++)points[k]=tri.v[k].point;}
    else {LeafTriangle tri;if(fread(&tri,sizeof tri,1,f)!=1)return 7;for(unsigned k=0;k<3;k++)points[k]=tri.points[k];}
    for(unsigned k=0;k<3;k++){if(points[k]>=h.point_count||homes[points[k]]==UINT16_MAX)return 8;render_refs++;}
   }
   free(homes);
  }
  fclose(f);
  printf("%s{\"index\":%d,\"points\":%u,\"leaves\":%u,\"home_offset\":%u,\"home_bytes\":%llu,\"runs\":%u,\"largest_run\":%u,\"highest_leaf\":%u,\"unused_points\":%u,\"valid_render_references\":%u}",i>1?",":"",i-1,h.point_count,h.leaf_count,h.point_home,(unsigned long long)bytes,runs,largest,highest,unused,render_refs);
 }
 puts("]}");return 0;
}
'''
paths=[p for p in sorted((ROOT/'build/assets').glob('stage_*.bin')) if 'shared' not in p.name and p.stat().st_size>=204]
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='home-audit-',dir=ROOT/'tmp') as directory:
    p=Path(directory);(p/'audit.c').write_text(source)
    try:
        result=subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{ROOT}:/source:ro','-v',f'{p}:/test','-w','/test','smg-runtime-tests','sh','-c',
            'cc -O2 -fsanitize=undefined -fno-sanitize-recover=all -I/source/src audit.c -o audit && exec ./audit "$@"','audit',*[f'/source/{q.relative_to(ROOT)}' for q in paths]],check=True,capture_output=True,text=True,timeout=60)
        report=json.loads(result.stdout)
    finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
for row,path in zip(report['rows'],paths):row['file']=str(path.relative_to(ROOT));row['fits_64KiB']=row['home_bytes']<=65536
print(json.dumps(report,indent=2))
