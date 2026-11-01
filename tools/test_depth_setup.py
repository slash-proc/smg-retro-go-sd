#!/usr/bin/env python3
"""Check guarded depth division and complete pixels against the wide shader.

Reuses the texture test's four-triangle scenes and independent mip contracts.
The reference substitutes only the original wide depth-setup calls.
"""
import ast
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def literal(path, name):
    tree = ast.parse(path.read_text())
    return next(ast.literal_eval(n.value) for n in tree.body if isinstance(n, ast.Assign)
                and any(isinstance(t, ast.Name) and t.id == name for t in n.targets))


prefix = literal(ROOT / 'tools/test_raster.py', 'prefix') + '#include <assert.h>\n#include <limits.h>\n'
source = (ROOT / 'src/graphics/engine.c').read_text()
production = source[source.index('static int64_t raster_edge('):source.index('\nvoid engine_raster(const DrawEntry* entry)')]
begin = production.index('__attribute__((always_inline)) static inline void engine_raster_inner_impl(')
end = production.index('__attribute__((noinline)) static void engine_raster_inner_affine(', begin)
reference = production[begin:end].replace('engine_raster_inner_impl(', 'reference_impl(', 1)
assert reference.count('raster_depth_setup_div(') == 2
reference = reference.replace('raster_depth_setup_div(', 'raster_setup_div(')
math = r'''
static unsigned ratios;
static void ratio(int64_t n,int64_t d,uint64_t* reciprocal){
 int64_t expected=n/d,actual=raster_depth_setup_div(n,d,reciprocal,true);
 assert(expected==actual);
 uint64_t before=*reciprocal;
 assert(raster_depth_setup_div(n,d,reciprocal,false)==expected&&*reciprocal==before);
 if(before)assert(before==(UINT64_C(1)<<63)/(uint64_t)(d<0?-d:d));
 ratios++;
}
static void check_depth_math(void){
 uint64_t reciprocal=0;
 ratio(0,17,&reciprocal);assert(!reciprocal);
 ratio(32767,17,&reciprocal);assert(!reciprocal);
 ratio(UINT64_C(1)<<33,17,&reciprocal);assert(reciprocal);
 uint64_t saved=reciprocal;ratio(-32767,-17,&reciprocal);assert(reciprocal==saved);
 reciprocal=0;ratio(INT32_MIN,-1,&reciprocal);assert(!reciprocal);
 ratio(UINT32_MAX,1,&reciprocal);ratio(-(int64_t)UINT32_MAX,1,&reciprocal);assert(!reciprocal);
 const int64_t edges[]={0,1,-1,INT32_MIN,INT32_MIN-INT64_C(1),INT32_MIN+INT64_C(1),INT32_MAX-INT64_C(1),INT32_MAX,INT32_MAX+INT64_C(1),UINT32_MAX,-(int64_t)UINT32_MAX,UINT32_MAX+INT64_C(1),-(UINT32_MAX+INT64_C(1)),UINT64_C(1)<<32,INT64_MAX,-INT64_MAX};
 for(unsigned i=0;i<sizeof edges/sizeof *edges;i++)for(unsigned j=0;j<sizeof edges/sizeof *edges;j++){
  if(!edges[j])continue;
  reciprocal=0;ratio(edges[i],edges[j],&reciprocal);ratio(-edges[i],-edges[j],&reciprocal);
 }
 for(unsigned i=0;i<200000;i++){
  uint64_t bits=((uint64_t)next()<<32)|next();int64_t n=bits&INT64_MAX;
  int64_t d=1+((((uint64_t)next()<<32)|next())&UINT64_C(0x1ffffffff));
  if(i%3==0){n=(int32_t)bits;d=1+(next()&INT32_MAX);}
  if(i%3==1)n=(int64_t)(bits&UINT64_C(0xffffffff));
  reciprocal=0;ratio(n,d,&reciprocal);ratio(-n,d,&reciprocal);ratio(n,-d,&reciprocal);ratio(-n,-d,&reciprocal);
 }
 printf("%u depth ratios exact vs signed division: signed/unsigned32 boundaries, wide quotients, valid64 magnitudes, zero/lazy/shared inverse and disabled control\n",ratios);
}
'''
suffix = literal(ROOT / 'tools/test_texture_address.py', 'suffix')
suffix = suffix.replace('int main(void){', 'int main(void){\n check_depth_math();', 1)
suffix = suffix.replace('texture addressing differs', 'depth setup differs').replace('Both mip layouts, every row/column', 'Guarded depth setup; both mip layouts, every row/column')
(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='depth-setup-', dir=ROOT / 'tmp') as directory:
    work = Path(directory)
    (work / 'test.c').write_text(prefix + production + reference + math + suffix)
    try:
        subprocess.run(['docker', 'run', '--rm', '--name', work.name, '--user', '1000:1000',
                        '-v', f'{work}:/test', '-w', '/test', 'smg-runtime-tests', 'sh', '-c',
                        'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie test.c -o test && ./test && '
                        'cc -O3 -fno-strict-aliasing -fsanitize=address,undefined,float-cast-overflow -no-pie -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 test.c -o native && ./native'], check=True, timeout=120)
    finally:
        subprocess.run(['docker', 'rm', '-f', work.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
