#!/usr/bin/env python3
"""Check the production shutter bound against every linked mesh vertex.

Also guard the shared-joint/translation-only animation assumption on which
the composite bound relies. The saved-view replay separately checks culling.
"""
from pathlib import Path
import struct
import subprocess
import tempfile
from elftools.elf.elffile import ELFFile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / 'src/actors/objects/mario_launcher.c').read_text()
production = source[source.index('static fx shutter_gate_radius('):source.index('\nstatic const FigureOps mario_launcher_figure_ops =')]
header = (ROOT / 'build/constructed/src/actors/mario_launcher_gate_gen.h').read_text()
start = header.index('static const LauncherJointPose shutterdoore_move')
animation = header[start:header.index('\n};', start)+3]
start = header.index('static const uint8_t shutterdoore_joints')
animation += header[start:header.index(';', start)+1]
parts = []
with (ROOT / 'retro-go-sd/build/smg/smg_core.elf').open('rb') as stream:
    elf = ELFFile(stream)
    symbols = elf.get_section_by_name('.symtab')
    for index in range(3):
        symbol = symbols.get_symbol_by_name(f'figure_shutterdoorej0p{index}')[0]
        section = elf.get_section(symbol['st_shndx'])
        data = section.data()[symbol['st_value']-section['sh_addr']:]
        count = struct.unpack_from('<H', data)[0]
        offset = struct.unpack_from('<I', data, 8)[0]
        extent = struct.unpack_from('<I', data, 60)[0]
        points = list(struct.iter_unpack('<3h', data[offset:offset+count*6]))
        parts.append((extent, points))

prefix = r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
typedef int32_t fx;
typedef struct {fx x,y,z;} v3;
typedef struct {uint32_t extent;} FigureHeader;
typedef struct {const FigureHeader* model;v3 position,scale;} Figure;
typedef struct {Figure figure;} Actor;
typedef struct {int unused;} CameraView;
typedef struct {int16_t pos[3],up[3],front[3],scale[3];} LauncherJointPose;
#define FX(x) (x)
static fx fx_max(fx a,fx b){return a>b?a:b;}
static fx fx_abs(fx a){return a<0?-a:a;}
static fx observed_radius;
static v3 observed_centre;
static unsigned sphere_calls;
static bool view_sphere_visible(const CameraView* view,v3 centre,fx radius){
 (void)view;observed_radius=radius;observed_centre=centre;sphere_calls++;return true;
}
'''
data = 'static const FigureHeader parts[]={' + ','.join('{%d}' % p[0] for p in parts) + '};\n'
data += 'static const uint8_t* const shutterdoore_models[]={' + ','.join('(const uint8_t*)&parts[%d]' % i for i in range(3)) + '};\n'
data += 'static const int16_t points[][3]={' + ','.join('{%d,%d,%d}' % xyz for _, points in parts for xyz in points) + '};\n'
suffix = r'''
int main(void){
 for(unsigned frame=0;frame<sizeof shutterdoore_move/sizeof *shutterdoore_move;frame++){
  const LauncherJointPose* p=&shutterdoore_move[frame][0];
  assert(p->up[0]==0&&p->up[1]==16384&&p->up[2]==0);
  assert(p->front[0]==0&&p->front[1]==0&&p->front[2]==16384);
  for(unsigned axis=0;axis<3;axis++)assert(p->scale[axis]==16384);
 }
 for(unsigned part=0;part<3;part++)assert(shutterdoore_joints[part]==0);
 const v3 scales[]={{64,64,64},{83,83,83},{-83,41,93},{1,256,512},{1024,-768,64},{0,0,0}};
 for(unsigned test=0;test<sizeof scales/sizeof *scales;test++){
  Figure f={&parts[0],{21662,-87483,513},scales[test]},copy=f;CameraView view={0};
  fx radius=shutter_gate_radius(&f);Actor actor={f};assert(shutter_gate_draw_extent(&actor)==radius);
  assert(shutter_gate_view_bounds(&f,&view)&&observed_radius==radius);
  assert(observed_centre.x==f.position.x&&observed_centre.y==f.position.y&&observed_centre.z==f.position.z);
  assert(f.model==copy.model&&f.scale.x==copy.scale.x&&f.position.y==copy.position.y);
  for(unsigned i=0;i<sizeof points/sizeof *points;i++){
   long double x=points[i][0]*f.scale.x/256.L,y=points[i][1]*f.scale.y/256.L,z=points[i][2]*f.scale.z/256.L;
   assert(x*x+y*y+z*z<=(long double)radius*radius);
  }
 }
 unsigned before=sphere_calls;Figure other={0};CameraView view={0};assert(!shutter_gate_view_bounds(&other,&view)&&sphere_calls==before);
 puts("All linked gate vertices contained at six signed/anisotropic scales; every animation frame shares joint0, fixed scale/basis; actor/scene bound dispatch and owner preservation pass");
}
'''
(ROOT / 'tmp').mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='shutter-bounds-', dir=ROOT / 'tmp') as directory:
    work = Path(directory)
    (work / 'test.c').write_text(prefix + data + animation + production + suffix)
    try:
        subprocess.run(['docker', 'run', '--rm', '--name', work.name, '--user', '1000:1000',
                        '-v', f'{work}:/test', '-w', '/test', 'smg-runtime-tests', 'sh', '-c',
                        'cc -O2 -fsanitize=address,undefined -no-pie test.c -o test && ./test'], check=True, timeout=60)
    finally:
        subprocess.run(['docker', 'rm', '-f', work.name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
