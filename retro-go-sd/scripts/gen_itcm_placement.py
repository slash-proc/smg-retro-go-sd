#!/usr/bin/env python3
"""Select measured hot input sections without changing compiler options or firmware."""
import argparse
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--enabled', choices=['0', '1'], required=True)
parser.add_argument('--visibility', choices=['0', '1'], default='1')
parser.add_argument('--emission', choices=['0', '1'], default='1')
parser.add_argument('--out', type=Path, required=True)
args = parser.parse_args()
sections = ['.text.engine_raster_inner*', '.text.engine_raster.part*',
            '.text.raster_mip_reciprocal*',
            '.rodata.inverse_steps*', '.rodata.mip_recip_table*']
if args.visibility == '1':
    sections += ['.text.geometry_cull_chunks*', '.text.geometry_collect_leaves_c*',
                 '.text.collect_visible_chunks*', '.text.collect_visible_triangles*',
                 '.text.engine_transform_points*', '.text.rank_bits_to_triangles*',
                 '.text.geometry_claim_points*', '.text.geometry_claim_compact_points*',
                 '.text.compact_find_point*']
if args.emission == '1':
    sections += ['.text.engine_push*', '.text.clip_polygon*', '.text.emit_split_vertex*',
                 '.text.engine_emit_clipped*', '.text.emit_face*', '.text.emit_subdivided*',
                 '.text.emit_stage_face_bins*', '.text.emit_stage_slow*',
                 '.text.emit_through_water*', '.text.emit_scroll_uv*', '.text.engine_emit_stage*']
# The scene dispatcher runs once per rendered frame, and the far pass is
# entered on every stage frame that has an authored far mesh. Keep their small
# orchestration functions in ITCM alongside the raster/emission kernels.
sections += ['.text.draw_scene*', '.text.draw_scene_bins*', '.text.scene_far_pass*']
text = '/* Generated SMG hot-code placement. */\n'
if args.enabled == '1':
    text += 'build/smg/main.o (' + ' '.join(sections) + ')\n'
else:
    text += '/* AXI reference build: no ITCM input sections. */\n'
args.out.parent.mkdir(parents=True, exist_ok=True)
if not args.out.exists() or args.out.read_text() != text:
    args.out.write_text(text)
