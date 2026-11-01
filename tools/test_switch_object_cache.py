#!/usr/bin/env python3
"""Verify cached watcher iteration against full source-order scans in Docker.

Uses production switch state and group logic, randomized stage transitions,
scenario changes and persistent object state, plus actual linked stage-object data.
"""
from pathlib import Path
import subprocess
import re
import struct
from elftools.elf.elffile import ELFFile
import tempfile

ROOT=Path(__file__).resolve().parents[1]
objects=(ROOT/'src/actors/objects/switch_objects.c').read_text()
group=(ROOT/'src/actors/objects/group_switch.c').read_text().split('static void group_switch_update')[0]
switches=(ROOT/'src/world/switches.c').read_text().split('static inline int stage_object_switch')[0]
# The independent oracle walks every object. It shares behavioral rules but
# never consults candidate membership, count, indices or cache invalidation.
generic=objects[objects.index('static void switch_objects_tick(void)'):objects.index('static void group_switch_objects_tick(void)')]
generic=generic.replace('switch_objects_tick(void)','reference_switch_tick(void)').replace('bool listed = switch_objects_prepare(st);','bool listed = false;')
watcher=objects[objects.index('static void group_switch_objects_tick(void)'):]
watcher=watcher.replace('group_switch_objects_tick(void)','reference_group_tick(void)').replace('bool listed = switch_objects_prepare(stage);','bool listed = false;')
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "assets_gen.h"
#define EWRAM_BSS
#define OBJECT_STATE_GONE 2
#define OBJECT_STATE_DEAD 8
#define CAPACITY 4096
typedef struct {const StageHeader* header;const StageObject* objects;} Stage;
static struct {Stage stage;int stage_index;} g_game;
static struct {uint8_t scenario_bit;} g_stream;
static uint8_t g_object_state[CAPACITY];
static unsigned stream_part_state(unsigned i){return g_object_state[i]>>4;}
'''
suffix=r'''
static uint32_t seed=0x1234abcd;
static uint32_t rnd(void){seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;return seed;}
static unsigned comparisons,overflow_cases;
static void compare_tick(bool group_only){
 uint8_t before[CAPACITY],expected[CAPACITY];
 int16_t old_switches[SWITCHES_ON_MAX],expected_switches[SWITCHES_ON_MAX];
 unsigned char hints[sizeof g_chip_hint],expected_hints[sizeof g_chip_hint];
 memcpy(before,g_object_state,sizeof before);memcpy(hints,g_chip_hint,sizeof hints);
 memcpy(old_switches,g_switches_on,sizeof old_switches);int old_count=g_switch_count;
 uint32_t old_generation=g_switch_generation;
 if(group_only)reference_group_tick();else reference_switch_tick();
 memcpy(expected,g_object_state,sizeof expected);memcpy(expected_hints,g_chip_hint,sizeof expected_hints);
 memcpy(expected_switches,g_switches_on,sizeof expected_switches);int expected_count=g_switch_count;
 uint32_t expected_generation=g_switch_generation;
 memcpy(g_object_state,before,sizeof before);memcpy(g_chip_hint,hints,sizeof hints);
 memcpy(g_switches_on,old_switches,sizeof old_switches);g_switch_count=old_count;
 g_switch_generation=old_generation;
 if(group_only)group_switch_objects_tick();else switch_objects_tick();
 if(memcmp(expected,g_object_state,sizeof expected)||expected_count!=g_switch_count||
    memcmp(expected_switches,g_switches_on,sizeof expected_switches)||expected_generation!=g_switch_generation||
    memcmp(expected_hints,g_chip_hint,sizeof expected_hints)){
  fprintf(stderr,"Cache differs from full scan at tick %u, group_only=%d\n",comparisons,group_only);exit(1);
 }
 comparisons++;
}
static void random_state(void){
 switches_reset();
 for(int i=0;i<64;i++)if(rnd()%3==0)switch_set(i,true);
 for(unsigned i=0;i<g_game.stage.header->object_count;i++)g_object_state[i]=(rnd()%7==0?OBJECT_STATE_GONE:0)|(rnd()%16<<4);
}
int main(int argc,char**argv){
 setvbuf(stdout,NULL,_IOLBF,0);
 StageObject banks[2][512];StageHeader headers[2]={0};
 // Explicit dependent one-shot chain: row 1 observes row 0's write.
 memset(banks,0,sizeof banks);headers[0].object_count=2;
 for(unsigned i=0;i<2;i++){
  banks[0][i].type=OBJ_GROUPSWITCHWATCHER;banks[0][i].scenarios=0xff;
  banks[0][i].sw[SWITCH_A]=10+i;banks[0][i].sw[SWITCH_B]=9+i;
  banks[0][i].args[0]=1;banks[0][i].args[1]=0;banks[0][i].args[2]=-1;
 }
 g_game.stage=(Stage){&headers[0],banks[0]};g_stream.scenario_bit=1;
 switches_reset();switch_set(9,true);compare_tick(true);
 if(!switch_on(10)||!switch_on(11)||!(g_object_state[0]&OBJECT_STATE_GONE)||!(g_object_state[1]&OBJECT_STATE_GONE))abort();
 compare_tick(true);g_switch_objects.objects=NULL;
 const unsigned counts[]={0,4,14,15,32,128};
 for(unsigned run=0;run<3000;run++){
  unsigned bank=run%2,count=counts[run%6];headers[bank].object_count=356;
  StageObject* o=banks[bank];memset(o,0,sizeof banks[bank]);
  for(unsigned i=0;i<356;i++){
   o[i].type=i<count?(i%3==0?OBJ_GROUPSWITCHWATCHER:i%3==1?OBJ_YELLOWCHIPGROUP:OBJ_SWITCHSYNCREV):OBJ_YELLOWCHIP;
   o[i].scenarios=rnd()%3?0xff:1u<<(rnd()%6);o[i].zone=rnd()%3;
   for(int s=0;s<5;s++)o[i].sw[s]=(int)(rnd()%68)-4;
   o[i].args[0]=1+rnd()%8;o[i].args[1]=(int)(rnd()%3)-1;o[i].args[2]=(int)(rnd()%2)-1;
  }
  g_game.stage=(Stage){&headers[bank],o};g_game.stage_index=bank;
  memset(g_chip_hint,0,sizeof g_chip_hint);random_state();
  for(unsigned tick=0;tick<8;tick++){
   g_stream.scenario_bit=1u<<(tick%6);compare_tick(tick%2==0);
   if(tick==0){
    if(g_switch_objects.count!=count){fprintf(stderr,"run %u: expected %u cached candidates, got %u\n",run,count,g_switch_objects.count);abort();}
    if(count>14)overflow_cases++;
   }
  }
 }
 for(int file=1;file<argc;file++){
  FILE* f=fopen(argv[file],"rb");if(!f)abort();fseek(f,0,SEEK_END);long n=ftell(f);rewind(f);
  unsigned char* blob=malloc(n);if(fread(blob,1,n,f)!=(size_t)n)abort();fclose(f);
  StageHeader* h=(StageHeader*)blob;g_game.stage=(Stage){h,(StageObject*)(blob+h->objects)};
  if(h->object_count>CAPACITY){fprintf(stderr,"%s: invalid object count %u\n",argv[file],h->object_count);abort();}g_game.stage_index=file;
  // Stages are immutable and normally retain distinct XIP addresses. These
  // fixture allocations may reuse addresses after free, so invalidate here.
  g_switch_objects.objects=NULL;memset(g_chip_hint,0,sizeof g_chip_hint);random_state();
  unsigned watchers=0;for(unsigned i=0;i<h->object_count;i++)watchers+=g_game.stage.objects[i].type==OBJ_GROUPSWITCHWATCHER;
  for(unsigned tick=0;tick<24;tick++){g_stream.scenario_bit=1u<<(tick%6);compare_tick(tick%2==0);}
  printf("%s: %u objects, %u watchers, %u cached candidates, %s\n",argv[file],h->object_count,watchers,g_switch_objects.count,g_switch_objects.count<=14?"listed":"full-scan fallback");
  free(blob);
 }
 printf("%u exact state/switch/order/generation comparisons; %u overflow fallback stages, scenario changes and persistent one-shot state\n",comparisons,overflow_cases);
 return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='switch-cache-',dir=ROOT/'tmp') as directory:
    path=Path(directory);(path/'test.c').write_text(prefix+switches+group+objects+generic+watcher+suffix)
    # Stage object arrays can be pooled outside each raw .bin. Read their
    # linked relative offsets from the SDK ELF instead of treating a zero
    # pre-link placeholder as an in-file object array.
    names=re.findall(r'extern const uint8_t (stage_[a-z_]+)\[\];',(ROOT/'build/assets/assets_gen.h').read_text())
    inputs=[]
    with (ROOT/'retro-go-sd/build/smg/smg_core.elf').open('rb') as source:
        elf=ELFFile(source);section=elf.get_section_by_name('.xip_smg_assets')
        assets=section.data();base=section['sh_addr'];symbols=elf.get_section_by_name('.symtab')
        def read(address,size):
            offset=address-base
            assert 0<=offset<=offset+size<=len(assets)
            return assets[offset:offset+size]
        for name in names:
            if name.endswith('_far'):continue
            address=symbols.get_symbol_by_name(name)[0]['st_value']
            header=bytearray(read(address,204));count,offset=struct.unpack_from('<2I',header,96)
            objects=read((address+offset)&0xffffffff,count*84)
            struct.pack_into('<I',header,100,len(header))
            (path/f'{name}.bin').write_bytes(header+objects)
            inputs.append(f'/test/{name}.bin')
    try:
        subprocess.run(['docker','run','--rm','--name',path.name,'--user','1000:1000',
            '-v',f'{ROOT}:/work:ro','-v',f'{path}:/test','-w','/test',
            '-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c',
            'cc -I/work/src -I/work/build/assets -O3 -fno-strict-aliasing -no-pie -fsanitize=address test.c -o test && ./test ' + ' '.join(inputs)],check=True,timeout=60)
    finally:
        subprocess.run(['docker','rm','-f',path.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
