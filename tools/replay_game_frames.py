#!/usr/bin/env python3
"""Replay recorded game buttons at controlled SMG API frame boundaries.

Uses GWProv/GDB, normal GWemu timing, unchanged firmware. This is a separate
logic/renderer cohort: time is quantized to the selected API frame rate, startup
and final firmware pause are handled outside the game, and render calls are
forced. Host duration is NOT an FPS benchmark or physical-input latency test.

The accepted recording's first B down/release dismisses a firmware dialog and
is consumed there. Game input starts at that release, after two zero-input
warmup calls, and ends before the final pause button. The original timeline is
read-only; this quantized game-call cohort is distinct from native playback.
"""
import argparse
from fractions import Fraction
import hashlib
import json
import math
from pathlib import Path
import subprocess
import struct
import sys
import time
from elftools.elf.elffile import ELFFile
from gwprov.common.timeline import parse
from gwprov.debug_shell import DebugSession, GwemuGDBBackend
from gwprov.gwemu_manager import start_instance
from gwprov.daemon import managed_instances

BUTTONS={'a':1,'b':2,'select':4,'start':8,'right':16,'left':32,'up':64,'down':128}
CONTEXT=['g_game.tick','g_game.time','g_game.stage_index','g_game.scene','g_game.lives',
         'g_game.player.figure.position','g_game.player.action','g_game.camera','g_game.actor_count']


def schedule(path,fps):
    events=parse(path)
    first=next(e for e in events if e.kind=='down')
    if first.button!='b':raise ValueError('Expected the accepted recording to start by dismissing firmware pause with B')
    resume=next(e for e in events if e.kind=='release' and e.button=='b' and e.t>first.t)
    if any(e.kind in ('down','release') and e.button!='b' and e.t<resume.t for e in events):
        raise ValueError('Unexpected game input before firmware resume B was released')
    end=next(e.t for e in events if e.kind=='down' and e.button=='pause')
    epoch=Fraction(str(resume.t));duration=Fraction(str(end))-epoch
    changes=[]
    for e in events:
        if e.t>=end:break
        if e.kind not in ('down','release') or e.t<=resume.t:continue
        if e.button not in BUTTONS:raise ValueError(f'Firmware-only button before final pause: {e.button}')
        changes.append((Fraction(str(e.t))-epoch,e.kind,BUTTONS[e.button]))
    held=0;at=0;result=[]
    for frame in range(math.ceil(duration*fps)):
        now=Fraction(frame,fps)
        while at<len(changes) and changes[at][0]<=now:
            _,kind,bit=changes[at];held=held|bit if kind=='down' else held&~bit;at+=1
        result.append(held)
    return [0,0]+result,{'epoch_seconds':str(epoch),'active_duration_seconds':str(duration),'warmup_frames':2,'fps':fps,'events':len(changes),'startup_b_consumed_by_firmware':True}


def frame_dt(frame,fps):
    # Reproduce millisecond -> Q8 accumulation at a fixed nominal cadence.
    ms0=frame*1000//fps;ms1=(frame+1)*1000//fps
    return ms1*256//1000-ms0*256//1000


def symbol_address(path,name):
    with path.open('rb') as f:
        e=ELFFile(f);s=e.get_section_by_name('.symtab').get_symbol_by_name(name)
        if not s:raise ValueError(f'Missing {name} in {path}')
        return s[0]['st_value'],s[0]['st_size']



def run_to(d,location,timeout=30):
    address=(d.at(location) if isinstance(location,str) else location)&~1
    if d.bp(address)!='OK':raise RuntimeError('Cannot install temporary breakpoint')
    try:
        d.resume();result=d.wait_stopped(timeout,poll_interval=.005)
        if not result['stopped'] or result['registers']['pc']!=address:
            raise RuntimeError(f'Unexpected stop: {result}')
        result['hit']=True;return result
    finally:d.clear_bp(address)


def capture_point_claim(d,b,output,frame):
    """Read a fresh main-stage claim before any pool truncation, without changing it."""
    hit=run_to(d,'geometry_claim_points')
    raw=b.read_memory(hit['registers']['r0'],52)
    visible,n,triangles,home,first,_,_,pool,stamp,plist,next_slot,limit,fast_limit=struct.unpack('<13I',raw)
    assert 0<n<=d.memory_path('g_visible_triangles')['size']//2 and next_slot==0
    assert triangles==d.read_path('g_game.stage.triangles')
    assert pool==d.at('g_frame_points')
    point_count=d.read_path('g_game.stage.header.point_count')
    leaf_count=d.read_path('g_game.stage.header.leaf_count')
    triangle_count=d.read_path('g_game.stage.header.triangle_count')
    ids_raw=b.read_memory(visible,n*2)
    tables={'triangles.bin':b.read_memory(triangles,triangle_count*24),
            'home.bin':b.read_memory(home,point_count*2),
            'first.bin':b.read_memory(first,(leaf_count+1)*2)}
    directory=output/'claim-jobs'/f'frame-{frame:04d}';directory.mkdir(parents=True,exist_ok=True)
    shared=directory.parent/f"stage-{d.read_path('g_game.stage_index')}";shared.mkdir(exist_ok=True)
    for name,data in tables.items():
        path=shared/name
        if path.exists():assert path.read_bytes()==data,'Stage claim tables changed during the cohort'
        else:path.write_bytes(data)
    ids=struct.unpack('<'+str(n)+'H',ids_raw)
    homes=struct.unpack('<'+str(point_count)+'H',tables['home.bin'])
    starts=struct.unpack('<'+str(leaf_count+1)+'H',tables['first.bin'])
    points={struct.unpack_from('<H',tables['triangles.bin'],tri*24+vertex*6)[0]
            for tri in ids for vertex in range(3)}
    assert all(p<point_count and homes[p]<leaf_count for p in points)
    leaves={homes[p] for p in points}
    slots=sum(starts[leaf+1]-starts[leaf] for leaf in leaves)
    far=d.read_path('g_far_reserve');water=138 if d.read_path('g_oceanbowl_split') else 0
    def admission(capacity):
        used=set();vertices=set();allocated=0;admitted=0
        for tri in ids:
            face={struct.unpack_from('<H',tables['triangles.bin'],tri*24+k*6)[0] for k in range(3)}
            additions={homes[p] for p in face}-used
            needed=sum(starts[leaf+1]-starts[leaf] for leaf in additions)
            if allocated+needed>capacity:break
            used.update(additions);vertices.update(face);allocated+=needed;admitted+=1
        return {'triangles':admitted,'slots':allocated,'unique_vertices':len(vertices)}
    expected=admission(limit)
    returned=run_to(d,hit['registers']['lr'])
    actual=b.read_memory(hit['registers']['r0'],52)
    actual_plist,actual_slots=struct.unpack_from('<2I',actual,36)
    observed={'triangles':returned['registers']['r0'],'slots':actual_slots,
              'unique_vertices':(actual_plist-plist)//2}
    state={'completed_api_call':frame,'visible_triangles':n,'unique_vertices':len(points),
        'claimed_leaves':len(leaves),'required_leaf_slots':slots,'actual_main_slot_limit':limit,
        'fast_limit':fast_limit,'stamp':stamp,'far_reserve':far,'water_reserve':water,
        'required_total_point_slots':slots+far+water,
        'compact_total_point_slots':len(points)+far+water,
        'view_mode':d.read_path('g_game.view_distance'),
        'point_capacity':d.memory_path('g_frame_points')['size']//16,
        'expected_current_admission':expected,'actual_current_admission':observed,
        'hypothetical_admission':{str(capacity):admission(max(0,capacity-far-water))
                                  for capacity in [4096,5120,6144,8192]},
        'context':{name:d.read_path(name) for name in CONTEXT},
        'tables':{name:hashlib.sha256(data).hexdigest() for name,data in tables.items()},
        'note':'Read-only demand of the admitted triangle list; excludes any earlier visibility-list overflow. No claim limits changed.'}
    (directory/'job.bin').write_bytes(raw);(directory/'visible.bin').write_bytes(ids_raw)
    (directory/'job.json').write_text(json.dumps(state,indent=2)+'\n')
    assert observed==expected,(observed,expected)
    return {key:state[key] for key in ['completed_api_call','visible_triangles','unique_vertices',
        'required_leaf_slots','required_total_point_slots','compact_total_point_slots','point_capacity','view_mode']}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('profile',type=Path);p.add_argument('elf',type=Path);p.add_argument('firmware_elf',type=Path);p.add_argument('timeline',type=Path);p.add_argument('output',type=Path)
    p.add_argument('--frames',type=int,default=None,help='Limit a smoke test; omitted runs the whole recording')
    p.add_argument('--probe-address',type=lambda x:int(x,0),action='append',default=[],help='On the first call, require an execution hit at one of these loaded code addresses')
    p.add_argument('--perspective',choices=['on','off'],default=None,help='Override texture mapping only in this disposable run')
    p.add_argument('--depth-buffer',choices=['on','off'],default=None,help='Override per-pixel depth testing only in this disposable run')
    p.add_argument('--span-precheck',choices=['on','off'],default=None,help='Override the experimental perspective bound only in this disposable run')
    p.add_argument('--perspective-reuse',choices=['on','off'],default=None,help='Override reuse of exact rational span endpoints only in this disposable run')
    p.add_argument('--projection-divide',choices=['on','off'],default=None,help='Use exact M7 unsigned division or the XIP projection table in this disposable run')
    p.add_argument('--raster-setup-recip',choices=['on','off'],default=None,help='Override the exact shared reciprocal for raster setup divisions only in this disposable run')
    p.add_argument('--cull-narrow',choices=['on','off'],default=None,help='Select guarded32 or wide cull arithmetic for this run')
    p.add_argument('--collect-narrow',choices=['on','off'],default=None,help='Select guarded signed32 or wide triangle facing arithmetic')
    p.add_argument('--visibility-cache',choices=['on','off'],default=None,help='Select SRAM or canonical chunk descriptors for this disposable run')
    p.add_argument('--partial-chunk-cache',choices=['on','off'],default=None,help='Select bounded partial caching for oversized stage chunk tables in this disposable run')
    p.add_argument('--transform-dsp',choices=['on','off'],default=None,help='Override packed transform control for this disposable cohort')
    p.add_argument('--vertex-cache-bytes',type=int,choices=[16384,32768],default=None,help='Use the game setter to limit an already allocated source cache in this disposable cohort')
    p.add_argument('--home-lifecycle',action='store_true',help='Exercise owned AHB shutdown/wake callbacks at the first API boundary; a separate diagnostic cohort')
    p.add_argument('--home-cache',choices=['on','off'],default=None,help='Select cached or canonical point-home tables in this disposable run')
    p.add_argument('--compact-points',choices=['on','off'],default=None,help='Select compact exact-position claims or legacy leaf allocation')
    p.add_argument('--cinematic-detail',choices=['on','off'],default=None,help='Select textured main geometry for intro and launch-star flight shots')
    p.add_argument('--far-textures',choices=['on','off'],default=None,help='Select detailed far texels or average colours in this disposable cohort')
    p.add_argument('--audit-water-table',action='store_true',help='Require the installed palette-blend bytes to match the linked ELF before replay')
    p.add_argument('--audit-zbin-dtcm',action='store_true',help='Require the optional DTCM depth-bin copy to exactly match mapped XIP')
    p.add_argument('--capture-transform-frame',type=int,action='append',default=[],help='Capture the first main transform job at this completed-call number')
    p.add_argument('--vertex-stats',action='store_true',help='Read transform-cache counters at completed-call boundaries without changing runtime controls')
    p.add_argument('--raster-work-stats',action='store_true',help='Read fragment/overdraw work from a SMG_RASTER_WORK_STATS diagnostic build; not timing evidence')
    p.add_argument('--opaque-raster',choices=['on','off'],default=None,help='Select opaque material specialization or the same-build general raster path')
    p.add_argument('--raster-convert-narrow',choices=['on','off'],default=None,help='Select exact guarded signed32 perspective conversions or the wide reference')
    p.add_argument('--instruction-stages',action='store_true',help='CPU-stage cohort: baseline DWT endpoints and bypassed firmware audio-DMA wait; never hardware cycles/FPS')
    p.add_argument('--stage-index',type=int,choices=range(10),default=None,help='Separate smoke cohort: load this stage before API replay; not the original recording trajectory')
    p.add_argument('--view-mode',type=int,choices=range(5),default=None,help='Separate detail cohort: 0 Full, 1 High, 2 Medium, 3 Low, 4 Minimum')
    p.add_argument('--capture-cull-frame',type=int,action='append',default=[],help='Capture the first chunk-cull job inside this completed-call number')
    p.add_argument('--capture-claim-frame',type=int,action='append',default=[],help='Read complete main-stage point demand; requires a SMG_CAPTURE_POINT_CLAIMS diagnostic build')
    p.add_argument('--capture-frame',type=int,action='append',default=[],help='Capture stage geometry after this many completed API calls')
    p.add_argument('--fps',type=int,choices=[30,60],default=30,
        help='Injected game API cadence; independent of configured firmware audio/scanout pacing')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    buttons,conversion=schedule(args.timeline,args.fps)
    if args.frames is not None:buttons=buttons[:args.frames]
    if any(frame<1 or frame>len(buttons) for frame in args.capture_frame+args.capture_cull_frame+args.capture_claim_frame+args.capture_transform_frame):
        p.error('--capture-frame must select an executed call (1 through requested frame count)')
    if args.instruction_stages and (args.capture_cull_frame or args.capture_claim_frame or args.capture_transform_frame or args.probe_address):
        p.error('--instruction-stages cannot stop inside a game call; use only completed-frame captures')
    if args.capture_claim_frame:
        try:symbol_address(args.elf,'geometry_claim_points')
        except ValueError:p.error('--capture-claim-frame requires a SMG_CAPTURE_POINT_CLAIMS diagnostic build')
        try:symbol_address(args.elf,'g_compact_points_enabled')
        except ValueError:pass
        else:
            if args.compact_points!='off':p.error('Legacy claim capture requires --compact-points off')
    report={'protocol':'api-return-v2','note':__doc__.strip(),'profile':str(args.profile.resolve()),'elf':str(args.elf.resolve()),
        'timeline':str(args.timeline.resolve()),'timeline_sha256':hashlib.sha256(args.timeline.read_bytes()).hexdigest(),
        'conversion':conversion,'requested_frames':len(buttons),'samples':[],'caller_skipped_draws':0,
        'control_sha256':hashlib.sha256(b''.join(struct.pack('<II',held,frame_dt(i,args.fps)) for i,held in enumerate(buttons))).hexdigest(),
        'elf_sha256':hashlib.sha256(args.elf.read_bytes()).hexdigest()}
    pid=None;b=None;started=time.monotonic()
    try:
        start_instance(str(args.profile.resolve()),headless=True,gdb_port=1236,start_halted=True,rtc_epoch=1700000000,
                       **({'timing_mode':'baseline','pacing':'unthrottled'} if args.instruction_stages else {}))
        row=next(r for r in managed_instances() if r['profile']==str(args.profile.resolve()))
        pid=row['pid'];report['cohort']=row
        report['device']=f'gwprov://{pid}';(args.output/'session.json').write_text(json.dumps(report,indent=2)+'\n')
        b=GwemuGDBBackend(port=1236)
        for attempt in range(100):
            try:b.open(halt=True);break
            except Exception:
                b.close()
                if attempt==99:raise
                time.sleep(.05)
        d=DebugSession(b,'gwemu',qmp_socket=report['device']);d.symbols.load(args.elf)
        b.write_memory(0x58004500,(0x544f4f42).to_bytes(4,'little'))
        loader=run_to(d,'smg_xip_load');assert loader['hit'],loader
        result=run_to(d,loader['registers']['lr']);assert result['hit'],result
        d.symbols.rebase_from_runtime_pointers(b.read_memory)
        if args.audit_zbin_dtcm:
            try:
                stats=d.read_path('smg_zbin_dtcm_stats')
                pointer=d.read_path('g_lut_zbin_dtcm')
                source=d.at('lut_zbin')
            except (KeyError,ValueError) as error:
                raise RuntimeError('--audit-zbin-dtcm requires an SMG_ZBIN_DTCM build') from error
            with args.elf.open('rb') as stream:
                image=ELFFile(stream);symbol=image.get_section_by_name('.symtab').get_symbol_by_name('lut_zbin')[0]
                section=image.get_section(symbol['st_shndx']);offset=symbol['st_value']-section['sh_addr']
                expected=section.data()[offset:offset+2048]
            assert len(expected)==2048 and b.read_memory(source,2048)==expected, 'Mapped XIP LUT differs from linked table'
            record={'stats':stats,'pointer':pointer,'xip_source':source,'bytes':2048,
                'xip_matches_elf':True,'exact_copy':None,'dtcm_memory':d.read_path('smg_memory_stats')}
            if stats['active']:
                assert stats['bytes']==2048 and pointer
                assert b.read_memory(pointer,2048)==expected, 'DTCM LUT copy differs from XIP source'
                record['exact_copy']=True
            else:
                assert stats['bytes']==0 and not pointer
                record['exact_copy']=False
                record['note']='No DTCM allocation in this GWemu runtime; verified XIP fallback. Hardware DTCM performance remains unmeasured.'
            report['zbin_dtcm']=record
        if args.audit_water_table:
            with args.elf.open('rb') as stream:
                image=ELFFile(stream);symbol=image.get_section_by_name('.symtab').get_symbol_by_name('shade_quarter')[0]
                section=image.get_section(symbol['st_shndx']);offset=symbol['st_value']-section['sh_addr']
                # Generated .incbin labels have no ELF st_size; the table's
                # fixed256x256 schema supplies its extent.
                expected=section.data()[offset:offset+65536]
            assert len(expected)==65536
            assert b.read_memory(d.at('shade_quarter'),len(expected))==expected,'Mapped palette table was modified by installation/relocation'
            report['water_table']={'bytes':len(expected),'sha256':hashlib.sha256(expected).hexdigest(),'runtime_matches_elf':True}
        result=run_to(d,'smg_game_frame');assert result['hit'],result
        assert d.read_path('g_game.tick')==0 and d.read_path('g_game.time')==0,'Did not intercept the first game API call'
        report['api_cadence'] = dict(injected_fps=args.fps,
            configured_firmware_target_fps=d.read_path('smg_target_fps'),
            native_pacing_measured=False,
            note='Injected API delta/input cadence; rendering is forced. Firmware audio/scanout pacing is not measured.')
        if args.stage_index is not None:
            saved=result['registers'];assert saved['sp']%8==0
            for name,value in [('r0',args.stage_index),('r1',0),('r2',1),('lr',saved['pc']|1),('pc',d.at('game_load_stage')&~1)]:b.write_register(name,value)
            run_to(d,saved['pc'])
            for name in [f'r{i}' for i in range(13)]+['sp','lr','pc']:b.write_register(name,saved[name])
            result={'registers':saved};report['startup_stage_override']=args.stage_index
            assert d.read_path('g_game.stage_index')==args.stage_index and d.read_path('g_game.time')==0
        if args.view_mode is not None:
            saved=result['registers']
            for name,value in [('r0',args.view_mode),('lr',saved['pc']|1),('pc',d.at('smg_game_set_view_mode')&~1)]:b.write_register(name,value)
            run_to(d,saved['pc'])
            for name in [f'r{i}' for i in range(13)]+['sp','lr','pc']:b.write_register(name,saved[name])
            report['view_mode_override']=args.view_mode
            assert d.read_path('g_game.view_distance')==args.view_mode
        if args.home_lifecycle:
            saved=result['registers'];assert saved['sp']%8==0
            def invoke(name):
                b.write_register('lr',saved['pc']|1);b.write_register('pc',d.at(name)&~1)
                returned=run_to(d,saved['pc'])['registers']['r0']
                for reg in [f'r{i}' for i in range(13)]+['sp','lr','pc']:b.write_register(reg,saved[reg])
                return returned
            record={'before_allocation':d.read_path('smg_home_allocation'),'before_active':d.read_path('smg_home_cache_stats.active'),'free_before':invoke('core_ahb_get_free_size')}
            assert record['before_allocation']
            invoke('Shutdown');record.update(after_shutdown=d.read_path('smg_home_allocation'),active_after_shutdown=d.read_path('smg_home_cache_stats.active'),free_after_shutdown=invoke('core_ahb_get_free_size'))
            assert not record['after_shutdown'] and not record['active_after_shutdown'] and record['free_after_shutdown']>record['free_before']
            invoke('Shutdown');assert invoke('core_ahb_get_free_size')==record['free_after_shutdown']
            invoke('SleepWake');record.update(after_wake=d.read_path('smg_home_allocation'),active_after_wake=d.read_path('smg_home_cache_stats.active'),free_after_wake=invoke('core_ahb_get_free_size'))
            assert record['after_wake'] and record['free_after_wake']>=16384
            if d.read_path('g_game.stage.header.point_home'):assert record['active_after_wake']
            report['home_lifecycle']=record
        if args.vertex_cache_bytes is not None:
            buffer=d.read_path('g_stage_vertex_cache');allocated=(d.read_path('g_stage_vertex_cache_mask')+1)*8
            assert buffer and args.vertex_cache_bytes<=allocated,'Requested cache exceeds existing allocation'
            saved=result['registers'];assert saved['sp']%8==0
            for name,value in [('r0',buffer),('r1',args.vertex_cache_bytes),('lr',saved['pc']|1),('pc',d.at('smg_game_set_vertex_cache')&~1)]:b.write_register(name,value)
            run_to(d,saved['pc'])
            for name in [f'r{i}' for i in range(13)]+['sp','lr','pc']:b.write_register(name,saved[name])
            report['vertex_cache_override']={'allocated_bytes':allocated,'active_bytes':args.vertex_cache_bytes}
        if args.transform_dsp is not None:
            b.write_memory(d.at('g_transform_dsp_enabled'),bytes([args.transform_dsp=='on']))
            report['transform_dsp_enabled']=d.read_path('g_transform_dsp_enabled')
        report['perspective_before']=d.read_path('g_perspective_enabled')
        if args.perspective is not None:
            b.write_memory(d.at('g_perspective_enabled'),bytes([args.perspective=='on']))
        report['perspective_enabled']=d.read_path('g_perspective_enabled')
        if args.projection_divide is not None:
            report['projection_divide_before']=d.read_path('g_projection_divide_enabled')
            b.write_memory(d.at('g_projection_divide_enabled'),bytes([args.projection_divide=='on']))
            report['projection_divide_enabled']=d.read_path('g_projection_divide_enabled')
        if args.span_precheck is not None:
            report['span_precheck_before']=d.read_path('g_perspective_precheck_enabled')
            b.write_memory(d.at('g_perspective_precheck_enabled'),bytes([args.span_precheck=='on']))
            report['span_precheck_enabled']=d.read_path('g_perspective_precheck_enabled')
        report['perspective_reuse_before']=d.read_path('g_perspective_reuse_enabled')
        if args.perspective_reuse is not None:
            b.write_memory(d.at('g_perspective_reuse_enabled'),bytes([args.perspective_reuse=='on']))
        report['perspective_reuse_enabled']=d.read_path('g_perspective_reuse_enabled')
        if args.raster_setup_recip is not None:
            report['raster_setup_recip_before']=d.read_path('g_raster_setup_recip_enabled')
            b.write_memory(d.at('g_raster_setup_recip_enabled'),bytes([args.raster_setup_recip=='on']))
            report['raster_setup_recip_enabled']=d.read_path('g_raster_setup_recip_enabled')
        if args.cull_narrow is not None:
            report['cull_narrow_before']=d.read_path('g_cull_narrow_enabled')
            b.write_memory(d.at('g_cull_narrow_enabled'),bytes([args.cull_narrow=='on']))
            report['cull_narrow_enabled']=d.read_path('g_cull_narrow_enabled')
        if args.visibility_cache is not None:
            report['visibility_cache_before']=d.read_path('g_visibility_cache_enabled')
            b.write_memory(d.at('g_visibility_cache_enabled'),bytes([args.visibility_cache=='on']))
            report['visibility_cache_enabled']=d.read_path('g_visibility_cache_enabled')
        if args.partial_chunk_cache is not None:
            report['partial_chunk_cache_before']=d.read_path('smg_partial_chunk_cache_enabled')
            b.write_memory(d.at('smg_partial_chunk_cache_enabled'),bytes([args.partial_chunk_cache=='on']))
            report['partial_chunk_cache_enabled']=d.read_path('smg_partial_chunk_cache_enabled')
        if args.collect_narrow is not None:
            b.write_memory(d.at('g_collect_narrow_enabled'),bytes([args.collect_narrow=='on']))
            report['collect_narrow_enabled']=d.read_path('g_collect_narrow_enabled')
        if args.home_cache is not None:
            b.write_memory(d.at('g_home_cache_enabled'),bytes([args.home_cache=='on']))
            report['home_cache_enabled']=d.read_path('g_home_cache_enabled')
        if args.compact_points is not None:
            b.write_memory(d.at('g_compact_points_enabled'),bytes([args.compact_points=='on']))
            report['compact_points_enabled']=d.read_path('g_compact_points_enabled')
        if args.cinematic_detail is not None:
            b.write_memory(d.at('g_cinematic_detail_enabled'),bytes([args.cinematic_detail=='on']))
            report['cinematic_detail_enabled']=d.read_path('g_cinematic_detail_enabled')
        if args.depth_buffer is not None:
            b.write_memory(d.at('g_depth_enabled'),bytes([args.depth_buffer=='on']))
            report['depth_buffer_enabled']=d.read_path('g_depth_enabled')
        if args.opaque_raster is not None:
            b.write_memory(d.at('g_opaque_raster_enabled'),bytes([args.opaque_raster=='on']))
            report['opaque_raster_enabled']=d.read_path('g_opaque_raster_enabled')
        if args.raster_convert_narrow is not None:
            b.write_memory(d.at('g_raster_convert_narrow_enabled'),bytes([args.raster_convert_narrow=='on']))
            report['raster_convert_narrow_enabled']=d.read_path('g_raster_convert_narrow_enabled')
        if args.far_textures is not None:
            b.write_memory(d.at('g_far_texture_detail_enabled'),bytes([args.far_textures=='on']))
            report['far_texture_detail_enabled']=d.read_path('g_far_texture_detail_enabled')
        emu,size=symbol_address(args.firmware_elf,'common_emu_state');assert size==16
        assert int.from_bytes(b.read_memory(emu+10,2),'little',signed=True)==100000//30
        # SDK's byte12 bits3..5: cancel only the startup auto-pause request.
        # This is volatile runtime state in this disposable VM, not firmware.
        value=b.read_memory(emu+12,1)[0];b.write_memory(emu+12,bytes([value&~0x38]))
        report['startup_pause_before']=value;report['startup_pause_after']=value&~0x38
        if args.vertex_stats:
            report['vertex_cache_start']={'stats':d.read_path('smg_vertex_cache_stats'),
                'memory':d.read_path('smg_memory_stats'),
                'buffer':d.read_path('g_stage_vertex_cache'),
                'source':d.read_path('g_stage_vertex_cache_source'),
                'main_points':d.read_path('g_game.stage.points'),
                'main_point_count':d.read_path('g_game.stage.header.point_count'),
                'texture_dtcm':d.read_path('g_texture_cache_dtcm'),
                'texture_active':d.read_path('g_texture_dtcm_active'),
                'dsp_enabled':d.read_path('g_transform_dsp_enabled')}
            try:symbol_address(args.elf,'g_stage_vertex_cache_mask')
            except ValueError:pass # Older fixed-size cache has no mask symbol.
            else:
                mask=d.read_path('g_stage_vertex_cache_mask')
                report['vertex_cache_start']['entries']=mask+1 if report['vertex_cache_start']['buffer'] else 0
            report['vertex_cache_samples']=[]
        address=d.at('smg_framebuffer');expected_size=d.memory_path('smg_framebuffer')['size']
        if args.instruction_stages:
            assert row['timing']['mode']=='baseline' and row['timing']['cycleSemantics']=='one-instruction-one-cycle'
            demcr=int.from_bytes(b.read_memory(0xe000edfc,4),'little')
            ctrl=int.from_bytes(b.read_memory(0xe0001000,4),'little')
            b.write_memory(0xe000edfc,(demcr|(1<<24)).to_bytes(4,'little'))
            b.write_memory(0xe0001000,(ctrl|1).to_bytes(4,'little'))
            assert int.from_bytes(b.read_memory(0xe000edfc,4),'little')&(1<<24)
            assert int.from_bytes(b.read_memory(0xe0001000,4),'little')&1
            stages_address=d.at('smg_perf_frame_cycles');assert d.memory_path('smg_perf_frame_cycles')['size']==36
            b.write_memory(stages_address,bytes(36))
            report['instruction_stages']={'note':'Guest-internal game-stage endpoints only; baseline instruction work including guest interrupts. Excludes palette/scaling/HUD after API return. World includes nested visibility/transform/emission/raster. Debugger-endpoint DWT deltas and enclosing SDK frame counters are invalid for this cohort. No hardware timing/FPS claim.',
                'audio_sync':'After each game API return only, set common_emu_state.skip_frames=1 to bypass the preserved firmware audio-DMA wait that stalls with this baseline-mode binary. Game rendering remains forced on every call. No firmware code or runtime game quality change; not an audio/scanout performance test.',
                'dwt_ctrl_before':ctrl,'demcr_before':demcr,'frames':[]}
        if args.raster_work_stats:
            report['raster_work_start']=d.read_path('smg_raster_work_stats')
            report['raster_work_samples']=[]
        report['geometry_limits']={name:d.memory_path(symbol)['size']//stride for name,symbol,stride in
            [('points','g_frame_points',16),('visible','g_visible_triangles',2),('arena','g_arena',32)]}
        for frame,held in enumerate(buttons):
            if frame%6==0:
                report['samples'].append({'api_frame':frame,**{name:d.read_path(name) for name in CONTEXT}})
                if args.vertex_stats:
                    report['vertex_cache_samples'].append({'api_frame':frame,**d.read_path('smg_vertex_cache_stats')})
                if args.raster_work_stats:
                    report['raster_work_samples'].append({'api_frame':frame,**d.read_path('smg_raster_work_stats')})
            regs=result['registers'];assert regs['r2']==address,'Unexpected game API ABI/framebuffer'
            report['caller_skipped_draws']+=not bool(regs['r3'])
            b.write_register('r0',held);b.write_register('r1',frame_dt(frame,args.fps));b.write_register('r3',1)
            previous_time=d.read_path('g_game.time')
            if frame+1 in args.capture_cull_frame:
                hit=run_to(d,'geometry_cull_chunks')
                raw=b.read_memory(hit['registers']['r0'],176)
                chunks,n=struct.unpack_from('<2I',raw)
                assert 0<n<=65535
                state={'completed_api_call':frame+1,'chunks_address':chunks,'n':n,
                    'planes':[list(struct.unpack_from('<7i',raw,8+p*28)) for p in range(5)],
                    'z_drop':struct.unpack_from('<i',raw,148)[0],
                    'cam':list(struct.unpack_from('<3i',raw,152)),
                    'z_inner':struct.unpack_from('<i',raw,172)[0],
                    'role':'main' if chunks==d.read_path('g_game.stage.chunks') else 'other',
                    'context':{name:d.read_path(name) for name in CONTEXT},
                    'abi_note':'ARM CullJob176B, z_inner172; StageChunk32B, verified by port static assertions.'}
                directory=args.output/'cull-jobs'/f'frame-{frame+1:04d}';directory.mkdir(parents=True,exist_ok=True)
                (directory/'job.bin').write_bytes(raw);(directory/'chunks.bin').write_bytes(b.read_memory(chunks,n*32))
                (directory/'job.json').write_text(json.dumps(state,indent=2)+'\n')
            if frame+1 in args.capture_claim_frame:
                report.setdefault('claim_demand',[]).append(capture_point_claim(d,b,args.output,frame+1))
            if frame+1 in args.capture_transform_frame:
                hit=run_to(d,'engine_transform_points');raw=b.read_memory(hit['registers']['r0'],76)
                src=struct.unpack_from('<I',raw)[0];home=struct.unpack_from('<I',raw,68)[0]
                assert src==d.read_path('g_game.stage.points'),'Expected main-stage transform'
                active=d.read_path('smg_home_cache_stats.active');header=d.read_path('g_game.stage.header');offset=d.read_path('g_game.stage.header.point_home')
                canonical=header+offset if offset else 0;memory=d.read_path('g_home_cache_memory')
                assert home==(memory if active else canonical)
                report.setdefault('transform_bindings',[]).append({'api_frame':frame+1,'source':src,'home':home,'canonical':canonical,'cache':memory,'cached':bool(active),
                    'mapping':'compact' if d.read_path('smg_home_cache_stats').get('compact',0) else 'leaf'})
            if frame==0 and args.probe_address:
                probes=[address&~1 for address in args.probe_address]
                try:
                    for probe in probes:
                        if d.bp(probe)!='OK':raise RuntimeError('Cannot install execution probe')
                    d.resume();hit=d.wait_stopped(10,poll_interval=.005)
                    assert hit['stopped'] and hit['registers']['pc'] in probes,hit
                    report['execution_probe']=hit
                finally:
                    for probe in probes:d.clear_bp(probe)
            run_to(d,regs['lr']) # Complete exactly one call, even if an IRQ arrives at entry.
            if args.instruction_stages:
                from profile_runtime import PERF_STAGES
                values=struct.unpack('<9I',b.read_memory(stages_address,36))
                report['instruction_stages']['frames'].append({'api_call':frame+1,'stages':dict(zip(PERF_STAGES,values))})
                value=b.read_memory(emu+12,1)[0]
                b.write_memory(emu+12,bytes([(value&~3)|1]))
            actual_time=d.read_path('g_game.time')
            assert actual_time-previous_time==frame_dt(frame,args.fps),(frame,previous_time,actual_time)
            try:
                result=run_to(d,'smg_game_frame',timeout=30 if args.instruction_stages else 10)
            except RuntimeError:
                if args.instruction_stages:
                    # Repeated live QMP observations in wait_stopped are not
                    # proof of termination. Halt this owned VM deliberately
                    # to diagnose the wait before its final cleanup.
                    d.halt()
                    diagnostic={'registers':d.regs(),'faults':{hex(a):int.from_bytes(b.read_memory(a,4),'little') for a in [0xe000ed28,0xe000ed2c]},
                                'systick':b.read_memory(0xe000e010,16).hex(),'icsr':b.read_memory(0xe000ed04,4).hex()}
                    pc=diagnostic['registers']['pc']&~1
                    with args.firmware_elf.open('rb') as stream:
                        symbols=ELFFile(stream).get_section_by_name('.symtab')
                        diagnostic['firmware_functions']=[s.name for s in symbols.iter_symbols() if s['st_info']['type']=='STT_FUNC' and (s['st_value']&~1)<=pc<(s['st_value']&~1)+s['st_size']]
                        for name in ['uwTick','SystemCoreClock']:
                            matches=symbols.get_symbol_by_name(name)
                            if matches:diagnostic[name]=int.from_bytes(b.read_memory(matches[0]['st_value'],4),'little')
                    report['instruction_wait_diagnostic']=diagnostic
                    report['measurement_valid']=False
                    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
                raise
            if frame+1 in args.capture_frame:
                capture=args.output/'captures'/f'frame-{frame+1:04d}';capture.mkdir(parents=True,exist_ok=True)
                state={'completed_api_calls':frame+1,**{name:d.read_path(name) for name in
                    CONTEXT+['g_arena_count','g_depth_enabled','g_perspective_enabled','g_display_full_res','smg_render_stats']}}
                state['point_reservations']={'far':d.read_path('g_far_reserve'),
                    'ocean_bowl_active':d.read_path('g_oceanbowl_split'),'ocean_bowl':d.read_path('g_oceanbowl')}
                try:symbol_address(args.elf,'g_scene_cinematic_detail_active')
                except ValueError:pass
                else:state['scene_detail']={'cinematic':d.read_path('g_scene_cinematic_detail_active'),
                    'intro_active':d.read_path('g_intro.active'),'intro_flying':d.read_path('g_intro.flying'),
                    'flight_phase':d.read_path('g_flight.phase'),'gameplay_view_mode':d.read_path('g_game.view_distance')}
                count=state['g_arena_count'];assert 0<=count<=report['geometry_limits']['arena']
                for name,stride in [('g_arena',32),('g_arena_depth',12)]:
                    (capture/(name+'.bin')).write_bytes(b.read_memory(d.at(name),count*stride))
                indexed=b.read_memory(address,expected_size)
                palette=b.read_memory(d.at('smg_palette'),256*4)
                (capture/'indexed.bin').write_bytes(indexed);(capture/'palette.bin').write_bytes(palette)
                from PIL import Image
                rgb_palette=[channel for value in struct.unpack('<256I',palette) for channel in
                             ((value>>16)&255,(value>>8)&255,value&255)]
                width,height=(240,160) if state['g_display_full_res'] else (
                    state['smg_render_stats']['raster_width'],state['smg_render_stats']['raster_height'])
                img=Image.frombytes('P',(320,240),indexed);img.putpalette(rgb_palette)
                img.crop((0,0,width,height)).convert('RGB').resize((320,240),Image.Resampling.NEAREST).save(capture/'world.png')
                state['image_note']='World raster cropped from stride320 and scaled to LCD size; excludes the separately composited HUD and firmware overlay.'
                (capture/'state.json').write_text(json.dumps(state,indent=2)+'\n')
            if frame%120==0:print('api frame',frame,'tick',d.read_path('g_game.tick'),flush=True)
        report['final_context']={name:d.read_path(name) for name in CONTEXT}
        report['geometry']=d.read_path('smg_geometry_stats');report['render_stats']=d.read_path('smg_render_stats')
        try:symbol_address(args.elf,'smg_compact_point_stats')
        except ValueError:pass
        else:report['compact_points']=d.read_path('smg_compact_point_stats')
        if args.home_cache is not None:
            stats=report['home_cache']=d.read_path('smg_home_cache_stats');report['memory']=d.read_path('smg_memory_stats')
            memory=d.read_path('g_home_cache_memory');header=d.read_path('g_game.stage.header');offset=d.read_path('g_game.stage.header.point_home')
            report['home_binding']={'memory':memory,'canonical':header+offset if offset else 0}
            if stats['active']:
                assert memory%4==0 and 0x30000000<=memory<memory+stats['capacity']<=0x30020000
                assert stats['bytes']==d.read_path('g_game.stage.header.point_count')*2<=stats['capacity']
                if not stats.get('compact',0):
                    assert b.read_memory(memory,stats['bytes'])==b.read_memory(header+offset,stats['bytes'])
                    report['home_binding']['bytes_match_canonical']=True
                else:report['home_binding']['mutable_compact_slots']=True
        if args.vertex_stats:report['vertex_cache_final']=d.read_path('smg_vertex_cache_stats')
        if args.raster_work_stats:report['raster_work_final']=d.read_path('smg_raster_work_stats')
        if args.collect_narrow is not None:report['collect_math']=d.read_path('smg_collect_math_stats')
        if args.cull_narrow is not None:report['cull_math']=d.read_path('smg_cull_math_stats')
        if args.visibility_cache is not None:
            stats=report['visibility_cache']=d.read_path('smg_visibility_cache_stats')
            report['memory']=d.read_path('smg_memory_stats')
            binding={'memory':d.read_path('g_visibility_cache_memory'),'main':d.read_path('g_game.stage.chunks'),
                     'canonical_main':d.read_path('g_game.stage.header')+d.read_path('g_game.stage.header.chunks')}
            if stats['active']:
                base=binding['memory'];assert base%32==0
                binding.update(core_bss_end=d.symbols.link_address('__CORE_BSS_END__'),ram_emu_end=d.symbols.link_address('__RAM_EMU_END__'))
                assert binding['core_bss_end']<=base<base+stats['capacity']<=binding['ram_emu_end']
                assert binding['main']==base
                assert b.read_memory(base,stats['main_bytes'])==b.read_memory(binding['canonical_main'],stats['main_bytes'])
                if stats['far_bytes']:
                    binding.update(far=d.read_path('g_far_stage.chunks'),canonical_far=d.read_path('g_far_stage.header')+d.read_path('g_far_stage.header.chunks'))
                    assert binding['far']==base+stats['main_bytes']
                    assert b.read_memory(binding['far'],stats['far_bytes'])==b.read_memory(binding['canonical_far'],stats['far_bytes'])
                binding['copied_bytes_match_canonical']=True
            else:assert binding['main']==binding['canonical_main']
            report['visibility_binding']=binding
        if args.partial_chunk_cache is not None:
            report['partial_chunk_cache']=d.read_path('smg_partial_chunk_cache_stats')
        report['indexed_framebuffer_sha256']=hashlib.sha256(b.read_memory(address,expected_size)).hexdigest()
        if args.instruction_stages:
            report['instruction_stages']['dwt_ctrl_after']=int.from_bytes(b.read_memory(0xe0001000,4),'little')
            assert report['instruction_stages']['dwt_ctrl_after']&1
        report['faults']={hex(a):int.from_bytes(b.read_memory(a,4),'little') for a in [0xe000ed28,0xe000ed2c]}
        assert not any(report['faults'].values())
        report['wall_seconds_not_fps']=time.monotonic()-started
        (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps({'frames':len(buttons),'final_context':report['final_context'],'faults':report['faults']},indent=2),flush=True)
    finally:
        if b is not None:b.close()
        if pid is not None:
            result=subprocess.run(['gwprov','device',f'gwemu:{pid}','stop'],capture_output=True,text=True)
            (args.output/'cleanup.json').write_text(json.dumps({'pid':pid,'stop_exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr},indent=2)+'\n')
            if result.returncode:
                print('VM cleanup failed:',result.stderr,file=sys.stderr)
                if sys.exc_info()[0] is None:raise RuntimeError('Could not stop the owned test VM')


if __name__=='__main__':main()
