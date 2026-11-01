#!/usr/bin/env python3
"""Check the shipped projection table, rounding and GNW divider in Docker.

Uses the linked SDK ELF's table and an independent wide-integer arithmetic
oracle. Covers every reachable denominator, z-clamp boundaries, signed screen
rounding and packed coordinates at both current and native render sizes.
"""
from pathlib import Path
import hashlib
import struct
import subprocess
import tempfile
from elftools.elf.elffile import ELFFile

ROOT=Path(__file__).resolve().parents[1]
frame=(ROOT/'src/graphics/frame.c').read_text()
production=frame[:frame.index('static inline uint8_t frame_get')]
elf_path=ROOT/'retro-go-sd/build/smg/smg_core.elf'
with elf_path.open('rb') as source:
    elf=ELFFile(source);sym=elf.get_section_by_name('.symtab').get_symbol_by_name('lut_project')[0]
    section=elf.get_section(sym['st_shndx']);offset=sym['st_value']-section['sh_addr']
    table=section.data()[offset:offset+65536*4]
assert len(table)==262144
entries=struct.unpack('<65536i',table)
assert all(entries[i]==1048576//i for i in range(32,65536))
print(f'Linked table {hashlib.sha256(table).hexdigest()}: all 65504 reachable entries equal floor(1048576/index)',flush=True)
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <limits.h>
#include <sys/mman.h>
#define __GNW__ 1
#define IWRAM_CODE
#ifndef FRAME_WIDTH
#define FRAME_WIDTH 160
#define FRAME_HEIGHT 120
#endif
#define FX_ONE 256
#define FXC(x) ((int32_t)((x)*256))
typedef struct {int32_t x,y,z;} v3;
typedef struct {int32_t x,y;} v2;
static v2 V2(int32_t x,int32_t y){return (v2){x,y};}
static int32_t* lut_project;
'''
suffix=r'''
static uint32_t seed=0x1234abcd;
static uint32_t rnd(void){seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;return seed;}
static int32_t floor_shift(int64_t value,unsigned bits){
 return value<0?-(int32_t)(((uint64_t)-value+((1u<<bits)-1))>>bits):(int32_t)(value>>bits);
}
static unsigned cases;
static void check(v3 p){
 uint32_t index=(uint32_t)(p.z<128?128:p.z)/4;
 if(index>65535)index=65535;
 int32_t r=lut_project[index];
 v2 expected={floor_shift((int64_t)r*p.x,14)*(FRAME_WIDTH/2)+(FRAME_WIDTH/2)*256,
              floor_shift((int64_t)r*p.y,14)*(FRAME_HEIGHT/2)+(FRAME_HEIGHT/2)*256};
 for(unsigned divide=0;divide<2;divide++){
  g_projection_divide_enabled=divide;v2 actual=project_to_screen(p);
  uint32_t packed=(uint16_t)floor_shift(expected.x,4)|((uint32_t)(uint16_t)floor_shift(expected.y,4)<<16);
  if(actual.x!=expected.x||actual.y!=expected.y||screen_pack_position(actual)!=packed){
   fprintf(stderr,"Projection differs at %d,%d,%d, divider=%u\n",p.x,p.y,p.z,divide);exit(1);
  }
 }
 cases++;
}
int main(void){
 lut_project=mmap(NULL,262144,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);
 if(lut_project==MAP_FAILED)abort();FILE*f=fopen("table.bin","rb");if(!f)abort();
 if(fread(lut_project,4,65536,f)!=65536)abort();fclose(f);
 // All four z values mapping to every reachable denominator; varied signed XY.
 for(unsigned index=32;index<65536;index++)for(unsigned sub=0;sub<4;sub++)
  check((v3){(int32_t)((index*7919)&131071)-65536,(int32_t)((index*4507)&131071)-65536,index*4+sub});
 const int32_t zs[]={INT_MIN,-1,0,1,127,128,129,159,160,161,255,256,262139,262140,262143,262144,262145,1073741824,INT_MAX};
 const int32_t xy[]={-65536,-32768,-257,-1,0,1,257,65535};
 for(unsigned z=0;z<sizeof zs/sizeof *zs;z++)for(unsigned x=0;x<8;x++)for(unsigned y=0;y<8;y++)check((v3){xy[x],xy[y],zs[z]});
 for(unsigned n=0;n<100000;n++)check((v3){(int)(rnd()&131071)-65536,(int)(rnd()&131071)-65536,(int32_t)rnd()});
 // Divider mode must not fetch table bytes, even speculatively.
 g_projection_divide_enabled=true;
 if(mprotect(lut_project,262144,PROT_NONE))abort();
 volatile int32_t denominator=12345;v2 result=project_to_screen((v3){-1234,5678,denominator});
 if(result.x==INT_MIN)abort();
 if(mprotect(lut_project,262144,PROT_READ|PROT_WRITE))abort();munmap(lut_project,262144);
 printf("%u projections x 2 modes match independent rounding/packing oracle at %dx%d; protected-table divider succeeds\n",cases,FRAME_WIDTH,FRAME_HEIGHT);
 return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='projection-',dir=ROOT/'tmp') as directory:
    path=Path(directory);(path/'table.bin').write_bytes(table);(path/'test.c').write_text(prefix+production+suffix)
    try:
        subprocess.run(['docker','run','--rm','--name',path.name,'--user','1000:1000',
            '-v',f'{path}:/test','-w','/test','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0',
            'smg-runtime-tests','sh','-c',
            'cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined test.c -o test && ./test && '
            'cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 test.c -o test-native && ./test-native'],check=True,timeout=60)
    finally:
        subprocess.run(['docker','rm','-f',path.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
