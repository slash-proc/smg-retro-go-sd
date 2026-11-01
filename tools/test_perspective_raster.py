#!/usr/bin/env python3
"""Check perspective span error and production coverage against independent oracles.

Docker native ASan/UBSan. Long-double per-pixel rational barycentrics are the
oracle, not the prior affine rasterizer. Host times do not predict M7 cost.
"""
import ast
import os
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
MAX_SPAN=int(os.environ.get('SMG_PERSPECTIVE_SPAN_MAX','32'))
if MAX_SPAN not in (8,16,32):
    raise SystemExit('SMG_PERSPECTIVE_SPAN_MAX must be 8, 16 or 32')
tree=ast.parse((ROOT/'tools/test_raster.py').read_text())
prefix=next(ast.literal_eval(n.value) for n in tree.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='prefix' for t in n.targets))
source=(ROOT/'src/graphics/engine.c').read_text()
production=source[source.index('static int64_t raster_edge('):source.index('\nvoid engine_raster(const DrawEntry* entry)')]
suffix=r'''
#include <math.h>
#include <assert.h>
static unsigned long samples,spans,pixels;
static long double worst;
static void check_span(void){
 for(unsigned n=0;n<200000;n++){
  unsigned width=1+next()%FRAME_WIDTH;
  long double q0=1+next()%8388608,q1=1+next()%8388608;
  long double u0=(int16_t)next(),u1=(int16_t)next(),v0=(int16_t)next(),v1=(int16_t)next();
  if(n%7==0){q0=n%2?1:8388608;q1=n%2?8388608:1;}
  // Real row attributes carry the triangle-area factor. Stress cancellation
  // at that scale, including constant UVs under extreme depth variation.
  if(n%5==0){long double scale=(1ULL<<30)+next()%1000000000;q0*=scale;q1*=scale;}
  if(n%17==0){u0=u1=n%2?32767:-32768;v0=v1=n%2?-32768:32767;}
  if(n%13==0){u0=v1=-32768;u1=v0=32767;}
  long double dq=(q1-q0)/width,duq=(q1*u1-q0*u0)/width,dvq=(q1*v1-q0*v0)/width;
  unsigned at=0; PerspectiveEndpoint endpoint={0};
  double curvature=raster_perspective_curvature(q0,q0*u0,q0*v0,dq,duq,dvq);
  while(at<width){
   PerspectiveSpan s=raster_perspective_span(q0+dq*at,q0*u0+duq*at,q0*v0+dvq*at,dq,duq,dvq,width-at,
        g_perspective_reuse_enabled?&endpoint:NULL,g_perspective_precheck_enabled?&curvature:NULL);
   assert(s.count>=1&&s.count<=SMG_PERSPECTIVE_SPAN_MAX&&s.count<=width-at);spans++;
   uint32_t u=s.u,v=s.v;
   for(unsigned j=0;j<s.count;j++){
    long double q=q0+dq*(at+j),eu=(q0*u0+duq*(at+j))/q,ev=(q0*v0+dvq*(at+j))/q;
    // Q16 coordinates wrap modulo65536 model units; compare circular distance.
    long double au=(long double)u/65536,av=(long double)v/65536;
    long double erru=fabsl(remainderl(au-eu,65536)),errv=fabsl(remainderl(av-ev,65536));
    long double error=fmaxl(erru,errv);if(error>worst)worst=error;
    if(error>1.001L){fprintf(stderr,"span error %.9Lf model units n%u at%u/%u\n",error,n,at+j,width);abort();}
    u+=s.du;v+=s.dv;samples++;
   }
   at+=s.count;
  }
 }
}
static long double edge(int ax,int ay,int bx,int by,int px,int py){return (long double)(bx-ax)*(py-ay)-(long double)(by-ay)*(px-ax);}
static bool allowed(unsigned value,long double uv,unsigned shift,unsigned mask,bool alpha){
 long lo=(long)floorl((uv-1.001L)/(1u<<shift)),hi=(long)floorl((uv+1.001L)/(1u<<shift));
 for(long i=lo;i<=hi;i++){unsigned texel=((unsigned)i&mask)+1;if(alpha&&!(texel&3))texel=255;if(value==texel)return true;}
 return false;
}
static void check_pixels(void){
 static uint8_t fb[FRAME_STRIDE*FRAME_HEIGHT],tex[1024],mip[1024];uint32_t depths[3];
 TextureReference t={{(uintptr_t)tex,(uintptr_t)mip}};g_raster_depth=depths;g_perspective_active=true;
 for(unsigned n=0;n<100;n++){
  DrawEntry e={.mode=RM_TEX,.texture=&t};int x[3],y[3],u[3],v[3];
  for(unsigned i=0;i<3;i++){
   x[i]=(int)(next()%(FRAME_WIDTH*20))-FRAME_WIDTH*2;y[i]=(int)(next()%(FRAME_HEIGHT*20))-FRAME_HEIGHT*2;
   u[i]=(int16_t)next();v[i]=(int16_t)next();depths[i]=1+next()%8388608;
   if(n%11==0)depths[i]=12345;
   e.s[i]=(uint16_t)x[i]|((uint32_t)(uint16_t)y[i]<<16);e.uv[i]=(uint16_t)u[i]|((uint32_t)(uint16_t)v[i]<<16);
  }
  if(n%13==0){
   x[0]=8;y[0]=FRAME_HEIGHT*16-8;x[1]=y[1]=y[2]=-32768;x[2]=32767;
   depths[0]=1;depths[1]=depths[2]=8388608;
   for(unsigned i=0;i<3;i++){u[i]=i?-32768:32767;v[i]=i?32767:-32768;e.s[i]=(uint16_t)x[i]|((uint32_t)(uint16_t)y[i]<<16);e.uv[i]=(uint16_t)u[i]|((uint32_t)(uint16_t)v[i]<<16);}
  }
  long double a=edge(x[0],y[0],x[1],y[1],x[2],y[2]);if(!a)continue;
  long double derivs[]={ (long double)(u[1]-u[0])*(y[2]-y[0])-(long double)(u[2]-u[0])*(y[1]-y[0]),
   (long double)(x[1]-x[0])*(u[2]-u[0])-(long double)(x[2]-x[0])*(u[1]-u[0]),
   (long double)(v[1]-v[0])*(y[2]-y[0])-(long double)(v[2]-v[0])*(y[1]-y[0]),
   (long double)(x[1]-x[0])*(v[2]-v[0])-(long double)(x[2]-x[0])*(v[1]-v[0])};
  /* The GBA mip contract uses horizontal U/V gradients only. The separate
   * Docker render-contract audit independently checks its RECIP/ASR rounding. */
  unsigned mip_shift;uint32_t mip_recip=raster_mip_reciprocal((uint32_t)fabsl(a),&mip_shift);
  bool small=raster_gba_mip_gradient((int64_t)derivs[0],mip_recip,mip_shift)||
             raster_gba_mip_gradient((int64_t)derivs[2],mip_recip,mip_shift);
  for(unsigned axis=0;axis<2;axis++)for(unsigned alpha=0;alpha<2;alpha++){
   e.mode=alpha?RM_ALPHA:RM_TEX;g_depth_active=n%2;memset(g_depth_buffer,0,sizeof g_depth_buffer);
   for(unsigned i=0;i<1024;i++){tex[i]=1+(axis?i/32:i%32);mip[i]=1+(axis?i/64:i%64);if(alpha){if(!(tex[i]&3))tex[i]=0;if(!(mip[i]&3))mip[i]=0;}}
   memset(fb,255,sizeof fb);g_engine_fb=fb;engine_raster_inner(&e);
   for(int py=0;py<FRAME_HEIGHT;py++)for(int px=0;px<FRAME_WIDTH;px++){
    long double w[3]={edge(x[1],y[1],x[2],y[2],px*16+8,py*16+8)/a,
     edge(x[2],y[2],x[0],y[0],px*16+8,py*16+8)/a,
     edge(x[0],y[0],x[1],y[1],px*16+8,py*16+8)/a};
    bool inside=w[0]>=0&&w[1]>=0&&w[2]>=0;unsigned value=fb[py*FRAME_STRIDE+px];
    if((!alpha&&(value!=255)!=inside)||(!inside&&value!=255)){fprintf(stderr,"coverage n%u at%d,%d\n",n,px,py);abort();}
    if(g_depth_active&&((g_depth_buffer[py*FRAME_WIDTH+px]!=0)!=(value!=255))){fprintf(stderr,"cutout depth write mismatch\n");abort();}
    if(!inside)continue;
    long double q=0,numerator=0;for(unsigned i=0;i<3;i++){q+=w[i]*depths[i];numerator+=w[i]*depths[i]*(axis?v[i]:u[i]);}
    long double uv=numerator/q;
    if(!allowed(value,uv,axis&&small?5:4,axis&&small?15:31,alpha)){
     fprintf(stderr,"pixel n%u axis%u at%d,%d value%u exactUV%.9Lf mip%d\n",n,axis,px,py,value,uv,small);abort();
    }
    pixels++;
   }
  }
 }
 g_perspective_active=false;
}
int main(void){
 for(unsigned i=0;i<200000;i++){
  double value=(int)(next()%131070)-65535+(double)next()/4294967296.0;
  if(raster_uv_q16(value)!=(uint32_t)(int64_t)(value*65536))abort();
 }
 for(unsigned precheck=0;precheck<2;precheck++)for(unsigned reuse=0;reuse<2;reuse++){
  g_perspective_precheck_enabled=precheck;g_perspective_reuse_enabled=reuse;check_span();check_pixels();}
 printf("%lu rational span samples, %lu spans; max error %.9Lf UV units (1/16 texel limit); %lu production covered samples, exact coverage and bounded sampling at %dx%d (max span %u)\n",samples,spans,worst,pixels,FRAME_WIDTH,FRAME_HEIGHT,SMG_PERSPECTIVE_SPAN_MAX);return 0;}
'''
(ROOT/'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='perspective-test-',dir=ROOT/'tmp') as directory:
 p=Path(directory);(p/'test.c').write_text(prefix+production+suffix)
 try:
  define=f'-DSMG_PERSPECTIVE_SPAN_MAX={MAX_SPAN}'
  subprocess.run(['docker','run','--rm','--name',p.name,'--user','1000:1000','-v',f'{p}:/test','-w','/test','-e','ASAN_OPTIONS=abort_on_error=1:handle_segv=0','smg-runtime-tests','sh','-c',f'cc -O3 {define} -fno-strict-aliasing -no-pie -fsanitize=address,undefined,float-cast-overflow test.c -lm -o test && ./test && cc -O3 {define} -fno-strict-aliasing -no-pie -fsanitize=address,undefined,float-cast-overflow -DFRAME_WIDTH=320 -DFRAME_HEIGHT=240 test.c -lm -o test-native && ./test-native'],check=True,timeout=120)
 finally:subprocess.run(['docker','rm','-f',p.name],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
