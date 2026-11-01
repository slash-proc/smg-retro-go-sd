#!/usr/bin/env python3
"""Capture a stopped SMG renderer with GWProv, always restoring VM run state.

The draw lists may be mid-frame; depth_active and PC identify that limitation.
The installed ELF and relocation report must be preserved alongside the dump.
"""
import argparse
import json
from pathlib import Path
from types import SimpleNamespace
from gwprov.debug_shell import DebugSession
from gwprov.qmp import QMPConnection

VALUES = ['g_arena_count', 'g_game.stage_index', 'g_game.scene', 'g_game.view_distance',
          'g_game.camera', 'g_game.player.figure.position', 'g_game.stage', 'g_far_stage',
          'g_engine_fb', 'smg_geometry_stats', 'smg_render_stats', 'g_depth_enabled',
          'g_depth_active', 'g_depth_effect_count', 'smg_assets_xip_runtime_base',
          'smg_code_xip_runtime_base']
VALUES += ['g_projection_divide_enabled', 'g_raster_setup_recip_enabled',
           'g_perspective_enabled', 'g_perspective_active', 'g_perspective_reuse_enabled',
           'g_texture_cache_dtcm', 'g_texture_dtcm_enabled', 'g_texture_dtcm_active']
VALUES += ['smg_vertex_cache_stats', 'g_stage_vertex_cache_enabled',
           'g_stage_vertex_cache_source', 'g_stage_vertex_cache']
VALUES += ['g_game.scenario', 'g_game.lives', 'g_game.player.on_ground',
           'g_game.player.has_floor_below', 'g_game.player.up', 'g_game.player.velocity',
           'g_game.player.action', 'g_collision', 'smg_collision_incident_sequence',
           'smg_collision_incidents']
BLOCKS = ['g_arena', 'g_triangle_bin_heads', 'g_figure_bin_heads', 'g_frame_points',
          'g_screen_positions', 'g_visible_triangles', 'g_leaf_delta', 'g_point_marks',
          'g_texture_cache', 'g_texture_cache_reference', 'g_texture_cache_slot',
          'g_depth_buffer', 'g_arena_depth', 'g_depth_effects']

def capture(device, elf, directory):
    directory.mkdir(parents=True, exist_ok=True)
    with QMPConnection(device, timeout=20) as q:
        d = DebugSession(SimpleNamespace(read_memory=q.read_memory), 'gwemu', qmp_socket=device)
        d.symbols.load(elf)
        state = {'device': device, 'elf': str(elf), 'addresses': {}, 'sizes': {}, 'errors': {}}
        was_running = q.execute('query-status')['return']['running']
        q.execute('stop')
        try:
            state['registers'] = q.registers()
            state['faults'] = {hex(a): q.u32(a) for a in (0xe000ed28,0xe000ed2c,0xe000ed38)}
            for name in VALUES:
                try: state[name] = d.read_path(name)
                except Exception as e: state['errors'][name] = str(e)
            axi = b''.join(q.read_memory(a,65536) for a in range(0x24000000,0x24100000,65536))
            (directory/'axi.bin').write_bytes(axi)
            for name in BLOCKS:
                try:
                    layout = d.memory_path(name)
                    base, size = layout['address'], layout['size']
                    if not 0x24000000 <= base <= base+size <= 0x24100000:
                        raise ValueError('block outside captured AXI RAM')
                    state['addresses'][name] = base
                    state['sizes'][name] = size
                    (directory/(name+'.bin')).write_bytes(axi[base-0x24000000:base-0x24000000+size])
                except Exception as e: state['errors'][name] = str(e)
            if state.get('g_texture_cache_dtcm'):
                try:
                    base = state['g_texture_cache_dtcm']
                    size = d.memory_path('g_texture_cache')['size']
                    if not 0x20000000 <= base < base + size <= 0x20020000:
                        raise ValueError('texture allocation outside physical DTCM')
                    (directory/'g_texture_cache_dtcm.bin').write_bytes(q.read_memory(base,size))
                    state['addresses']['g_texture_cache_dtcm_bytes'] = base
                    state['sizes']['g_texture_cache_dtcm_bytes'] = size
                except Exception as e: state['errors']['g_texture_cache_dtcm_bytes'] = str(e)
            state['screenshot'] = d.screenshot(directory/'screen.png')
            (directory/'state.json').write_text(json.dumps(state,indent=2)+'\n')
        finally:
            if was_running: q.execute('cont')
    print(json.dumps({k:state.get(k) for k in ['g_game.stage_index','g_game.camera','g_depth_enabled','g_depth_active','errors']},indent=2))

if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('device');parser.add_argument('elf',type=Path);parser.add_argument('directory',type=Path)
    args=parser.parse_args();capture(args.device,args.elf,args.directory.resolve())
