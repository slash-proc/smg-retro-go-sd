#!/usr/bin/env python3
"""Compare controlled SMG replay reports; does not infer hardware FPS.

Checks input/timing cohorts, every recorded boundary state, final indexed image,
and geometry counters. SDK skipped blits and host duration are intentionally
separate: the test driver forces game rendering but does not benchmark scanout.
"""
import argparse
import json
from pathlib import Path


def compare(a,b):
    cohort=[]
    for key in ['protocol','timeline_sha256','control_sha256','conversion','requested_frames','startup_stage_override','view_mode_override']:
        if a.get(key)!=b.get(key):cohort.append(key)
    for key in ['timing']:
        if a['cohort'][key]!=b['cohort'][key]:cohort.append('emulator_'+key)
    if a['cohort']['binaryIdentity']['sha256']!=b['cohort']['binaryIdentity']['sha256']:cohort.append('emulator_binary')
    if a.get('protocol')!='api-return-v2' or b.get('protocol')!='api-return-v2':cohort.append('unvalidated_protocol')
    aa={x['api_frame']:x for x in a['samples']};bb={x['api_frame']:x for x in b['samples']}
    complete_samples=all(
        [x['api_frame'] for x in r['samples']]==list(range(0,r['requested_frames'],6))
        for r in [a,b])
    missing=sorted(aa.keys()^bb.keys());different=[]
    for frame in sorted(aa.keys()&bb.keys()):
        keys=sorted(aa[frame].keys()|bb[frame].keys());fields=[k for k in keys if aa[frame].get(k)!=bb[frame].get(k)]
        if fields:different.append({'api_frame':frame,'fields':fields})
    final=[k for k in a['final_context'].keys()|b['final_context'].keys() if a['final_context'].get(k)!=b['final_context'].get(k)]
    geometry={k:[a['geometry'].get(k),b['geometry'].get(k)] for k in a['geometry'].keys()|b['geometry'].keys() if a['geometry'].get(k)!=b['geometry'].get(k)}
    faults=bool(any(a['faults'].values()) or any(b['faults'].values()))
    complete=all(r['geometry']['rendered_frames']==r['requested_frames'] for r in [a,b])
    image=a['indexed_framebuffer_sha256']==b['indexed_framebuffer_sha256']
    identical=not(cohort or missing or different or final or geometry or faults) and image and complete and complete_samples
    return {'note':__doc__.strip(),'cohort_differences':cohort,'missing_boundary_samples':missing,
        'compared_boundary_samples':len(aa.keys()&bb.keys()),'different_boundary_samples':different,
        'final_context_differences':sorted(final),'same_final_indexed_image':image,'geometry_differences':geometry,
        'complete_game_render_calls':complete,'complete_boundary_sampling':complete_samples,
        'faults_present':faults,'identical_recorded_game_workload':identical,
        'same_game_elf':a.get('elf_sha256')==b.get('elf_sha256'),
        'caller_skipped_blits':[a['caller_skipped_draws'],b['caller_skipped_draws']],
        'wall_seconds_not_fps':[a['wall_seconds_not_fps'],b['wall_seconds_not_fps']]}


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('a',type=Path);p.add_argument('b',type=Path);p.add_argument('output',type=Path);p.add_argument('--require-identical',action='store_true');args=p.parse_args()
    result=compare(json.loads(args.a.read_text()),json.loads(args.b.read_text()));args.output.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    if args.require_identical and not result['identical_recorded_game_workload']:raise SystemExit(1)
