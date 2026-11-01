#!/usr/bin/env python3
"""Compile the production DTCM LUT storage code and verify copy/restore/fallback."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / 'src/graphics/visibility.c').read_text()
start = source.index('#if defined(__GNW__) && defined(SMG_ZBIN_RAM)')
end = source.index('IWRAM_CODE static inline uint32_t lowest_set_bit_index', start)
production = source[start:end]
prefix = r'''
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#define __GNW__ 1
#define SMG_ZBIN_DTCM 1
#define SMG_ZBIN_RAM 0
#define EWRAM_BSS
static uint8_t lut_zbin[2048];
'''
test = r'''
int main(void){
 uint8_t memory[2048];
 for(unsigned i=0;i<2048;i++)lut_zbin[i]=(uint8_t)((i*73u+19u)&255u);
 memset(memory,0xa5,sizeof memory);
 if(!smg_visibility_set_zbin_storage(memory,sizeof memory))return 1;
 if(!smg_zbin_dtcm_stats.active||smg_zbin_dtcm_stats.bytes!=2048||memcmp(memory,lut_zbin,2048))return 2;
 memset(memory,0,sizeof memory);smg_visibility_init();
 if(memcmp(memory,lut_zbin,2048)||smg_zbin_dtcm_stats.restore_count!=1)return 3;
 if(smg_visibility_set_zbin_storage(memory,2047)||smg_zbin_dtcm_stats.active||smg_zbin_dtcm_stats.bytes||g_lut_zbin_dtcm)return 4;
 if(smg_visibility_set_zbin_storage(NULL,2048)||smg_zbin_dtcm_stats.active||g_lut_zbin_dtcm)return 5;
 puts("production DTCM LUT copy, sleep/wake restore, undersized allocation rejection and XIP fallback pass");
 return 0;
}
'''

(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='zbin-dtcm-', dir=ROOT / 'tmp') as directory:
    path = Path(directory)
    (path / 'test.c').write_text(prefix + production + test)
    subprocess.run(['docker', 'run', '--rm', '--name', path.name, '--user', '1000:1000',
                    '-v', f'{path}:/test', '-w', '/test', 'smg-runtime-tests', 'sh', '-c',
                    'cc -O3 -no-pie -fsanitize=address,undefined -fno-sanitize-recover=all test.c -o test && ./test'],
                   check=True, timeout=60)
