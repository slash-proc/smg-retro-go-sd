#!/usr/bin/env python3
"""Compare scalar and packed-DSP ARM instructions in a frozen warm transform call.

Uses headless GWemu/GWProv with disposable profiles. Warms the source cache with two unmeasured scalar calls, then restores cache,
outputs, counters and ABI registers before each path. Never resumes gameplay.
Debugger transitions count instructions, not M7 cycles, memory latency or FPS.
Rejects unexpected execution outside the function, including interrupt handlers.
"""
import argparse
import hashlib
import json
import os
import signal
from pathlib import Path
import struct
import subprocess
import sys
import time
from elftools.elf.elffile import ELFFile
from gwprov.debug_shell import DebugSession,GwemuGDBBackend
from gwprov.gwemu_manager import start_instance
from gwprov.daemon import managed_instances,qmp_request
from replay_game_frames import run_to,symbol_address


def main():
 p=argparse.ArgumentParser(description=__doc__)
 for name in ['profile','elf','output']:p.add_argument(name,type=Path)
 p.add_argument('--stage-index',type=int,choices=range(10),default=None,help='Separate cold-stage kernel cohort, not the accepted recording')
 p.add_argument('--point-count',type=int,action='append',default=[],help='Also measure a truncated prefix of the captured main-stage list')
 a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
 r={'note':__doc__.strip(),'samples':[],'elf_sha256':hashlib.sha256(a.elf.read_bytes()).hexdigest()};pid=None;b=None
 try:
  start_instance(str(a.profile.resolve()),headless=True,gdb_port=1236,start_halted=True,rtc_epoch=1700000000,timing_mode='baseline',pacing='unthrottled')
  row=next(x for x in managed_instances() if x['profile']==str(a.profile.resolve()));pid=row['pid'];r['cohort']=row
  assert row['timing']['mode']=='baseline' and row['timing']['cycleSemantics']=='one-instruction-one-cycle'
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
  api=run_to(d,'smg_game_frame')
  if a.stage_index is not None:
   saved=api['registers'];assert saved['sp']%8==0
   for name,value in [('r0',a.stage_index),('r1',0),('r2',1),('lr',saved['pc']|1),('pc',d.at('game_load_stage')&~1)]:b.write_register(name,value)
   run_to(d,saved['pc'])
   for name in [f'r{i}' for i in range(13)]+['sp','lr','pc']:b.write_register(name,saved[name])
   assert d.read_path('g_game.stage_index')==a.stage_index and d.read_path('g_game.time')==0
   r['startup_stage_override']=a.stage_index
  hit=run_to(d,'engine_transform_points');entry=hit['registers'];job=entry['r0']
  raw=b.read_memory(job,76);src,dst,screen,plist,n=struct.unpack_from('<5I',raw)
  home,delta=struct.unpack_from('<2I',raw,68);matrix=list(struct.unpack_from('<12i',raw,20))
  assert src==d.read_path('g_game.stage.points'),'Expected main-stage source transform'
  assert dst==d.at('g_frame_points') and screen==d.at('g_screen_positions')
  outputs=[('points',dst,d.memory_path('g_frame_points')['size']),('screen',screen,d.memory_path('g_screen_positions')['size'])]
  capacity=outputs[0][2]//16;assert 0<n<=capacity
  point_count=d.read_path('g_game.stage.header.point_count')
  indices=list(struct.unpack('<'+str(n)+'H',b.read_memory(plist,n*2))) if plist else list(range(n))
  assert max(indices)<point_count
  inputs={'source':b.read_memory(src,point_count*6),'list':struct.pack('<'+str(n)+'H',*indices)}
  slots=indices
  if home:
   homes=list(struct.unpack('<'+str(max(indices)+1)+'H',b.read_memory(home,(max(indices)+1)*2)))
   inputs['home']=struct.pack('<'+str(len(homes))+'H',*homes)
   if delta:
    last_leaf=max(homes[p] for p in indices)
    deltas=list(struct.unpack('<'+str(last_leaf+1)+'h',b.read_memory(delta,(last_leaf+1)*2)))
    inputs['delta']=struct.pack('<'+str(len(deltas))+'h',*deltas)
    slots=[p+deltas[homes[p]] for p in indices]
   else:slots=[homes[p] for p in indices]
  assert min(slots)>=0 and max(slots)<capacity
  eligible=all(all(-32768<=v<=32767 for v in matrix[row*4:row*4+3]) and sum(abs(v) for v in matrix[row*4:row*4+3])<=65535 for row in range(3))
  r['job']={'n':n,'point_count':point_count,'home':bool(home),'mapping':'compact' if home and not delta else 'leaf' if home else 'direct','list':bool(plist),'matrix':matrix,'slot_min':min(slots),'slot_max':max(slots),'packed_matrix_eligible':eligible}
  r['input_sha256']={name:hashlib.sha256(data).hexdigest() for name,data in inputs.items()}
  cache=d.read_path('g_stage_vertex_cache');mask=d.read_path('g_stage_vertex_cache_mask');assert cache and mask in [2047,4095]
  owner=d.at('g_stage_vertex_cache_source');stats_address=d.at('smg_vertex_cache_stats')
  original=[(name,address,b.read_memory(address,size)) for name,address,size in outputs]
  def reset_registers():
   for name in [f'r{i}' for i in range(13)]+['sp','lr','pc']:b.write_register(name,entry[name])
  # Warm through the complete callee, including direct-map collisions. No
  # synthetic hits are injected; both measured paths start from these bytes.
  b.write_memory(d.at('g_transform_dsp_enabled'),b'\0')
  for warm in range(2):
   b.write_memory(job,raw);reset_registers();run_to(d,entry['lr'])
  warm_memory=[('cache',cache,b.read_memory(cache,(mask+1)*8)),('owner',owner,b.read_memory(owner,4)),('stats',stats_address,b.read_memory(stats_address,20))]
  assert int.from_bytes(warm_memory[1][2],'little')==src
  r['cache_entries']=mask+1;r['warmup_calls']=2
  r['warm_start_sha256']={name:hashlib.sha256(data).hexdigest() for name,address,data in warm_memory}
  r['initial_output_sha256']={name:hashlib.sha256(data).hexdigest() for name,address,data in original}
  base,size=symbol_address(a.elf,'engine_transform_points');base&=~1
  helper_ranges=[]
  with a.elf.open('rb') as stream:
   for symbol in ELFFile(stream).get_section_by_name('.symtab').iter_symbols():
    if symbol.name in ['memcpy','__memcpy_veneer','transform_pack_matrix'] and symbol['st_size']:
     address=symbol['st_value']&~1;helper_ranges.append((address,address+symbol['st_size']))
  r['included_helper_ranges']=helper_ranges;expected=None;expected_cache_stats=None
  assert all(0<=value<=n for value in a.point_count),'Prefix point count exceeds captured job'
  r['measured_point_counts']=sorted(set([n]+a.point_count))
  for active_n in r['measured_point_counts']:
   expected=None;expected_cache_stats=None
   for enabled in [False,True]:
    b.write_memory(d.at('g_transform_dsp_enabled'),bytes([enabled]));b.write_memory(job,raw[:16]+struct.pack('<I',active_n)+raw[20:])
    for name,address,data in original+warm_memory:b.write_memory(address,data)
    before=d.read_path('smg_vertex_cache_stats');reset_registers();steps=helper_steps=0;pc=entry['pc']
    while pc!=(entry['lr']&~1):
     in_transform=base<=pc<base+size
     assert in_transform or any(lo<=pc<hi for lo,hi in helper_ranges),('Unexpected execution outside transform/helpers',hex(pc))
     if not in_transform:helper_steps+=1
     previous=pc;b.single_step();pc=b.read_register('pc');steps+=1
     assert pc!=previous,'Ambiguous stationary debugger step'
     if steps%5000==0:
      state=qmp_request(f'gwprov://{pid}','query-status');assert state['return']['running'] is False and b.read_register('pc')==pc,state
      r['event_drain_queries']=r.get('event_drain_queries',0)+1;print('transform steps',active_n,enabled,steps,flush=True)
     if steps>1000000:raise RuntimeError('Transform exceeded debugger-step bound')
    result={name:hashlib.sha256(b.read_memory(address,size)).hexdigest() for name,address,size in outputs}
    after=d.read_path('smg_vertex_cache_stats');delta_stats={k:(after[k]-before[k])&0xffffffff for k in ['hits','misses','bypassed','invalidations']}
    if expected is None:expected=result;expected_cache_stats=delta_stats
    assert result==expected and delta_stats==expected_cache_stats,'Transform output or cache work changed'
    assert delta_stats['hits']+delta_stats['misses']==active_n and not delta_stats['bypassed'] and not delta_stats['invalidations']
    r['samples'].append({'point_count':active_n,'dsp_requested':enabled,'packed_matrix_eligible':eligible,'debug_step_count':steps,'included_helper_steps':helper_steps,'result':result,'vertex_cache_delta':delta_stats})
  r['faults']={hex(x):int.from_bytes(b.read_memory(x,4),'little') for x in [0xe000ed28,0xe000ed2c]};assert not any(r['faults'].values())
  r['measurement_valid']=True
  (a.output/'report.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r,indent=2),flush=True)
 except Exception as error:
  r['measurement_valid']=False;r['error']=str(error) or type(error).__name__
  (a.output/'report.json').write_text(json.dumps(r,indent=2)+'\n');raise
 finally:
  if b is not None:b.close()
  if pid is not None:
   result=subprocess.run(['gwprov','device',f'gwemu:{pid}','stop'],capture_output=True,text=True)
   cleanup={'pid':pid,'gwprov_exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr}
   stopped=result.returncode==0
   if not stopped:
    # The benchmark owns this exact process/profile. If daemon/QMP cleanup
    # times out, verify identity before a graceful process-local fallback.
    proc=Path(f'/proc/{pid}')
    if not proc.exists():stopped=True
    else:
     command=(proc/'cmdline').read_bytes().split(b'\0')
     assert command[0].decode()==row['binaryIdentity']['executable']
     assert ('gnw-h7b0-soc.extflash-image='+str(a.profile.resolve()/'extflash.bin')).encode() in command
     os.kill(pid,signal.SIGTERM);cleanup['fallback']='SIGTERM after exact executable/profile verification'
     for attempt in range(100):
      status=proc/'status'
      if not status.exists() or '\nState:\tZ' in status.read_text():stopped=True;break
      time.sleep(.05)
   cleanup['stop_exit_code']=0 if stopped else result.returncode
   (a.output/'cleanup.json').write_text(json.dumps(cleanup,indent=2)+'\n')
   if not stopped and sys.exc_info()[0] is None:raise RuntimeError('Owned VM stop failed')


if __name__=='__main__':main()
