#!/usr/bin/env python3
"""Count executed ARM instructions in one frozen triangle-collection call.

Uses headless GWemu/GWProv with disposable profiles. Restores all job outputs
and ABI registers before each path; never resumes gameplay after this cohort.
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
 p.add_argument('--legacy-image',action='store_true')
 p.add_argument('--stage-index',type=int,choices=range(10),default=None,help='Separate cold-stage kernel cohort, not the accepted recording')
 a=p.parse_args();a.output.mkdir(parents=True,exist_ok=True)
 r={'note':__doc__.strip(),'samples':[],'elf_sha256':hashlib.sha256(a.elf.read_bytes()).hexdigest()};pid=None;b=None
 try:
  start_instance(str(a.profile.resolve()),headless=True,gdb_port=1236,start_halted=True,rtc_epoch=1700000000,timing_mode='baseline',pacing='unthrottled')
  row=next(x for x in managed_instances() if x['profile']==str(a.profile.resolve()));pid=row['pid'];r['cohort']=row
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
  hit=run_to(d,'geometry_collect_leaves_c');entry=hit['registers'];job=entry['r0']
  raw=b.read_memory(job,88);w=struct.unpack('<22I',raw)
  assert w[17]==d.read_path('g_game.stage.points'),'Expected initial main-stage job'
  counts={n:d.read_path('g_game.stage.header.'+n) for n in ['point_count','triangle_count','leaf_count','leaf_triangle_count','rank_count']}
  r['job']={'indexed':bool(entry['r1']),'leaf_count':w[1],'cam':list(struct.unpack_from('<3i',raw,24)),
   'limit':struct.unpack_from('<i',raw,20)[0],'axis_coord':w[20],'axis_sign':w[21],'home':bool(w[16]),'counts':counts}
  r['input_sha256']={}
  for name,address,size in [('leaves',w[0],w[1]*2),('offsets',w[2],(counts['leaf_count']+1)*2),('ranks',w[4],counts['leaf_triangle_count']*2),('points',w[17],counts['point_count']*6),('triangles',w[18],counts['triangle_count']*24)]:
   data=b.read_memory(address,size);r['input_sha256'][name]=hashlib.sha256(data).hexdigest()
  if not entry['r1']:
   data=b.read_memory(w[3],counts['leaf_triangle_count']*20);r['input_sha256']['records']=hashlib.sha256(data).hexdigest()
  outputs=[('bits',w[11],d.memory_path('g_rank_bits')['size']),('plist',w[14],w[15]-w[14])]
  # Home-remapped indexed stages do not use marks during collection. Their
  # point counts can exceed the direct-mark buffer; never snapshot/write an
  # unused pointer (which may be NULL or too small for the whole stage).
  if not w[16]:
   assert w[9] and counts['point_count']<=d.memory_path('g_point_marks')['size']
   outputs.append(('marks',w[9],counts['point_count']))
  saved=[(name,address,b.read_memory(address,size)) for name,address,size in outputs]
  r['initial_output_sha256']={name:hashlib.sha256(data).hexdigest() for name,address,data in saved}
  base,size=symbol_address(a.elf,'geometry_collect_leaves_c');base&=~1;expected=None
  # Count the complete job-copy helper if this build emits one. Excluding a
  # candidate-only memcpy would bias the comparison against the original.
  helper_ranges=[]
  with a.elf.open('rb') as stream:
   for symbol in ELFFile(stream).get_section_by_name('.symtab').iter_symbols():
    if symbol.name in ['memcpy','__memcpy_veneer'] and symbol['st_size']:
     address=symbol['st_value']&~1;helper_ranges.append((address,address+symbol['st_size']))
  r['included_helper_ranges']=helper_ranges
  for enabled in ([False] if a.legacy_image else [False,True]):
   if not a.legacy_image:b.write_memory(d.at('g_collect_narrow_enabled'),bytes([enabled]))
   b.write_memory(job,raw)
   for name,address,data in saved:b.write_memory(address,data)
   for name in [f'r{i}' for i in range(13)]+['sp','lr','pc']:b.write_register(name,entry[name])
   steps=0;helper_steps=0;pc=entry['pc']
   while pc!=(entry['lr']&~1):
    in_collector=base<=pc<base+size
    assert in_collector or any(lo<=pc<hi for lo,hi in helper_ranges),('Unexpected execution outside collector/helpers',hex(pc))
    if not in_collector:helper_steps+=1
    previous=pc;b.single_step();pc=b.read_register('pc');steps+=1
    assert pc!=previous,'Ambiguous stationary debugger step'
    if steps%5000==0:
     state=qmp_request(f'gwprov://{pid}','query-status')
     assert state['return']['running'] is False and b.read_register('pc')==pc,state
     r['event_drain_queries']=r.get('event_drain_queries',0)+1
     print('collect steps',enabled,steps,flush=True)
    if steps>500000:raise RuntimeError('Collector exceeded debugger-step bound')
   after=struct.unpack('<22I',b.read_memory(job,88));used=after[14]-w[14];assert 0<=used<=w[15]-w[14]
   result={'lo':after[12],'hi':after[13],'plist_bytes':used,'hashes':{name:hashlib.sha256(b.read_memory(address,used if name=='plist' else size)).hexdigest() for name,address,size in outputs}}
   if expected is None:expected=result
   assert result==expected,'Collector outputs changed'
   r['samples'].append({'narrow':enabled,'debug_step_count':steps,'included_helper_steps':helper_steps,'result':result})
  r['faults']={hex(x):int.from_bytes(b.read_memory(x,4),'little') for x in [0xe000ed28,0xe000ed2c]};assert not any(r['faults'].values())
  if not a.legacy_image:r['collect_math']=d.read_path('smg_collect_math_stats')
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
