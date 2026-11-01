#!/usr/bin/env python3
"""Run the accepted input route headlessly and preserve floor-loss incidents.

Use a disposable installed profile; the original baseline/timeline are inputs.
The supplied replay timeline may omit only the final quit for inspection.
This records states for divergence checks, not a hardware performance verdict.
"""
import argparse
from contextlib import nullcontext
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time
from types import SimpleNamespace
from gwprov.debug_shell import DebugSession, GwemuGDBBackend
from gwprov.qmp import QMPConnection


class SnapshotPause:
    """Read a coherent sample, preserving the VM's prior running state."""
    def __init__(self, q):
        self.q = q
        self.running = False

    def __enter__(self):
        self.running = self.q.execute('query-status')['return']['running']
        if self.running:
            self.q.execute('stop')

    def __exit__(self, *error):
        if self.running:
            self.q.execute('cont')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('profile',type=Path);parser.add_argument('elf',type=Path)
    parser.add_argument('timeline',type=Path);parser.add_argument('output',type=Path)
    parser.add_argument('--seconds',type=float,default=50)
    parser.add_argument('--minimum-frames',type=int,default=1,help='Fail timed runs that do not complete this many rendered frames')
    parser.add_argument('--timing-mode',choices=['default','baseline','experimental-m7'],default='default')
    parser.add_argument('--pacing',choices=['realtime','unthrottled'],default=None)
    parser.add_argument('--rtc-epoch',type=int,default=None)
    parser.add_argument('--coherent',action='store_true',help='Pause guest execution while reading each complete sample')
    parser.add_argument('--perspective',choices=['on','off'],default=None,help='Set the debug control once gameplay is initialized')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    started=time.monotonic()
    launch=['gwprov','start',f'profile:{args.profile.resolve()}',
                           '--name','smg-collision-route','--boot-bank','2','--headless','--gdb-port','1236',
                           '--timeline',str(args.timeline.resolve()),'--timing-mode',args.timing_mode]
    if args.pacing is not None:launch += ['--pacing',args.pacing]
    if args.rtc_epoch is not None:launch += ['--rtc-epoch',str(args.rtc_epoch)]
    result=subprocess.run(launch,text=True,capture_output=True,check=True)
    print(result.stdout,flush=True);pid=int(re.search(r'\(pid (\d+)\)',result.stdout)[1]);device=f'gwprov://{pid}'
    report={'device':device,'profile':str(args.profile.resolve()),'elf':str(args.elf.resolve()),
            'timeline':str(args.timeline.resolve()),'note':__doc__.strip(),'requested_perspective':args.perspective,
            'launch_command':launch,'timing_mode':args.timing_mode,'coherent_samples':args.coherent,
            'timeline_sha256':hashlib.sha256(args.timeline.read_bytes()).hexdigest(),'samples':[],'incidents':[]}
    (args.output/'session.json').write_text(json.dumps({k:v for k,v in report.items() if k not in ('samples','incidents')},indent=2)+'\n')
    last_sequence=0
    has_cache=False;perspective_set=False
    with QMPConnection(device,timeout=10) as q:
        d=DebugSession(SimpleNamespace(read_memory=q.read_memory),'gwemu',qmp_socket=device);d.symbols.load(args.elf)
        try:
            d.at('smg_vertex_cache_stats');has_cache=True
        except (KeyError,ValueError):pass
        while time.monotonic()-started<args.seconds:
            with SnapshotPause(q) if args.coherent else nullcontext():
                sample={'wall_seconds':time.monotonic()-started}
                for name in ['g_game.tick','g_game.time','g_game.stage_index','g_game.scene',
                         'g_game.player.figure.position','g_game.player.on_ground',
                         'g_game.player.action','g_game.lives','g_game.camera','smg_render_stats']:
                    sample[name]=d.read_path(name)
                if args.coherent:
                    sample['tick_after_read']=d.read_path('g_game.tick')
                    assert sample['tick_after_read']==sample['g_game.tick'],'Guest advanced inside a paused snapshot'
                ready = sample['smg_render_stats']['raster_width'] == 160 and sample['smg_render_stats']['frames'] > 0
                if ready and args.perspective is not None:sample['perspective_enabled']=d.read_path('g_perspective_enabled')
                if ready and has_cache:sample['vertex_cache']=d.read_path('smg_vertex_cache_stats')
                sequence=d.read_path('smg_collision_incident_sequence') if ready else last_sequence
                if sequence!=last_sequence:
                    incidents=d.read_path('smg_collision_incidents')
                    report['incidents'].extend(sorted((x for x in incidents if last_sequence<x['sequence']<=sequence),key=lambda x:x['sequence']))
                    last_sequence=sequence
                    print('floor-loss sequence',sequence,flush=True)
            report['samples'].append(sample)
            if ready and args.perspective is not None and not perspective_set:
                b=GwemuGDBBackend(port=1236)
                running=q.execute('query-status')['return']['running']
                try:
                    b.open(halt=True)
                    b.write_memory(d.at('g_perspective_enabled'),bytes([args.perspective=='on']))
                    perspective_set=True
                finally:
                    if b._socket and running:b.resume()
                    b.close()
            time.sleep(.1)
        if has_cache:
            report['vertex_cache']=d.read_path('smg_vertex_cache_stats')
            report['memory_pools']=d.read_path('smg_memory_stats')
        if args.perspective is not None:
            assert perspective_set,'Perspective control was never applied'
            report['perspective_enabled']=d.read_path('g_perspective_enabled')
        report['geometry']=d.read_path('smg_geometry_stats')
        report['faults']={hex(a):q.u32(a) for a in (0xe000ed28,0xe000ed2c)}
        report['screenshot']=d.screenshot((args.output/'final.png').resolve())
        with SnapshotPause(q) if args.coherent else nullcontext():
            report['final_context']={name:d.read_path(name) for name in [
                'g_game.tick','g_game.time','g_game.stage_index','g_game.scene',
                'g_game.player.figure.position','g_game.player.action','g_game.lives','g_game.camera']}
            address=d.at('smg_framebuffer');size=d.memory_path('smg_framebuffer')['size']
            digest=hashlib.sha256()
            for offset in range(0,size,32768):digest.update(q.read_memory(address+offset,min(32768,size-offset)))
            report['indexed_framebuffer_sha256']=digest.hexdigest()
            if args.timing_mode!='default':
                report['modeled_work']={name:d.read_path(name) for name in [
                    'smg_perf.frames','smg_perf.work_cycles','smg_perf.cycles','smg_clock_hz']}
                report['modeled_work']['DWT_CTRL']=q.u32(0xe0001000)
                report['modeled_work']['hardware_cycle_accurate']=False
    report['advancing_frames']=report['samples'][-1]['smg_render_stats']['frames']-report['samples'][0]['smg_render_stats']['frames']
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'device':device,'frames':report['advancing_frames'],'incidents':len(report['incidents']),'faults':report['faults']},indent=2),flush=True)
    if report['advancing_frames']<args.minimum_frames:raise RuntimeError(f"Only {report['advancing_frames']} rendered frames; expected at least {args.minimum_frames}")


if __name__=='__main__':main()
