#!/usr/bin/env python3
"""Stationary stage fast-path, vertex-cache or projection-divider A/B in an existing headless GWemu.

Changes only the selected volatile debug switch. Camera/scene/player changes
invalidate a window. These guest millisecond timings are not hardware cycles,
nor a substitute for the accepted traversal or its expensive worst frames.
"""
import argparse
import json
from pathlib import Path
import time
from types import SimpleNamespace
from gwprov.debug_shell import DebugSession, GwemuGDBBackend
from gwprov.qmp import QMPConnection


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('device');parser.add_argument('elf',type=Path);parser.add_argument('output',type=Path)
    parser.add_argument('--control',choices=['stage','vertex','projection','raster','perspective','texture','spans'],default='stage')
    parser.add_argument('--port',type=int,default=1236)
    parser.add_argument('--seconds',type=float,default=3)
    parser.add_argument('--rounds',type=int,default=3)
    args=parser.parse_args()
    control={'stage':'g_stage_fast_enabled','vertex':'g_stage_vertex_cache_enabled','projection':'g_projection_divide_enabled','raster':'g_raster_setup_recip_enabled','perspective':'g_perspective_enabled','texture':'g_texture_dtcm_enabled','spans':'g_perspective_reuse_enabled'}[args.control]
    report={'control':args.control,'device':args.device,'elf':str(args.elf.resolve()),'note':__doc__.strip(),'windows':[]}
    with QMPConnection(args.device,timeout=20) as q:
        d=DebugSession(SimpleNamespace(read_memory=q.read_memory),'gwemu',qmp_socket=args.device)
        d.symbols.load(args.elf);d.symbols.rebase_from_runtime_pointers(q.read_memory)
        original=bool(d.read_path(control))
        if args.control=='spans':assert d.read_path('g_perspective_enabled'),'Enable perspective correction before benchmarking spans'
        def set_fast(enabled):
            b=GwemuGDBBackend(port=args.port)
            running=q.execute('query-status')['return']['running']
            try:
                b.open(halt=True)
                assert d.read_path('smg_render_stats')['raster_width']==160
                assert not q.u32(0xe000ed28) and not q.u32(0xe000ed2c)
                b.write_memory(d.at(control),bytes([enabled]))
            finally:
                if b._socket and running:b.resume()
                b.close()
        def sample():
            return {**({'vertex_cache':d.read_path('smg_vertex_cache_stats')} if args.control=='vertex' else {}),'time':time.monotonic(),'stats':d.read_path('smg_render_stats'),
                'geometry':d.read_path('smg_geometry_stats'),
                **{key:d.read_path(key) for key in ['g_game.camera','g_game.scene','g_game.stage_index',
                   'g_game.view_distance','g_game.player.figure.position','g_depth_enabled']}}
        try:
            for round_number in range(args.rounds):
                # Reverse order every round to expose drift rather than hide it.
                for enabled in ((False,True) if round_number%2==0 else (True,False)):
                    set_fast(enabled);time.sleep(.25)
                    samples=[sample()];deadline=samples[0]['time']+args.seconds
                    while time.monotonic()<deadline:
                        time.sleep(min(.1,max(0,deadline-time.monotonic())));samples.append(sample())
                    a,b=samples[0],samples[-1];frames=b['stats']['frames']-a['stats']['frames']
                    assert frames>0,'No advancing rendered frames'
                    stable=all(all(s[k]==a[k] for k in a if k.startswith('g_')) for s in samples)
                    elapsed=b['time']-a['time']
                    delta={k:b['geometry'][k]-a['geometry'][k] for k in a['geometry'] if k!='version' and not k.endswith('_peak')}
                    window={'round':round_number,'fast':enabled,'stationary':stable,'frames':frames,
                        'wall_seconds':elapsed,'fps':frames/elapsed,
                        'game_ms_per_frame':(b['stats']['game_ms']-a['stats']['game_ms'])/frames,
                        'geometry_delta':delta,'samples':samples}
                    if args.control=='vertex':
                        window['cache_delta']={k:(b['vertex_cache'][k]-a['vertex_cache'][k])&0xffffffff
                            for k in a['vertex_cache'] if k!='version'}
                    report['windows'].append(window)
                    args.output.write_text(json.dumps(report,indent=2)+'\n')
                    print({k:v for k,v in window.items() if k not in ['samples','geometry_delta']},flush=True)
        finally:set_fast(original)
        report['faults']={hex(a):q.u32(a) for a in [0xe000ed28,0xe000ed2c]}
        report['screenshot']=d.screenshot(args.output.with_suffix('.png').resolve())
        args.output.write_text(json.dumps(report,indent=2)+'\n')


if __name__=='__main__':main()
