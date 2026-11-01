#!/usr/bin/env python3
"""Check packed transform bounds and exact signed dot products in Docker.

The arithmetic oracle uses int64 products. Native tests exercise the production
packing guard and portable instruction semantics; ARM execution is separately
checked by the installed game's controlled replay, not emulated by this fixture.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / 'src/graphics/visibility.c').read_text()
start = source.index('static bool transform_pack_matrix(')
production = source[start:source.index('void engine_transform_points(', start)]
prefix = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>
typedef struct { int32_t m[3][4]; } TransformJob;
'''
suffix = r'''
static uint32_t seed=0x98765432;
static uint32_t rnd(void){seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;return seed;}
static unsigned checks, accepted, rejected;
static void check(TransformJob job, int16_t x,int16_t y,int16_t z){
 bool expected=true;
 for(unsigned row=0;row<3;row++){
  int64_t magnitude=0;
  for(unsigned k=0;k<3;k++){
   int64_t a=job.m[row][k];
   if(a<INT16_MIN||a>INT16_MAX)expected=false;
   magnitude+=a<0?-a:a;
  }
  if(magnitude>65535)expected=false;
 }
 uint32_t packed[3];bool actual=transform_pack_matrix(&job,packed);
 if(actual!=expected)abort();
 if(!actual){rejected++;return;}
 accepted++;
 uint32_t xy=(uint16_t)x|((uint32_t)(uint16_t)y<<16);
 for(unsigned row=0;row<3;row++){
  int64_t oracle=(int64_t)job.m[row][0]*x+(int64_t)job.m[row][1]*y+(int64_t)job.m[row][2]*z;
  if(oracle<INT32_MIN||oracle>INT32_MAX)abort();
  if(transform_dot_packed(xy,z,packed[row],job.m[row][2])!=oracle){
   fprintf(stderr,"packed dot mismatch at check %u\n",checks);exit(1);
  }
  checks++;
 }
}
int main(void){
 const int32_t coefficients[]={INT32_MIN,-32769,-32768,-32767,-1,0,1,32767,32768,INT32_MAX};
 const int16_t coords[]={INT16_MIN,INT16_MIN+1,-1,0,1,INT16_MAX};
 for(unsigned a=0;a<10;a++)for(unsigned b=0;b<10;b++)for(unsigned c=0;c<10;c++){
  TransformJob j={0};for(unsigned row=0;row<3;row++){
   j.m[row][row]=coefficients[a];j.m[row][(row+1)%3]=coefficients[b];j.m[row][(row+2)%3]=coefficients[c];
  }
  for(unsigned x=0;x<6;x++)for(unsigned y=0;y<6;y++)for(unsigned z=0;z<6;z++)check(j,coords[x],coords[y],coords[z]);
 }
 for(unsigned n=0;n<1000000;n++){
  TransformJob j={0};for(unsigned r=0;r<3;r++)for(unsigned k=0;k<3;k++)
   j.m[r][k]=n%2?(int16_t)rnd():(int)(rnd()%4097)-2048;
  check(j,(int16_t)rnd(),(int16_t)rnd(),(int16_t)rnd());
 }
 if(!accepted||!rejected)abort();
 printf("%u packed dot products match independent int64 oracle; %u accepted/%u rejected matrices, full int16 coordinates and signed32 coefficient bounds pass\n",checks,accepted,rejected);
}
'''
(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='transform-dsp-', dir=ROOT / 'tmp') as directory:
    path = Path(directory)
    (path / 'test.c').write_text(prefix + production + suffix)
    try:
        subprocess.run(['docker', 'run', '--rm', '--name', path.name, '--user', '1000:1000',
                        '-v', f'{path}:/test', '-w', '/test',
                        '-e', 'ASAN_OPTIONS=abort_on_error=1:handle_segv=0',
                        'smg-runtime-tests', 'sh', '-c',
                        'cc -O3 -fsanitize=address,undefined -no-pie test.c -o test && ./test'],
                       check=True, timeout=60)
    finally:
        subprocess.run(['docker', 'rm', '-f', path.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
