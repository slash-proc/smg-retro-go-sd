#!/usr/bin/env python3
"""Exercise the actual flower textures through the production mip compiler."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
HARNESS = r'''
#include "tools/asset_compiler/model_builder.c"
#include <assert.h>

int main(void) {
    /* Check endpoints and rounded partial alpha independently of real assets. */
    uint8_t f[] = {200,100,50,0, 200,100,50,255, 200,100,50,128};
    uint8_t b[] = {20,40,60,255, 20,40,60,255, 20,40,60,255};
    Image fi = {3,1,f}, bi = {3,1,b};
    texture_apply_underlay(&fi,&bi);
    const uint8_t expected[] = {20,40,60,255, 200,100,50,255, 110,70,55,255};
    assert(!memcmp(f,expected,sizeof f));

    Rgba palette[256];
    size_t size;
    void *raw=read_file("build/constructed/assets/palette.rgba",&size);
    assert(raw && size==sizeof palette);
    memcpy(palette,raw,size);free(raw);
    Image base=png_load("build/constructed/assets/stages/observatory/grass.png");
    const char *names[]={"flowers","pinkflowers","whiteflowers"};
    for(unsigned n=0;n<3;n++){
        char path[200];
        snprintf(path,sizeof path,"build/constructed/assets/stages/observatory/%s.png",names[n]);
        Image flower=png_load(path);
        Texture before,after;
        int alpha=0;
        texture_build_mip(before.mip[0],&flower,0,1,0,&alpha,palette);
        assert(alpha); /* Reproduces the actual holes. */
        texture_apply_underlay(&flower,&base);
        alpha=0;
        texture_build_mip(after.mip[0],&flower,0,1,0,&alpha,palette);
        texture_build_mip1(after.mip[1],&flower,1,0,&alpha,palette);
        assert(!alpha);
        for(int i=0;i<1024;i++)assert(after.mip[0][i]);
        for(int row=0;row<16;row++)for(int col=0;col<32;col++)
            assert(after.mip[1][row*64+col]);
        image_free(&flower);
    }
    image_free(&base);
    puts("Three flower textures: original holes reproduced; both repaired mips opaque. Alpha endpoints and blend rounding pass.");
    return 0;
}
'''

with tempfile.TemporaryDirectory(prefix="underlay-test-", dir=ROOT / "tmp") as directory:
    path = Path(directory)
    (path / "test.c").write_text(HARNESS)
    relative = path.relative_to(ROOT)
    try:
        subprocess.run([
            "docker", "run", "--rm", "--name", path.name, "--user", "1000:1000",
            "-v", f"{ROOT}:/work", "-w", "/work", "smg-runtime-tests", "sh", "-c",
            f"cc -std=c11 -D_DEFAULT_SOURCE -O2 -ffunction-sections -fdata-sections "
            f"-fsanitize=address -no-pie -I. {relative}/test.c "
            "tools/asset_compiler/common.c tools/asset_compiler/png_reader.c "
            "tools/asset_compiler/palette.c -Wl,--gc-sections -lm "
            f"-o {relative}/test && ASAN_OPTIONS=handle_segv=0 {relative}/test",
        ], check=True, timeout=60)
    finally:
        subprocess.run(["docker", "rm", "-f", path.name],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
