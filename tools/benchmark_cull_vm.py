#!/usr/bin/env python3
"""Probe actual ARM cull paths with scoped DWT or debugger-step measurements.

Runs only an isolated kernel cohort, re-entering the captured callee with the
same argument/ABI registers. Never continues gameplay after repeated calls.
Baseline metadata alone does not prove that DWT isolates instructions. Require
a one-instruction calibration and stable repeated counts. No M7 latency/cache/
FPS inference. Changes affect only this VM; the shared counter is not reset.
--single-step instead counts debugger transitions inside the culler, rejecting
unexpected excursions and excluding explicitly identified external calls.
"""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import struct
import subprocess
import sys
import time
from gwprov.debug_shell import DebugSession,GwemuGDBBackend
from gwprov.gwemu_manager import start_instance
from gwprov.daemon import managed_instances
from replay_game_frames import run_to,symbol_address


def main():
 p=argparse.ArgumentParser(description=__doc__)
 for name in ['profile','elf','output']:p.add_argument(name,type=Path)
 p.add_argument('--mask-interrupts',action='store_true',help='Mask interrupts only during this disposable kernel cohort')
 p.add_argument('--single-step',action='store_true',help='Count debugger step transitions in the culler, excluding common external callees; do not use DWT')
 p.add_argument('--legacy-image',action='store_true',help='Single-step an original ELF without the narrow-path debug controls')
 a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
 if a.legacy_image and not a.single_step:p.error('--legacy-image requires --single-step')
 if a.single_step and a.mask_interrupts:p.error('--single-step and --mask-interrupts are separate measurement modes')
 r={'note':__doc__.strip(),'profile':str(a.profile.resolve()),'elf':str(a.elf.resolve()),'samples':[]}
 pid=None;b=None
 try:
  start_instance(str(a.profile.resolve()),headless=True,gdb_port=1236,start_halted=True,rtc_epoch=1700000000,timing_mode='baseline',pacing='unthrottled')
  row=next(x for x in managed_instances() if x['profile']==str(a.profile.resolve()));pid=row['pid'];r['cohort']=row
  assert row['timing']['cycleSemantics']=='one-instruction-one-cycle' and not row['timing']['hardwareCycleAccurate']
  (a.output/'session.json').write_text(json.dumps(r,indent=2)+'\n')
  b=GwemuGDBBackend(port=1236)
  for i in range(100):
   try:b.open(halt=True);break
   except Exception:
    b.close()
    if i==99:raise
    time.sleep(.05)
  d=DebugSession(b,'gwemu',qmp_socket=f'gwprov://{pid}');d.symbols.load(a.elf)
  b.write_memory(0x58004500,(0x544f4f42).to_bytes(4,'little'))
  hit=run_to(d,'smg_xip_load');run_to(d,hit['registers']['lr']);d.symbols.rebase_from_runtime_pointers(b.read_memory)
  run_to(d,'smg_game_frame');hit=run_to(d,'geometry_cull_chunks');entry=hit['registers']
  raw=b.read_memory(entry['r0'],176);chunks,n=struct.unpack_from('<2I',raw);leaves=struct.unpack_from('<I',raw,164)[0]
  chunk_data=b.read_memory(chunks,n*32)
  r['entry_registers']=entry
  r['job']={'n':n,'planes':[list(struct.unpack_from('<7i',raw,8+i*28)) for i in range(5)],'role':'main' if chunks==d.read_path('g_game.stage.chunks') else 'other',
   'cam':list(struct.unpack_from('<3i',raw,152)),'z_drop':struct.unpack_from('<i',raw,148)[0],
   'z_inner':struct.unpack_from('<i',raw,172)[0],'chunks_sha256':hashlib.sha256(chunk_data).hexdigest()}
  (a.output/'job.bin').write_bytes(raw);(a.output/'chunks.bin').write_bytes(b.read_memory(chunks,n*32))
  if a.single_step:
   base,size=symbol_address(a.elf,'geometry_cull_chunks');base&=~1
   expected=None;r['measurement_kind']='GDB step transitions inside culler; external callees excluded, not cycles/FPS'
   for enabled in ([False] if a.legacy_image else [False,True]):
    if not a.legacy_image:b.write_memory(d.at('g_cull_narrow_enabled'),bytes([enabled]))
    for name in [f'r{i}' for i in range(13)]+['sp','lr','pc']:b.write_register(name,entry[name])
    pc=entry['pc'];steps=0;excluded=[]
    while pc!=(entry['lr']&~1):
     assert base<=pc<base+size,('Unexpected execution outside culler',hex(pc))
     previous=pc;b.single_step();pc=b.read_register('pc');steps+=1
     assert pc!=previous,'Step did not advance; refuse an ambiguous instruction count'
     if pc!=(entry['lr']&~1) and not base<=pc<base+size:
      lr=b.read_register('lr');h1,h2=struct.unpack('<2H',b.read_memory(previous,4))
      assert h1&0xf800==0xf000 and h2&0xd000==0xd000 and (lr&~1)==previous+4,('Unexpected non-call excursion',hex(pc))
      excluded.append({'callsite':previous,'callee':pc});run_to(d,lr);pc=b.read_register('pc')
     if steps%1000==0:print('step_count',enabled,steps,flush=True)
     if steps>200000:raise RuntimeError('Culler exceeded debug-step limit')
    count=b.read_register('r0');assert count<=n
    key=(count,hashlib.sha256(b.read_memory(leaves,count*2)).hexdigest())
    if expected is None:expected=key
    assert key==expected,'Ordered leaves changed'
    r['samples'].append({'narrow':enabled,'debug_step_count':steps,'excluded_calls':excluded,'leaf_count':count,'ordered_leaf_sha256':key[1]})
   r['measurement_valid']=True
   if not a.legacy_image:r['cull_math']=d.read_path('smg_cull_math_stats')
   r['faults']={hex(x):int.from_bytes(b.read_memory(x,4),'little') for x in [0xe000ed28,0xe000ed2c]};assert not any(r['faults'].values())
   (a.output/'report.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r,indent=2),flush=True);return
  def u32(addr):return int.from_bytes(b.read_memory(addr,4),'little')
  for address,mask in [(0xe000edfc,1<<24),(0xe0001000,1)]:
   old=u32(address);r.setdefault('counter_controls',[]).append({'address':hex(address),'before':old,'after':old|mask});b.write_memory(address,(old|mask).to_bytes(4,'little'))
  baseline=u32(0xe0001004);assert u32(0xe0001004)==baseline,'Counter changes while the baseline guest is halted'
  if a.mask_interrupts:
   b.target_profile='stm32h7b0'
   r['primask_before']=b.read_register('primask');b.write_register('primask',1)
   r['primask_after']=b.read_register('primask');assert r['primask_after']==1,'VM did not retain the requested interrupt mask'
  before=u32(0xe0001004);d.step();after=u32(0xe0001004)
  r['single_instruction_delta']=(after-before)&0xffffffff
  assert r['single_instruction_delta']==1,'DWT does not isolate a single baseline instruction'
  expected=None
  for index,enabled in enumerate([False,True,True,False]*5):
   b.write_memory(d.at('g_cull_narrow_enabled'),bytes([enabled]))
   for name in [f'r{i}' for i in range(13)]+['sp','lr','pc']:b.write_register(name,entry[name])
   before=u32(0xe0001004);result=run_to(d,entry['lr']);after=u32(0xe0001004)
   count=result['registers']['r0'];assert count<=n
   data=b.read_memory(leaves,count*2);key=(count,hashlib.sha256(data).hexdigest())
   if expected is None:expected=key
   assert key==expected,'Ordered leaf output changed'
   delta=(after-before)&0xffffffff;assert delta>0,'No baseline instruction-proxy count'
   r['samples'].append({'index':index,'narrow':enabled,'raw_dwt_delta':delta,'leaf_count':count,'ordered_leaf_sha256':key[1]})
  r['cull_math']=d.read_path('smg_cull_math_stats')
  r['dwt_medians']={name:statistics.median(x['raw_dwt_delta'] for x in r['samples'] if x['narrow']==enabled) for name,enabled in [('wide',False),('narrow',True)]}
  r['measurement_valid']=all(
   max(values)-min(values)<=max(128,min(values)*.01)
   for values in [[x['raw_dwt_delta'] for x in r['samples'] if x['narrow']==enabled] for enabled in [False,True]])
  assert r['measurement_valid'],'DWT kernel samples are unstable; reject as performance evidence'
  r['faults']={hex(x):u32(x) for x in [0xe000ed28,0xe000ed2c]};assert not any(r['faults'].values())
  (a.output/'report.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r,indent=2),flush=True)
 except Exception as error:
  r['measurement_valid']=False;r['error']=str(error) or type(error).__name__
  (a.output/'report.json').write_text(json.dumps(r,indent=2)+'\n');raise
 finally:
  if b is not None:b.close()
  if pid is not None:
   result=subprocess.run(['gwprov','device',f'gwemu:{pid}','stop'],capture_output=True,text=True)
   (a.output/'cleanup.json').write_text(json.dumps({'pid':pid,'stop_exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr},indent=2)+'\n')
   if result.returncode:
    print('Owned VM stop failed:',result.stderr,file=sys.stderr)
    if sys.exc_info()[0] is None:raise RuntimeError('Owned VM stop failed')


if __name__=='__main__':main()
