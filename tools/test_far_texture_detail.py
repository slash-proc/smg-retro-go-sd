#!/usr/bin/env python3
"""Cross-check C and Python far-record decoding and legacy average compatibility."""
from pathlib import Path
import subprocess, tempfile, struct, sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools/assets'))
from textures import far_texture_payload
source=r'''
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include "assets/asset_format.h"
int main(void){
 FILE*f=fopen("records.bin","wb");if(!f)abort();uint32_t rng=0x35ae813d;
 for(unsigned i=0;i<50000;i++){
  rng=rng*1664525u+1013904223u;uint32_t offset=rng&0x00fffffcu;
  unsigned average=1+i%255;uint32_t record=FAR_TEXTURE_AVERAGE|FAR_TEXTURE_DETAIL|((offset/4)<<8)|average;
  if(!far_texture_has_pixels(record)||far_texture_source_offset(record)!=offset||(uint8_t)record!=average)abort();
  if(far_texture_has_pixels(FAR_TEXTURE_AVERAGE|average)||!far_texture_has_pixels(offset)||far_texture_source_offset(offset)!=offset)abort();
  uint32_t row[]={offset,record};if(fwrite(row,sizeof row,1,f)!=1)abort();
 }
 fclose(f);puts("50000 C far-record detail/raw/legacy-average checks pass");
}
'''
stage=(ROOT/'src/world/stage.c').read_text()
a=stage.index('static const TextureReference* stage_far_raw_texture(')
function=stage[a:stage.index('\nstatic __attribute__((noinline)) uint8_t stage_raw_texture_average',a)]
stub='static StageHeader far_header; static TextureReference far_refs[3]; static uint32_t far_records[3];\nstatic struct {const StageHeader* header;const TextureReference* textures;} g_far_stage;\nstatic const uint32_t* stage_far_records(const void* stage){(void)stage;if(!(far_header.surface_features&STAGE_PACKED_FAR_TEXTURES))abort();return far_records;}\n'
source=source.replace('int main(void){',stub+function+'\nint main(void){\n g_far_stage.header=&far_header;g_far_stage.textures=far_refs;\n if(stage_far_raw_texture(0)!=&far_refs[0])abort();\n far_header.surface_features=STAGE_PACKED_FAR_TEXTURES;far_records[0]=0x80000007;far_records[1]=0xc0000507;far_records[2]=0x1000;\n if(stage_far_raw_texture(0)||stage_far_raw_texture(1)!=&far_refs[1]||stage_far_raw_texture(2)!=&far_refs[2])abort();\n')
with tempfile.TemporaryDirectory(prefix='far-texture-detail-',dir=ROOT/'tmp') as directory:
 p=Path(directory);(p/'test.c').write_text(source)
 try:
  subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{ROOT}:/source:ro','-v',f'{p}:/test','-w','/test','smg-runtime-tests','sh','-c','cc -O2 -no-pie -fsanitize=address,undefined -fno-sanitize-recover=all -I/source/src test.c -o test && exec ./test'],check=True,timeout=60)
  for offset,record in struct.iter_unpack('<2I',(p/'records.bin').read_bytes()):
   assert far_texture_payload(record)==offset
   assert far_texture_payload(offset)==offset
   assert far_texture_payload(0x80000000|(record&255)) is None
  print('50000 Python production-packer detail/raw/legacy records match independently generated offsets')
 finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
