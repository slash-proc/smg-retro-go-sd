#!/usr/bin/env python3
"""Check all raw-depth quotient buckets against the original saturation rule.

For positive signed raw values, both expressions depend only on bits15..30;
bucket0 is uniformly clamped to one, and the entire negative half maps to one.
Check all131072 buckets at four low-bit positions plus random full32 values.
"""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / 'src/graphics/engine.c').read_text()
begin = source.index('__attribute__((always_inline)) static inline uint32_t raster_depth_quantize(')
end = source.index('\n}', begin) + 2
production = source[begin:end]
prefix = '#include <stdint.h>\n#include <assert.h>\n#include <stdio.h>\n'
suffix = r'''
static uint32_t state=0x273051ab;
static uint32_t random32(void){state^=state<<13;state^=state>>17;state^=state<<5;return state;}
static uint32_t original(uint32_t raw){
 uint32_t value=(int32_t)raw>0?raw>>15:1;
 if(!value)value=1;
 if(value>65535)value=65535;
 return value;
}
int main(void){
 const uint32_t low[]={0,1,16384,32767};uint64_t checksum=0;
 for(uint32_t bucket=0;bucket<131072;bucket++)for(unsigned i=0;i<4;i++){
  uint32_t raw=(bucket<<15)|low[i],expected=bucket&&bucket<65536?bucket:1;
  uint32_t actual=raster_depth_quantize(raw);assert(actual==expected&&actual==original(raw));checksum+=actual;
 }
 for(unsigned i=0;i<1000000;i++){
  uint32_t raw=random32(),actual=raster_depth_quantize(raw);
  assert(actual==original(raw)&&actual>=1&&actual<=65535);checksum+=actual;
 }
 printf("All131072 quotient/sign buckets at4 low-bit positions +1000000 full32 random values exact vs original depth rule; checksum%llu\n",(unsigned long long)checksum);
}
'''
(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='depth-quantize-', dir=ROOT / 'tmp') as directory:
    work = Path(directory)
    (work / 'test.c').write_text(prefix + production + suffix)
    try:
        subprocess.run(['docker', 'run', '--rm', '--name', work.name, '--user', f'{os.getuid()}:{os.getgid()}',
                        '-v', f'{work}:/test', '-w', '/test', 'smg-runtime-tests', 'sh', '-c',
                        'cc -O3 -fsanitize=address,undefined -no-pie test.c -o test && ./test'], check=True, timeout=60)
    finally:
        subprocess.run(['docker', 'rm', '-f', work.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
