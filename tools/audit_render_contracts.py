#!/usr/bin/env python3
"""Check the C port against independent asset/GBA contracts, not its old C copy.

Builds only small native diagnostics in Docker. Exits nonzero on discrepancies.
Checks GBA mip selection/addressing, axis/cone rejection and frustum inheritance.
"""
import ast
from pathlib import Path
import random
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def gba_mip_selects_mip1(numerator, denominator):
    """Independent integer model of raster.s RECIP, ASR and MIP1_STEP."""
    exponent = 0
    mantissa = denominator
    if mantissa >= 256:
        for bound, shift in ((0x01000000, 16), (0x00010000, 8),
                             (0x00001000, 4), (0x00000400, 2), (0x00000200, 1)):
            if mantissa >= bound:
                mantissa >>= shift
                exponent += shift
    else:
        if mantissa < 16:
            mantissa <<= 4
            exponent -= 4
        if mantissa < 64:
            mantissa <<= 2
            exponent -= 2
        if mantissa < 128:
            mantissa <<= 1
            exponent -= 1
        mantissa <<= 1
        exponent -= 1
    reciprocal = 65535 if mantissa == 256 else (16777216 + mantissa // 2) // mantissa
    shift = exponent + 16
    if shift >= 32:
        numerator >>= shift - 31
        shift = 31
    scaled = (numerator * reciprocal) >> shift
    scaled &= 0xFFFFFFFF
    if scaled >= 0x80000000:
        scaled -= 0x100000000
    return scaled != -0x80000000 and abs(scaled) >= 1536


def mip_oracle_cases():
    rng = random.Random(0x4D495031)
    denominators = [1, 15, 16, 63, 64, 127, 128, 255, 256, 257, 511,
                    512, 1023, 1024, 4095, 4096, 65535, 65536,
                    0xFFFFFE, 0xFFFFFF, 0x1000000, 0x7FFFFFFF]
    cases = []
    for den in denominators:
        cases.extend((num, den) for num in (den * 6 + delta for delta in range(-16, 17)))
    for _ in range(4096):
        den = rng.randrange(1, 0x80000000)
        factor = rng.randrange(0, 13)
        cases.append((den * factor + rng.randrange(-16, 17), den))
        cases.append((-(den * factor + rng.randrange(-16, 17)), den))
    return cases


def section(text, begin, end):
    start = text.index(begin)
    return text[start:text.index(end, start)]


def literal(path, name):
    for node in ast.parse(path.read_text()).body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == name for target in node.targets
        ):
            return ast.literal_eval(node.value)
    raise ValueError(name)


def main():
    engine = (ROOT / "src/graphics/engine.c").read_text()
    visibility = (ROOT / "src/graphics/visibility.c").read_text()
    header = (ROOT / "src/graphics/visibility.h").read_text()
    raster = literal(ROOT / "tools/test_raster.py", "prefix")
    raster += section(engine, "static int64_t raster_edge(", "\nvoid engine_raster(const DrawEntry* entry)")
    cases = mip_oracle_cases()
    mip_table = "static const struct { int64_t n; uint32_t d; uint8_t expected; } mip_cases[] = {\n"
    mip_table += ",\n".join(
        f"{{({n}LL), ({d}u), {int(gba_mip_selects_mip1(n, d))}}}"
        for n, d in cases
    ) + "\n};\n"
    raster += mip_table
    raster += r'''
int main(void){
    uint8_t texture[1024], mip[1024], fb[FRAME_STRIDE*FRAME_HEIGHT];
    /* Asset compiler stores V rows, U columns. Padding in mip1 is unused. */
    memset(mip,0,sizeof mip);
    for(int row=0;row<32;row++)for(int column=0;column<32;column++)
        texture[row*32+column]=(row*13+column*7)%251+1;
    for(int row=0;row<16;row++)for(int column=0;column<32;column++)
        mip[row*64+column]=(row*17+column*3)%251+1;
    TextureReference tex={{(uintptr_t)texture,(uintptr_t)mip}};
    int errors=0;
    for(unsigned i=0;i<sizeof mip_cases/sizeof mip_cases[0];i++){
        unsigned shift;uint32_t reciprocal=raster_mip_reciprocal(mip_cases[i].d,&shift);
        bool actual=raster_gba_mip_gradient(mip_cases[i].n,reciprocal,shift);
        if(actual!=mip_cases[i].expected){
            printf("GBA reciprocal mip case %u: n=%lld d=%u expected %u got %u: FAIL\n",
                i,(long long)mip_cases[i].n,mip_cases[i].d,mip_cases[i].expected,actual);
            errors++;
        }
    }
    printf("GBA reciprocal mip thresholds: %u independent cases: %s\n",
        (unsigned)(sizeof mip_cases/sizeof mip_cases[0]),errors?"FAIL":"PASS");
    for(int sample=0;sample<7;sample++){
        DrawEntry e={.mode=RM_TEX,.texture=&tex};
        e.s[0]=8|(8u<<16);e.s[1]=168|(8u<<16);e.s[2]=8|(168u<<16);
        for(int k=0;k<3;k++)e.uv[k]=32|(112u<<16); /* U=2, V=7 texels */
        if(sample==1)e.uv[1]=992|(112u<<16); /* +6 horizontal U lands at 1535 */
        if(sample==2)e.uv[1]=1152|(112u<<16); /* +7 horizontal U selects mip1 */
        if(sample==3)e.uv[2]=1152|(112u<<16); /* vertical U: GBA keeps mip0 */
        if(sample==4)e.uv[1]=32|(1232u<<16); /* +7 horizontal V selects mip1 */
        if(sample==5)e.uv[2]=32|(1232u<<16); /* vertical V: GBA keeps mip0 */
        if(sample==6)e.uv[1]=(uint16_t)-928|(112u<<16); /* -6 rounds to -1536 */
        memset(fb,0,sizeof fb);g_engine_fb=fb;engine_raster_inner(&e);
        bool expected_mip=sample==2||sample==4||sample==6;
        unsigned expected=expected_mip?mip[3*64+2]:texture[7*32+2];
        printf("GBA mip selection case %d: expected mip%d/%u, got %u: %s\n",
               sample,expected_mip,expected,fb[0],expected==fb[0]?"PASS":"FAIL");
        errors+=expected!=fb[0];
    }
    return errors?1:0;
}
'''
    cull = literal(ROOT / "tools/test_visibility.py", "prefix")
    for begin, end in [
        ("typedef struct {\n    const StageChunk*", "} CullJob;"),
        ("typedef struct {\n    const uint16_t* leaves;", "} CollectJob;"),
    ]:
        cull += section(header, begin, end) + end + "\n"
    # Start after the optional cache implementation. Extract only the culling
    # helpers and walker so preprocessor boundaries from unrelated optional
    # code cannot leak into this standalone fixture.
    cull += r'''
static volatile bool g_cull_narrow_enabled = true;
struct {uint32_t version, narrow_jobs, wide_jobs;} smg_cull_math_stats;
struct {
    uint32_t chunks_visited, planes_tested, chunks_frustum_rejected, chunks_cone_rejected;
} audit_cull_stats;
#undef SMG_COUNT
#define SMG_COUNT(field) audit_cull_stats.field++
'''
    cull += section(visibility, "static bool cull_narrow_planes", "\n_Static_assert(offsetof(CullJob")
    cull = cull.rsplit("\n#endif", 1)[0]
    cull += "\n#undef SMG_COUNT\n#define SMG_COUNT(field) ((void)0)\n"
    cull += section(visibility, "static volatile bool g_collect_narrow_enabled", "\nvoid geometry_collect_leaves(")
    cull += r'''
int main(void){
    int errors=0;
    /* Original geometry_collect_{leaves,indexed}: facing<0 triggers the
     * camera-axis test. All three coordinates exceed limit, so reject. */
    for(int indexed=0;indexed<2;indexed++)for(int backface=0;backface<2;backface++){
        uint16_t leaves[]={0}, offsets[]={0,1}, ranks[]={0}, plist[4]={0};
        uint8_t marks[3]={0};uint32_t bits[1]={0};
        int16_t points[3][3]={{100,0,0},{110,0,0},{120,0,0}};
        StageTriangle triangle={.v={{.point=0},{.point=1},{.point=2}},.normal={64,0,0}};
        LeafTriangle record={.points={0,1,2},.a={100,0,0},.normal={128,0,0}};
        CollectJob j={.leaves=leaves,.leaf_count=1,.leaf_triangle_offsets=offsets,
            .recs=&record,.ranks=ranks,.limit=50,.cam={200,0,0},.marks=marks,
            .stamp=1,.bits=bits,.lo=UINT32_MAX,.plist=plist,.plist_end=plist+4,
            .points=points,.triangles=&triangle,.axis_coord=0,.axis_sign=0};
        if(backface)j.cam[0]=0;
        geometry_collect_leaves_c(&j,indexed);
        printf("%s rejection indexed=%d: expected no triangle, got %u: %s\n",
               backface?"backface":"axis",indexed,bits[0],bits[0]?"FAIL":"PASS");
        errors+=bits[0]!=0;
    }
    /* AABB is visible, but original visibility_bvh.s rejects this cone:
     * dot=-100*64; radius=0; cone_d+2*(dot+radius)=-12800 <= -768. */
    StageChunk chunk={.min={-10,-10,-10},.max={10,10,10},.skip=1,.leaf=0,
        .cone_normal={64,0,0},.cone_normal_extent={0,0,0},.cone_center={0,0,0},.cone_plane_limit=0};
    uint16_t leaf=UINT16_MAX;
    CullJob c={.chunks=&chunk,.n=1,.z_drop=10000,.cam={-100,0,0},.leaves=&leaf};
    int n=geometry_cull_chunks(&c);
    printf("backface cone rejection: expected 0 leaves, got %d: %s\n",n,n?"FAIL":"PASS");
    errors+=n!=0;

    /* A parent wholly satisfies four side planes and the far plane. Its
     * descendants must inherit that mask instead of testing all five again. */
    StageChunk tree[4]={0};
    tree[0].min[0]=tree[0].min[1]=tree[0].min[2]=-10;
    tree[0].max[0]=tree[0].max[1]=tree[0].max[2]=10;
    tree[0].skip=4;tree[0].leaf=UINT16_MAX;tree[0].cone_plane_limit=CONE_NONE;
    for(unsigned i=1;i<4;i++){
        tree[i].min[0]=tree[i].min[1]=tree[i].min[2]=-5;
        tree[i].max[0]=tree[i].max[1]=tree[i].max[2]=5;
        tree[i].skip=i+1;tree[i].leaf=(uint16_t)i;tree[i].cone_plane_limit=CONE_NONE;
    }
    CullJob inherited={.chunks=tree,.n=4,.z_drop=10000,.leaves=&leaf};
    for(unsigned p=0;p<5;p++)inherited.planes[p][6]=p<4?-100:0;
    audit_cull_stats=(typeof(audit_cull_stats)){0};
    n=geometry_cull_chunks(&inherited);
    printf("frustum mask propagation: expected 3 leaves/5 plane tests, got %d/%u: %s\n",
           n,audit_cull_stats.planes_tested,
           n==3&&audit_cull_stats.planes_tested==5?"PASS":"FAIL");
    errors+=n!=3||audit_cull_stats.planes_tested!=5;

    /* A plane bit is cleared at each nested level. The fifth clear consumes
     * the last bit and reaches the maximum possible live mask-stack depth. */
    StageChunk deep[11]={0};
    for(unsigned i=0;i<11;i++){
        deep[i].min[0]=deep[i].min[1]=deep[i].min[2]=-300;
        deep[i].max[0]=deep[i].max[1]=deep[i].max[2]=100;
        deep[i].skip=i+1;deep[i].leaf=(uint16_t)i;deep[i].cone_plane_limit=CONE_NONE;
    }
    for(unsigned i=0;i<5;i++)deep[i].leaf=UINT16_MAX;
    deep[0].skip=11;deep[1].skip=10;deep[2].skip=9;deep[3].skip=8;deep[4].skip=7;
    deep[1].max[0]=-100;deep[2].max[0]=-100;deep[2].max[1]=-100;
    deep[3].max[0]=deep[3].max[1]=deep[3].max[2]=-100;
    deep[4].max[0]=-200;deep[4].max[1]=deep[4].max[2]=-100;
    deep[5].leaf=0;deep[6].leaf=1;deep[7].leaf=2;deep[8].leaf=3;deep[9].leaf=4;deep[10].leaf=5;
    deep[5].skip=6;deep[6].skip=7;deep[7].skip=8;deep[8].skip=9;deep[9].skip=10;deep[10].skip=11;
    uint16_t inherited_planes[6]={0};
    CullJob deep_job={.chunks=deep,.n=11,.z_drop=10000,.leaves=inherited_planes};
    for(unsigned axis=0;axis<4;axis++){
        unsigned coordinate=axis<3?axis:0;
        deep_job.planes[axis][coordinate]=256;
        deep_job.planes[axis][coordinate+3]=256;
    }
    deep_job.planes[3][6]=100;
    audit_cull_stats=(typeof(audit_cull_stats)){0};
    int actual=geometry_cull_chunks(&deep_job);
    bool leaves_match=true;
    for(unsigned i=0;i<6;i++)leaves_match&=inherited_planes[i]==i;
    printf("five-level inherited mask stack: expected 6 leaves/25 plane tests, got %d/%u: %s\n",
           actual,audit_cull_stats.planes_tested,
           actual==6&&audit_cull_stats.planes_tested==25&&leaves_match?"PASS":"FAIL");
    errors+=actual!=6||audit_cull_stats.planes_tested!=25||!leaves_match;
    return errors?1:0;
}
'''
    scratch = ROOT / "tmp"
    scratch.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="render-contract-", dir=scratch) as directory:
        path = Path(directory)
        (path / "raster.c").write_text(raster)
        (path / "visibility.c").write_text(cull)
        try:
            result = subprocess.run([
                "docker", "run", "--rm", "--name", path.name, "--user", f"{os.getuid()}:{os.getgid()}",
                "-v", f"{path}:/test", "-v", f"{ROOT / 'src'}:/source:ro", "-w", "/test",
                "smg-runtime-tests", "sh", "-c",
                "cc -O3 -fno-strict-aliasing -I/source raster.c -o raster && "
                "cc -O3 -fno-strict-aliasing -I/source visibility.c -o visibility && "
                "{ ./raster; a=$?; ./visibility; b=$?; test $a -eq 0 && test $b -eq 0; }",
            ], timeout=60)
        finally:
            subprocess.run(["docker", "rm", "-f", path.name],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
