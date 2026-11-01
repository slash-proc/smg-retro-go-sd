#!/usr/bin/env python3
"""Independent alpha coverage/UV contracts against the actual asset compiler."""
from pathlib import Path
import subprocess
import tempfile
import shlex

ROOT = Path(__file__).resolve().parents[1]
HARNESS = r'''
#include "tools/asset_compiler/model_builder.c"
#include <assert.h>

static uint32_t seed=0xbadc0ffe;
static uint32_t rnd(void){seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;return seed;}
static void check_uvs(void){
 for(unsigned test=0;test<100000;test++){
  float uv[3][2];int32_t raw[3][2];ModelVertex out[3]={0};
  for(int axis=0;axis<2;axis++){
   int origin=(int)(rnd()%2000000)-1000000;
   for(int i=0;i<3;i++){
    raw[i][axis]=origin+(int)(rnd()%60000);
    uv[i][axis]=(float)raw[i][axis]/512;
   }
  }
  model_triangle_uvs(out,uv);
  for(int axis=0;axis<2;axis++){
   int shifted[3];
   for(int i=0;i<3;i++){
    shifted[i]=axis?out[i].v:out[i].u;
    assert(shifted[i]>=INT16_MIN+512 && shifted[i]<=INT16_MAX-512);
    assert((raw[i][axis]-shifted[i])%512==0);
    assert(raw[i][axis]-shifted[i]==raw[0][axis]-shifted[0]);
   }
   for(int i=0;i<3;i++)assert(shifted[i]-shifted[0]==raw[i][axis]-raw[0][axis]);
  }
 }
 puts("100000 UV faces preserve quantized gradients and repeat phase, with animation headroom");
}
static void check_stage_uvs(int argc,char **argv){
 unsigned long long faces=0,legacy_saturations=0,normalized_faces=0;
 for(int a=1;a<argc;a++){
  Obj obj=obj_load(argv[a]);
  for(size_t o=0;o<obj.objects.len;o++){
   ObjObject *object=&VEC_AT(obj.objects,ObjObject,o);
   for(size_t g=0;g<object->groups.len;g++){
    ObjGroup *group=&VEC_AT(object->groups,ObjGroup,g);
    for(size_t f=0;f<group->faces.len;f++){
     ObjFace *face=&VEC_AT(group->faces,ObjFace,f);
     ObjVertex *fv=&VEC_AT(obj.verts,ObjVertex,face->first);
     for(int t=0;t+2<face->count;t++){
      ObjVertex *tv[3]={&fv[0],&fv[t+1],&fv[t+2]};
      float uv[3][2];int32_t raw[2][3];ModelVertex out[3]={0};int needs_shift=0;
      for(int i=0;i<3;i++){
       static const float no_uv[3]={0,0,0};
       const float *src=tv[i]->uv>=0?OBJ_UV(&obj,tv[i]->uv):no_uv;
       for(int axis=0;axis<2;axis++){
        uv[i][axis]=src[axis];float q=src[axis]*512.0f;
        raw[axis][i]=float_to_i32_saturated(q);
        if(q < INT16_MIN || q > INT16_MAX)legacy_saturations++;
       }
      }
      model_triangle_uvs(out,uv);
      for(int axis=0;axis<2;axis++){
       int16_t narrowed[3];
       for(int i=0;i<3;i++)narrowed[i]=axis?out[i].v:out[i].u;
       for(int i=0;i<3;i++){
        assert(narrowed[i]>=INT16_MIN+512 && narrowed[i]<=INT16_MAX-512);
        assert(((int64_t)raw[axis][i]-narrowed[i])%512==0);
        assert((int64_t)raw[axis][i]-narrowed[i]==(int64_t)raw[axis][0]-narrowed[0]);
        assert((int)narrowed[i]-narrowed[0]==raw[axis][i]-raw[axis][0]);
       }
       if(raw[axis][0]!=narrowed[0] || raw[axis][1]!=narrowed[1] || raw[axis][2]!=narrowed[2])needs_shift=1;
      }
      faces++;normalized_faces+=needs_shift;
     }
    }
   }
  }
 }
 assert(faces && legacy_saturations && normalized_faces);
 printf("%llu constructed stage triangles: %llu legacy signed16 UV saturations; %llu faces shifted by whole periods with exact quantized gradients and phase\n",faces,legacy_saturations,normalized_faces);
}
static void check_alpha(void){
 uint8_t pixels[4*4*4]={0};Image image={4,4,pixels};
 for(unsigned alpha=0;alpha<256;alpha++){
  for(unsigned i=0;i<16;i++){pixels[4*i]=71;pixels[4*i+3]=alpha;}
  texture_apply_alpha_policy(&image,TEXTURE_ALPHA_COVERAGE);
  unsigned count=0;for(unsigned i=0;i<16;i++){
   assert(pixels[4*i]==71);assert(pixels[4*i+3]==0 || pixels[4*i+3]==255);
   count+=pixels[4*i+3]==255;
  }
  assert(abs((int)(count*255)-(int)(alpha*16))<=127);
 }
 Rgba *palette;size_t size;
 palette=(Rgba*)read_file("build/constructed/assets/palette.rgba",&size);assert(size==1024);
 const char *opaque[]={"stages/deepdark/ground00_v_tev.png","stages/deepdark/sandGrand_v_tev.png",
  "figures/mariolauncherj0p0/mariolauncherj0p0_m0.png",
  "figures/mariolauncherj2p0/mariolauncherj2p0_m1.png",
  "figures/mariolauncherj3p0/mariolauncherj3p0_m2.png"};
 for(unsigned k=0;k<5;k++){
  char path[200];snprintf(path,sizeof path,"build/constructed/assets/%s",opaque[k]);
  Image img=png_load(path);Texture tex;int alpha=0;
  for(int i=0;i<img.w*img.h;i++)assert(img.rgba[i*4+3]==0);
  texture_apply_alpha_policy(&img,TEXTURE_ALPHA_OPAQUE);
  texture_build_mip(tex.mip[0],&img,0,1,0,&alpha,palette);
  texture_build_mip1(tex.mip[1],&img,1,0,&alpha,palette);
  assert(!alpha);
  for(unsigned i=0;i<1024;i++)assert(tex.mip[0][i]);
  for(unsigned row=0;row<16;row++)for(unsigned col=0;col<32;col++)assert(tex.mip[1][row*64+col]);
  image_free(&img);
 }
 Image water=png_load("build/constructed/assets/stages/beachbowl/beachwater.png");
 for(int i=0;i<water.w*water.h;i++)assert(water.rgba[i*4+3]==64);
 texture_apply_alpha_policy(&water,TEXTURE_ALPHA_COVERAGE);
 Texture tex;int alpha=0;unsigned solid=0;
 texture_build_mip(tex.mip[0],&water,0,1,0,&alpha,palette);
 texture_build_mip1(tex.mip[1],&water,1,0,&alpha,palette);
 for(unsigned i=0;i<1024;i++)solid+=tex.mip[0][i]!=0;
 assert(alpha && solid==256); /* Actual water's intended quarter coverage. */
 image_free(&water);
 water=png_load("build/constructed/assets/stages/beachbowl/beachwater.png");
 texture_apply_alpha_policy(&water,TEXTURE_ALPHA_BLEND_QUARTER);alpha=0;
 texture_build_mip(tex.mip[0],&water,0,1,0,&alpha,palette);
 texture_build_mip1(tex.mip[1],&water,1,0,&alpha,palette);assert(!alpha);
 for(unsigned i=0;i<1024;i++)assert(tex.mip[0][i]);
 for(unsigned row=0;row<16;row++)for(unsigned col=0;col<32;col++)assert(tex.mip[1][row*64+col]);
 image_free(&water);free(palette);
 puts("All256 alpha levels retain coverage within half a Bayer sample; five ground/launcher textures opaque in both mips; legacy water cutouts cover25%; blended water has valid opaque texels in both mips");
}
int main(int argc,char **argv){check_uvs();check_stage_uvs(argc,argv);check_alpha();return 0;}
'''

(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix="material-repairs-", dir=ROOT / "tmp") as directory:
    path = Path(directory)
    (path / "test.c").write_text(HARNESS)
    relative = path.relative_to(ROOT)
    stage_objs = sorted((ROOT / "build/constructed/assets/stages").glob("**/*.obj"))
    if not stage_objs:
        raise SystemExit("constructed stage OBJ files are missing; build asset inputs first")
    stage_args = shlex.join(str(obj.relative_to(ROOT)) for obj in stage_objs)
    try:
        subprocess.run([
            "docker", "run", "--rm", "--name", path.name, "--user", "1000:1000",
            "-v", f"{ROOT}:/work", "-w", "/work", "smg-runtime-tests", "sh", "-c",
            f"cc -std=c11 -D_DEFAULT_SOURCE -O2 -ffunction-sections -fdata-sections "
            f"-fsanitize=address,undefined -no-pie -I. {relative}/test.c "
            "tools/asset_compiler/common.c tools/asset_compiler/obj_reader.c "
            "tools/asset_compiler/png_reader.c "
            "tools/asset_compiler/palette.c -Wl,--gc-sections -lm "
            f"-o {relative}/test && ASAN_OPTIONS=handle_segv=0:detect_leaks=0 {relative}/test {stage_args}",
        ], check=True, timeout=60)
    finally:
        subprocess.run(["docker", "rm", "-f", path.name],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
