#!/usr/bin/env python3
"""Compare guest-internal baseline instruction work; never convert to hardware FPS."""
import argparse
import json
from pathlib import Path

from compare_game_replays import compare

STAGES = ('logic', 'world', 'visibility', 'transform', 'emit', 'raster')


def summary(control, candidate):
    fidelity = compare(control, candidate)
    assert fidelity['identical_recorded_game_workload'], fidelity
    assert control['cohort']['timing']['mode'] == 'baseline'
    assert candidate['cohort']['timing']['mode'] == 'baseline'
    a = control['instruction_stages']['frames']
    b = candidate['instruction_stages']['frames']
    assert [f['api_call'] for f in a] == list(range(1, control['requested_frames'] + 1))
    assert [f['api_call'] for f in a] == [f['api_call'] for f in b]
    assert all(not f['stages'][key] for f in a+b for key in ('palette', 'scale', 'hud'))
    for row in a+b:
        assert row['stages']['world'] >= row['stages']['raster']
    skip = control['conversion']['warmup_frames']
    fps = control['conversion']['fps']
    assert fps in (30, 60)

    def aggregate(first, last):
        result = {}
        for stage in STAGES:
            old = sum(row['stages'][stage] for row in a[first:last])
            new = sum(row['stages'][stage] for row in b[first:last])
            result[stage] = dict(control_per_call=old/(last-first), candidate_per_call=new/(last-first),
                                 percent_reduction=100*(old-new)/old if old else None)
        return dict(first_api_call=first+1, last_api_call=last, calls=last-first, stages=result)

    windows = [aggregate(start, min(start+6, len(a))) for start in range(skip, len(a), 6)]
    return dict(note=__doc__.strip(), fidelity=fidelity, warmup_calls_excluded=skip,
                api_cadence_fps=fps, window_calls=6, nominal_window_seconds=6/fps,
                active=aggregate(skip, len(a)), opening=aggregate(skip, min(6*fps, len(a))),
                worst_control_world_windows=sorted(windows, key=lambda x: x['stages']['world']['control_per_call'], reverse=True)[:10],
                worst_control_raster_windows=sorted(windows, key=lambda x: x['stages']['raster']['control_per_call'], reverse=True)[:10],
                windows=windows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['control', 'candidate', 'output']:
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    result = summary(json.loads(args.control.read_text()), json.loads(args.candidate.read_text()))
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: result[key] for key in ('active', 'opening')}, indent=2))


if __name__ == '__main__':
    main()
