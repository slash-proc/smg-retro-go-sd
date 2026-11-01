#!/usr/bin/env python3
"""Check loaded-code relocation bounds and fixed veneer pointers in Docker."""
from pathlib import Path
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
source=(ROOT/'retro-go-sd/src/smg_xip.c').read_text()
start=source.index('static bool smg_loaded_reloc_site(')
end=source.index('static void smg_patch_asset_reloc_sites(',start)
production=source[start:end]
prefix=r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
extern uint8_t __RAM_EMU_START__[],__ITCM_CORE_START__[],__CORE_ITCM_CODE_END__[];
extern uint32_t __CORE_CODE_END__;
'''
suffix=r'''
int main(void){
 const uint32_t valid[]={0x2404b000,0x2405fffc};
 const uint32_t invalid[]={1,3,0x1b06,0x1b08,0x2404affc,0x24060000,0x24060004,0xfffffffc};
 for(unsigned i=0;i<sizeof valid/sizeof *valid;i++)if(!smg_loaded_reloc_site(valid[i]))abort();
 for(unsigned i=0;i<sizeof invalid/sizeof *invalid;i++)if(smg_loaded_reloc_site(invalid[i]))abort();
#ifdef TEST_ITCM
 if(!smg_loaded_reloc_site(0)||!smg_loaded_reloc_site(4)||!smg_loaded_reloc_site(0x1b04))abort();
#else
 if(smg_loaded_reloc_site(0)||smg_loaded_reloc_site(4))abort();
#endif
 // Fixed-address veneers are recognized pointer sites but never rebased as XIP.
 const uint32_t fixed[]={1,0x1235,0x24050001};
 for(unsigned i=0;i<3;i++){
  uint32_t word=fixed[i];if(smg_rebase_word(&word,0xdec00000,0x200000,0xb0000000)||word!=fixed[i])abort();
 }
 uint32_t mapped=0xdec01235;
 if(!smg_rebase_word(&mapped,0xdec00000,0x200000,0xb0000000)||mapped!=0x8ec01235)abort();
 puts("Loaded relocation bounds: aligned code-only AXI/optional ITCM including origin 0; fixed veneers stay unchanged, Thumb XIP relocation preserved");
 return 0;
}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='loaded-sites-',dir=ROOT/'tmp') as directory:
    path=Path(directory);(path/'test.c').write_text(prefix+production+suffix)
    common='cc -O3 -no-pie -fsanitize=address,undefined -Wl,--defsym,__RAM_EMU_START__=0x2404b000 -Wl,--defsym,__CORE_CODE_END__=0x24060000 -Wl,--defsym,__ITCM_CORE_START__=0 '
    try:
        subprocess.run(['docker','run','--rm','--name',path.name,'--user','1000:1000','-v',f'{path}:/test','-w','/test',
            '-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c',
            common+'-DTEST_ITCM -Wl,--defsym,__CORE_ITCM_CODE_END__=0x1b08 test.c -o test-itcm && ./test-itcm && '+
            common+'-Wl,--defsym,__CORE_ITCM_CODE_END__=0 test.c -o test-axi && ./test-axi'],check=True,timeout=60)
    finally:
        subprocess.run(['docker','rm','-f',path.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
