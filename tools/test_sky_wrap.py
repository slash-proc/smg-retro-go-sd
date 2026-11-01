#!/usr/bin/env python3
"""Check the production sky copier against independent per-row wrapping."""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "src/graphics/sky.c").read_text()
prefix = r'''
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <assert.h>
#include <string.h>
#define __GNW__ 1
#define IWRAM_FN
#define FX_ONE 256
#define FXC(x) ((int32_t)((x)*256))
#define TAU_F 6.283185307179586f
#ifndef FRAME_WIDTH
#define FRAME_WIDTH 160
#define FRAME_HEIGHT 120
#endif
#define FRAME_STRIDE 320
#define SKY_DIM 256
#define SKY_W 128
typedef int32_t fx;
static fx fx_mul(fx a,fx b){return (int64_t)a*b>>8;}
static int divide_euclidean(int a,int b){return a/b;}
'''
suffix = r'''
int main(void){
 uint8_t *sky=malloc(SKY_W*SKY_DIM),fb[FRAME_STRIDE*FRAME_HEIGHT];
 for(unsigned y=0;y<SKY_DIM;y++)for(unsigned x=0;x<SKY_W;x++)sky[y*SKY_W+x]=(y*31+x)%251;
 for(int yaw=-4096;yaw<=4096;yaw+=37)for(int pitch=-4096;pitch<=4096;pitch+=127){
  memset(fb,253,sizeof fb);draw_sky(fb,sky,yaw,pitch);
  int offset=fx_mul(fx_mul(yaw,FXC(-4.0f/TAU_F)),SKY_W<<8)>>8;
  unsigned col=(unsigned)(offset+(offset&1))&(SKY_W-2);
  int scale=(SKY_DIM<<8)/2/FRAME_HEIGHT;
  for(int row=0;row<FRAME_HEIGHT;row++){
   int y=((fx_mul(-pitch,FXC(.55f))*SKY_DIM+fx_mul((row-FRAME_HEIGHT/2)*256,scale))>>8)+SKY_DIM/2;
   y&=SKY_DIM-1;
   for(unsigned x=0;x<FRAME_WIDTH;x++)if(fb[row*FRAME_STRIDE+x]!=sky[y*SKY_W+(col+x)%SKY_W]){
    fprintf(stderr,"Sky crosses source row: yaw%d pitch%d row%d x%u col%u\n",yaw,pitch,row,x,col);abort();
   }
   for(unsigned x=FRAME_WIDTH;x<FRAME_STRIDE;x++)assert(fb[row*FRAME_STRIDE+x]==253);
  }
 }
 free(sky);printf("Sky row wrapping/stride/guard contracts pass at %dx%d\n",FRAME_WIDTH,FRAME_HEIGHT);
}
'''
(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix="sky-wrap-", dir=ROOT / "tmp") as directory:
    path = Path(directory)
    (path / "test.c").write_text(prefix + source + suffix)
    try:
        subprocess.run([
            "docker", "run", "--rm", "--name", path.name, "--user", f"{os.getuid()}:{os.getgid()}",
            "-v", f"{path}:/test", "-w", "/test", "smg-runtime-tests", "sh", "-c",
            "cc -O2 -fsanitize=address -no-pie test.c -o test && ./test && "
            "cc -O2 -fsanitize=address -no-pie -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 test.c -o native && ./native",
        ], check=True, timeout=60)
    finally:
        subprocess.run(["docker", "rm", "-f", path.name],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
