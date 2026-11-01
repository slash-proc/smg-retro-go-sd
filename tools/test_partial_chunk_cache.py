#!/usr/bin/env python3
"""Validate the opt-in bounded cache for oversized visibility chunk tables."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
VIS = (ROOT / "src/graphics/visibility.c").read_text()
HDR = (ROOT / "src/graphics/visibility.h").read_text()
types_at = HDR.index("typedef struct {\n    const StageChunk*")
types = HDR[types_at:HDR.index("} CullJob;", types_at)] + "} CullJob;\n"
cull_marker = VIS.index("/* Cortex-M7 is Thumb-only; the GBA's ARM-state hierarchy walk cannot be used. */")
cull_at = VIS.rfind("#if defined(__GNW__)", 0, cull_marker)
cull_end = VIS.index("\n_Static_assert(offsetof(CullJob", cull_at)
cull = VIS[cull_at:cull_end]

prefix = r'''
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include "assets/asset_format.h"
#define abort() do { fprintf(stderr,"fixture assertion at C line %d\n",__LINE__); exit(2); } while(0)
#define __GNW__ 1
#define SMG_PARTIAL_CHUNK_CACHE 1
#define SMG_COUNT(field) ((void)0)
#define SMG_ADD(field,count) ((void)(count))
#define SMG_PEAK(field,value) ((void)0)
'''

suffix = r'''
static _Alignas(32) uint8_t cache_storage[39040];
static unsigned checks;

static void setup_job(CullJob* j, const StageChunk* chunks, uint32_t n, uint16_t* leaves){
    *j=(CullJob){.chunks=chunks,.n=n,.leaves=leaves,.z_drop=INT32_MAX};
    for(unsigned p=0;p<5;p++){
        for(unsigned k=0;k<3;k++)j->planes[p][k]=j->planes[p][k+3]=0;
        j->planes[p][6]=0;
    }
}

static void synthetic(unsigned n, unsigned far_n){
    StageChunk* main=calloc(n,sizeof *main);
    StageChunk* far=calloc(far_n,sizeof *far);
    uint16_t *a=calloc(n+1,2),*b=calloc(n+1,2),*c=calloc(n+1,2);
    if(!main||!far||!a||!b||!c)abort();
    for(unsigned i=0;i<n;i++){main[i].skip=i+1;main[i].leaf=i;main[i].cone_plane_limit=CONE_NONE;}
    for(unsigned i=0;i<far_n;i++){far[i].skip=i+1;far[i].leaf=10000+i;far[i].cone_plane_limit=CONE_NONE;}
    smg_visibility_set_partial_chunk_cache(cache_storage,sizeof cache_storage,main,n,far,far_n);
    if(!smg_partial_chunk_cache_stats.active||smg_partial_chunk_cache_stats.main_entries!=512||
       smg_partial_chunk_cache_stats.far_entries!=128||smg_partial_chunk_cache_stats.entries!=640||
       smg_partial_chunk_cache_stats.main_ways!=2||
       smg_partial_chunk_cache_stats.bytes!=640*36+256)abort();
    CullJob j; setup_job(&j,main,n,a); geometry_cull_chunks(&j);
    for(unsigned i=0;i<n;i++)if(a[i]!=i)abort();
    uint32_t hit0=smg_partial_chunk_cache_stats.main_hits,miss0=smg_partial_chunk_cache_stats.main_misses;
    setup_job(&j,main,n,b); geometry_cull_chunks(&j);
    if(memcmp(a,b,n*2))abort();
    uint32_t hits1=smg_partial_chunk_cache_stats.main_hits-hit0;
    uint32_t misses1=smg_partial_chunk_cache_stats.main_misses-miss0;
    if(n==1100&&hits1+misses1!=n)abort();
    if(n==400&&(hits1!=n||misses1))abort();
    setup_job(&j,main,n,c); geometry_cull_chunks(&j);
    if(memcmp(a,c,n*2))abort();
    uint32_t hits2=smg_partial_chunk_cache_stats.main_hits-hit0-hits1;
    uint32_t misses2=smg_partial_chunk_cache_stats.main_misses-miss0-misses1;
    if(n==1100&&hits2+misses2!=n)abort();
    if(n==400&&(hits2!=n||misses2))abort();

    uint16_t* f=calloc(far_n+1,2); if(!f)abort();
    uint32_t far_misses=smg_partial_chunk_cache_stats.far_misses;
    setup_job(&j,far,far_n,f); geometry_cull_chunks(&j);
    if(smg_partial_chunk_cache_stats.far_misses-far_misses!=far_n)abort();
    for(unsigned i=0;i<far_n;i++)if(f[i]!=10000+i)abort();
    uint32_t far_hits=smg_partial_chunk_cache_stats.far_hits;
    setup_job(&j,far,far_n,f); geometry_cull_chunks(&j);
    if(smg_partial_chunk_cache_stats.far_hits-far_hits!=far_n)abort();
    uint32_t main_hits=smg_partial_chunk_cache_stats.main_hits;
    setup_job(&j,main,n,b); geometry_cull_chunks(&j);
    if(memcmp(a,b,n*2))abort(); /* Far indices must never masquerade as main hits. */
    if(n==400&&smg_partial_chunk_cache_stats.main_hits-main_hits!=n)abort();
    checks++;
    free(f);free(main);free(far);free(a);free(b);free(c);
}

static void protected_hit(void){
    const unsigned n=400; const size_t page=(size_t)sysconf(_SC_PAGESIZE);
    size_t bytes=n*sizeof(StageChunk), mapped=(bytes+page-1)&~(page-1);
    StageChunk* chunks=mmap(NULL,mapped,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);
    if(chunks==MAP_FAILED)abort();
    uint16_t* a=calloc(n+1,2),*b=calloc(n+1,2);if(!a||!b)abort();
    for(unsigned i=0;i<n;i++){chunks[i].skip=i+1;chunks[i].leaf=i;chunks[i].cone_plane_limit=CONE_NONE;}
    smg_visibility_set_partial_chunk_cache(cache_storage,sizeof cache_storage,chunks,n,NULL,0);
    CullJob j;setup_job(&j,chunks,n,a);geometry_cull_chunks(&j);
    if(mprotect(chunks,mapped,PROT_NONE))abort();
    uint32_t hits=smg_partial_chunk_cache_stats.main_hits;
    setup_job(&j,chunks,n,b);geometry_cull_chunks(&j);
    if(memcmp(a,b,n*2)||smg_partial_chunk_cache_stats.main_hits-hits!=n)abort();
    if(mprotect(chunks,mapped,PROT_READ|PROT_WRITE))abort();
    munmap(chunks,mapped);free(a);free(b);checks++;
}

static void two_way_conflict(void){
    const unsigned n=513; const size_t page=(size_t)sysconf(_SC_PAGESIZE);
    size_t bytes=n*sizeof(StageChunk), mapped=(bytes+page-1)&~(page-1);
    StageChunk* chunks=mmap(NULL,mapped,PROT_READ|PROT_WRITE,MAP_PRIVATE|MAP_ANONYMOUS,-1,0);
    if(chunks==MAP_FAILED)abort();
    for(unsigned i=0;i<n;i++){chunks[i].skip=1;chunks[i].leaf=i;chunks[i].cone_plane_limit=CONE_NONE;}
    smg_visibility_set_partial_chunk_cache(cache_storage,sizeof cache_storage,chunks,n,NULL,0);
    if(!smg_partial_chunk_cache_stats.active||smg_partial_chunk_cache_stats.main_ways!=2)abort();
    CullJob j;setup_job(&j,chunks,n,NULL);
    const unsigned sequence[]={0,256,0,512,0};
    for(unsigned i=0;i<sizeof sequence/sizeof sequence[0];i++){
        const StageChunk* chunk=partial_chunk_for_job(&j,sequence[i]);
        if(chunk->leaf!=sequence[i])abort();
    }
    if(smg_partial_chunk_cache_stats.main_hits!=2||smg_partial_chunk_cache_stats.main_misses!=3)abort();
    if(mprotect(chunks,mapped,PROT_NONE))abort();
    uint32_t hits=smg_partial_chunk_cache_stats.main_hits;
    if(partial_chunk_for_job(&j,0)->leaf!=0)abort();
    if(smg_partial_chunk_cache_stats.main_hits-hits!=1)abort();
    if(mprotect(chunks,mapped,PROT_READ|PROT_WRITE))abort();
    munmap(chunks,mapped);checks++;
}

static void real_pair(const char* main_path,const char* far_path){
    FILE*f=fopen(main_path,"rb");if(!f)abort();StageHeader mh;if(fread(&mh,sizeof mh,1,f)!=1)abort();
    if(fseek(f,(long)mh.chunks,SEEK_SET))abort();
    StageChunk* main=malloc((size_t)mh.chunk_count*sizeof *main);
    if(!main||fread(main,sizeof *main,mh.chunk_count,f)!=mh.chunk_count)abort();fclose(f);
    StageHeader fh={0};StageChunk* far=NULL;uint32_t fn=0;
    if(strcmp(far_path,"-")){
        f=fopen(far_path,"rb");if(!f)abort();if(fread(&fh,sizeof fh,1,f)!=1)abort();
        if(fseek(f,(long)fh.chunks,SEEK_SET))abort();fn=fh.chunk_count;
        far=malloc((size_t)fn*sizeof *far);if(!far||fread(far,sizeof *far,fn,f)!=fn)abort();fclose(f);
    }
    uint16_t* expected=calloc((size_t)mh.chunk_count+1,2),*actual=calloc((size_t)mh.chunk_count+1,2);
    uint16_t* expected_far=calloc((size_t)fn+1,2),*actual_far=calloc((size_t)fn+1,2);
    if(!expected||!actual||!expected_far||!actual_far)abort();
    smg_visibility_set_partial_chunk_cache(NULL,0,NULL,0,NULL,0);
    CullJob j;setup_job(&j,main,mh.chunk_count,expected);geometry_cull_chunks(&j);int main_leaves=j.leaf_count;
    int far_leaves=0;
    if(far){setup_job(&j,far,fn,expected_far);geometry_cull_chunks(&j);far_leaves=j.leaf_count;}
    smg_visibility_set_partial_chunk_cache(cache_storage,sizeof cache_storage,main,mh.chunk_count,far,fn);
    for(unsigned round=0;round<3;round++){
        setup_job(&j,main,mh.chunk_count,actual);geometry_cull_chunks(&j);
        if(j.leaf_count!=main_leaves||memcmp(expected,actual,(size_t)main_leaves*2))abort();
        if(far){setup_job(&j,far,fn,actual_far);geometry_cull_chunks(&j);
            if(j.leaf_count!=far_leaves||memcmp(expected_far,actual_far,(size_t)far_leaves*2))abort();}
    }
    checks++;
    free(main);free(far);free(expected);free(actual);free(expected_far);free(actual_far);
}

int main(int argc,char**argv){
    static StageChunk fallback_main[512];
    if(sizeof(StageHeader)!=204||sizeof(StageChunk)!=32)abort();
    if((argc-1)%2)abort();
    synthetic(1100,127);synthetic(700,127);synthetic(400,127);protected_hit();two_way_conflict();
    for(int i=1;i<argc;i+=2){fprintf(stderr,"testing %s\n",argv[i]);fflush(stderr);real_pair(argv[i],argv[i+1]);}
    smg_visibility_set_partial_chunk_cache(cache_storage,35,NULL,0,NULL,0);
    if(smg_partial_chunk_cache_stats.active)abort();
    smg_visibility_set_partial_chunk_cache((void*)(cache_storage+1),sizeof cache_storage,NULL,0,NULL,0);
    if(smg_partial_chunk_cache_stats.active)abort();
    smg_visibility_set_partial_chunk_cache(cache_storage,512*36,fallback_main,512,NULL,0);
    if(smg_partial_chunk_cache_stats.active&&smg_partial_chunk_cache_stats.main_ways!=1)abort();
    printf("%u partial-cache ownership, collision, main/far namespace, protected-hit and real-stage differential checks pass\n",checks);
}
'''

with tempfile.TemporaryDirectory(prefix="partial-chunk-cache-", dir=ROOT / "tmp") as name:
    work = Path(name)
    (work / "test.c").write_text(prefix + types + cull + suffix)
    stage_names = ("astrodome", "beachbowl", "buoybase", "deepdark", "freezeflame",
                   "goodegg", "gustygarden", "honeyhive", "honeyhive_tower", "observatory")
    stages = [ROOT / "build/assets" / f"stage_{name}.bin" for name in stage_names]
    args = []
    for stage in stages:
        far = stage.with_name(stage.stem + "_far.bin")
        args += ["/source/" + stage.relative_to(ROOT).as_posix(),
                 "/source/" + far.relative_to(ROOT).as_posix() if far.is_file() else "-"]
    command = "cc -I/source/src -O3 -fno-strict-aliasing -no-pie -fsanitize=address,undefined test.c -o test && ./test " + " ".join(args)
    subprocess.run(["docker", "run", "--rm", "--user", "1000:1000", "-v", f"{ROOT}:/source:ro",
                    "-v", f"{work}:/test", "-w", "/test", "-e", "ASAN_OPTIONS=abort_on_error=1:handle_segv=0",
                    "smg-runtime-tests", "sh", "-c", command], check=True, timeout=120)
