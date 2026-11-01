#!/usr/bin/env python3
"""Refresh and save the homebrew's read-only SDK pool-size queries in GWemu.

Requests a sample through the debugger; the game calls FREE_SIZE only. Never
initializes or allocates from a pool. Values describe this VM's firmware/profile,
not the hardware device or total RAM. Preserve the matching ELF with the report.
"""
import argparse
import json
from pathlib import Path
import struct
import time
from types import SimpleNamespace
from gwprov.debug_shell import DebugSession, GwemuGDBBackend
from gwprov.qmp import QMPConnection


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('device');parser.add_argument('elf',type=Path);parser.add_argument('output',type=Path)
    parser.add_argument('--port',type=int,default=1236)
    args=parser.parse_args()
    with QMPConnection(args.device,timeout=20) as q:
        d=DebugSession(SimpleNamespace(read_memory=q.read_memory),'gwemu',qmp_socket=args.device)
        d.symbols.load(args.elf);d.symbols.rebase_from_runtime_pointers(q.read_memory)
        assert d.read_path('smg_render_stats')['raster_width']==160,'Homebrew is not loaded'
        assert q.execute('query-status')['return']['running'],'Resume the VM before requesting a sample'
        before=d.read_path('smg_memory_stats');request=(before['request']+1)&0xffffffff
        b=GwemuGDBBackend(port=args.port)
        try:
            b.open(halt=True);b.write_memory(d.at('smg_memory_request'),struct.pack('<I',request))
        finally:
            if b._socket:b.resume()
            b.close()
        deadline=time.monotonic()+10
        while True:
            stats=d.read_path('smg_memory_stats')
            if stats['request']==request and stats['seq']%2==0 and stats['seq']==d.read_path('smg_memory_stats.seq'):break
            if time.monotonic()>=deadline:raise RuntimeError('Memory query request was not published; check startup pause')
            time.sleep(.05)
        faults={hex(a):q.u32(a) for a in [0xe000ed28,0xe000ed2c]};assert not any(faults.values())
        end=d.symbols.link_address('__RAM_EMU_END__');bss=d.symbols.link_address('__CORE_BSS_END__')
        report={'device':args.device,'elf':str(args.elf.resolve()),'note':__doc__.strip(),
            'before':before,'after':stats,'linker_headroom_bytes':end-bss,'faults':faults}
        assert stats['ram_emu']<=end-bss,'Allocator reports RAM overlapping the linked core'
        args.output.parent.mkdir(parents=True,exist_ok=True)
        args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))


if __name__=='__main__':main()
