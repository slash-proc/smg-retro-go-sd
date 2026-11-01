#!/usr/bin/env python3
"""Check that resolving the next keyframe cannot overwrite a live pose input."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "src/world/model.c").read_text()
start = source.index("#define GNW_POSE_SLOTS")
source = source[start:source.index("\n#else", start)]
prefix = r'''
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#define EWRAM_BSS
#define FIGURE_PACKED_MAX_POINTS 8
#define FIGURE_POSE_TAG 1
#define FIGURE_POSE_RAW 0
typedef struct {uint16_t mode,point_count;} PackedPoseHeader;
typedef struct {PackedPoseHeader header;int16_t value[24];} Descriptor;
static const int16_t* pose_decode(const uint8_t* data,int16_t* out){
    const Descriptor* d=(const Descriptor*)data;
    memcpy(out,d->value,sizeof d->value);return out;
}
'''
suffix = r'''
int main(void){
    _Alignas(4) Descriptor keys[3]={0};
    for(int k=0;k<3;k++){
        keys[k].header.mode=1;keys[k].header.point_count=8;
        for(int i=0;i<24;i++)keys[k].value[i]=100*k+i;
    }
    model_invalidate_pose_cache();
    model_resolve_packed((uintptr_t)&keys[0]|1,0);
    model_resolve_packed((uintptr_t)&keys[1]|1,1);
    /* On the next animation interval, old B becomes new A. */
    const int16_t (*a)[3]=model_resolve_packed((uintptr_t)&keys[1]|1,0);
    const int16_t (*b)[3]=model_resolve_packed((uintptr_t)&keys[2]|1,1);
    if(a[0][0]!=100||b[0][0]!=200){
        fprintf(stderr,"live interpolation input overwritten: A=%d B=%d, expected 100/200\n",a[0][0],b[0][0]);return 1;
    }
    /* Crossfade/render slots must not lend out any other live input slot. */
    for(unsigned slot=2;slot<GNW_POSE_SLOTS;slot++){
        const int16_t (*c)[3]=model_resolve_packed((uintptr_t)&keys[1]|1,slot);
        model_resolve_packed((uintptr_t)&keys[0]|1,0);
        if(c[0][0]!=100){fprintf(stderr,"slot %u borrowed an overwritten slot\n",slot);return 1;}
    }
    puts("Pose-cache key transitions and crossfade/render slots preserve live interpolation inputs.");
    return 0;
}
'''
with tempfile.TemporaryDirectory(prefix="pose-test-", dir=ROOT / "tmp") as directory:
    path = Path(directory)
    (path / "test.c").write_text(prefix + source + suffix)
    try:
        subprocess.run(["docker", "run", "--rm", "--name", path.name, "--user", "1000:1000",
            "-v", f"{path}:/test", "-w", "/test", "smg-runtime-tests", "sh", "-c",
            "cc -O3 -fno-strict-aliasing -no-pie -fsanitize=address test.c -o test && ASAN_OPTIONS=handle_segv=0 ./test"],
            check=True, timeout=60)
    finally:
        subprocess.run(["docker", "rm", "-f", path.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
