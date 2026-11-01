#!/usr/bin/env python3
"""Passively save complete support-loss incidents from a live GWemu.

No halt, reset, inputs or target-memory writes. The ring is committed using
volatile sequence fields; partially published records are skipped and retried.
"""
import argparse
import hashlib
import json
from pathlib import Path
import time
import os
from types import SimpleNamespace
from gwprov.debug_shell import DebugSession
from gwprov.qmp import QMPConnection


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('device');parser.add_argument('elf',type=Path);parser.add_argument('output',type=Path)
    parser.add_argument('--seconds',type=float,default=300)
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    report={'device':args.device,'elf':str(args.elf.resolve()),'elf_sha256':hashlib.sha256(args.elf.read_bytes()).hexdigest(),
            'note':__doc__.strip(),'incidents':[],'missed_records':0,'watcher_pid':os.getpid()}
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    last_sequence=0;started=time.monotonic()
    with QMPConnection(args.device,timeout=10) as q:
        d=DebugSession(SimpleNamespace(read_memory=q.read_memory),'gwemu',qmp_socket=args.device);d.symbols.load(args.elf)
        while time.monotonic()-started<args.seconds:
            stats=d.read_path('smg_render_stats')
            if stats['frames'] and stats['raster_width']==160:
                sequence=d.read_path('smg_collision_incident_sequence')
                if sequence!=last_sequence:
                    if sequence<last_sequence:
                        report['reset_detected']=True;break
                    values=d.read_path('smg_collision_incidents')
                    records=sorted((x for x in values if last_sequence<x['sequence']<=sequence),key=lambda x:x['sequence'])
                    if records:
                        report['missed_records']+=max(0,records[0]['sequence']-last_sequence-1)
                        report['incidents'].extend(records);last_sequence=records[-1]['sequence']
                        report['context']={n:d.read_path(n) for n in ['g_game.stage_index','g_game.scene','g_game.lives','g_game.camera','g_game.player.figure.position']}
                        report['last_wall_seconds']=time.monotonic()-started
                        (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
                        print('saved support-loss sequence',last_sequence,flush=True)
            time.sleep(.1)
        report['elapsed_seconds']=time.monotonic()-started
        report['faults']={hex(a):q.u32(a) for a in (0xe000ed28,0xe000ed2c)}
        (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print('watch completed',len(report['incidents']),'incidents',flush=True)


if __name__=='__main__':main()
