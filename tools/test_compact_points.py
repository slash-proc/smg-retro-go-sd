#!/usr/bin/env python3
"""Sanitized exact-position compact claims against a linear independent oracle."""
from pathlib import Path
import os
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include "assets/asset_format.h"
typedef struct {int32_t x,y,z;} v3;
typedef struct {v3 position;uint8_t clip,mark;uint16_t pad;} FramePoint;
#include "graphics/compact_points.h"
static uint32_t rng=0xa371293b;
static uint32_t next(void){rng^=rng<<13;rng^=rng>>17;rng^=rng<<5;return rng;}
int main(void){
 uint16_t map[256],list[256],buckets[1024],visible[128],expected[256];
 FramePoint pool[256];StageTriangle triangles[128];int16_t xyz[256][3];
 unsigned saturated=0,aliases=0,full_reuse=0;
 for(unsigned test=0;test<30000;test++){
  for(unsigned p=0;p<256;p++){
   // Entirely stale map/pool, repeated stamps and different source coordinates
   // model stage changes, stamp wrap and overwritten collision scratch.
   if(test%4==0)map[p]=(uint16_t)next();
   pool[p].position=(v3){(int32_t)next(),(int32_t)next(),(int32_t)next()};
   pool[p].mark=(uint8_t)next();pool[p].pad=(uint16_t)next();
   for(unsigned k=0;k<3;k++)xyz[p][k]=(int16_t)next();
   if(p&&test%3)memcpy(xyz[p],xyz[next()%p],sizeof xyz[p]);
   list[p]=expected[p]=UINT16_MAX;
  }
  for(unsigned t=0;t<128;t++){
   visible[t]=t;for(unsigned k=0;k<3;k++)triangles[t].v[k].point=next()%256;
  }
  if(test%7==0)for(unsigned t=1;t<128;t++)triangles[t]=triangles[0];
  unsigned cap=test%7==0?3:next()%130,count=test%13==0?0:128;
  unsigned used=0,admitted=0;
  for(unsigned t=0;t<count;t++){
   uint16_t newly[3];unsigned needed=0;
   for(unsigned k=0;k<3;k++){
    unsigned p=triangles[t].v[k].point;bool found=false;
    for(unsigned j=0;j<used;j++)if(!memcmp(xyz[p],xyz[expected[j]],sizeof xyz[p]))found=true;
    for(unsigned j=0;j<needed;j++)if(!memcmp(xyz[p],xyz[newly[j]],sizeof xyz[p]))found=true;
    if(!found)newly[needed++]=p;
   }
   if(used+needed>cap)break;
   for(unsigned j=0;j<needed;j++)expected[used++]=newly[j];admitted++;
  }
  CompactPointJob job={.visible=visible,.count=count,.triangles=triangles,.source=xyz,
   .slots=map,.pool=pool,.list=list,.buckets=buckets,.bucket_count=test%2?1024:1,
   .capacity=cap,.stamp=test%2?1:255};
  geometry_claim_compact_points(&job);
  if(job.used!=used||job.admitted!=admitted||memcmp(list,expected,sizeof list))abort();
  for(unsigned i=0;i<used;i++)if(!compact_same_position(&pool[i],xyz[list[i]]))abort();
  for(unsigned t=0;t<admitted;t++)for(unsigned k=0;k<3;k++){
   unsigned p=triangles[t].v[k].point;if(map[p]>=used||!compact_same_position(&pool[map[p]],xyz[p]))abort();
  }
  if(admitted<count)saturated++;if(job.position_hits)aliases++;if(admitted==128&&used==cap)full_reuse++;
 }
 if(!saturated||!aliases||!full_reuse)abort();
 printf("30000 compact claim oracle jobs pass: %u saturated, %u coordinate-reuse, %u full-pool reuse; stale maps/stamps/stages, collision scratch, duplicate faces, one-bucket chains and zero capacities\n",saturated,aliases,full_reuse);
}
'''
with tempfile.TemporaryDirectory(prefix="compact-points-",dir=ROOT/"tmp") as directory:
 p=Path(directory);(p/"test.c").write_text(prefix)
 try:
  subprocess.run(["docker","run","--rm","--name",p.name,"--user",f"{os.getuid()}:{os.getgid()}","-v",f"{ROOT}:/source:ro","-v",f"{p}:/test","-w","/test","smg-runtime-tests","sh","-c",'cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined -fno-sanitize-recover=all -I/source/src test.c -o test && exec ./test'],check=True,timeout=90)
 finally:subprocess.run(["docker","rm","-f",p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
