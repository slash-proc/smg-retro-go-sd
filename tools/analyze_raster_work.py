#!/usr/bin/env python3
"""Summarize diagnostic fragment work; no host/VM timing or hardware FPS claims."""
import argparse
import json
from pathlib import Path

MODES = ('flat', 'texture', 'cutout', 'additive', 'darken', 'tint', 'quarter_blend')
FIELDS = ('triangles', 'covered_pixels', 'depth_rejected_pixels', 'texture_samples',
          'zero_texels', 'colour_writes', 'depth_writes', 'perspective_spans',
          'perspective_pixels', 'perspective_divisions')


def subtract(before, after):
    assert before['version'] == after['version'] == 1
    assert len(before['mode']) == len(after['mode']) == len(MODES)
    return {mode: {key: (b[key] - a[key]) & 0xffffffff for key in FIELDS}
            for mode, a, b in zip(MODES, before['mode'], after['mode'])}


def summarize(report):
    modes = subtract(report['raster_work_start'], report['raster_work_final'])
    total = {key: sum(mode[key] for mode in modes.values()) for key in FIELDS}
    assert total['triangles'] == report['geometry']['raster_triangles']
    assert total['covered_pixels'] >= total['depth_rejected_pixels']
    assert total['texture_samples'] <= total['covered_pixels'] - total['depth_rejected_pixels']
    assert total['colour_writes'] <= total['covered_pixels'] - total['depth_rejected_pixels']
    assert total['depth_writes'] <= total['colour_writes']
    assert total['perspective_pixels'] <= total['covered_pixels']
    windows = []
    samples = report['raster_work_samples'] + [dict(api_frame=report['requested_frames'], **report['raster_work_final'])]
    for before, after in zip(samples, samples[1:]):
        frames = after['api_frame'] - before['api_frame']
        assert frames > 0
        work = subtract(before, after)
        totals = {key: sum(mode[key] for mode in work.values()) for key in FIELDS}
        windows.append(dict(first_api_frame=before['api_frame'] + 1,
                            last_api_frame=after['api_frame'], frames=frames,
                            per_frame={key: value / frames for key, value in totals.items()}))
    rejected = total['depth_rejected_pixels']
    covered = total['covered_pixels']
    return dict(
        note=__doc__.strip(), requested_frames=report['requested_frames'],
        depth_enabled=report.get('depth_buffer_enabled'),
        perspective_enabled=report.get('perspective_enabled'),
        modes=modes, total=total,
        per_frame={key: value / report['requested_frames'] for key, value in total.items()},
        depth_rejected_fraction=rejected / covered if covered else None,
        # Span generation currently precedes each pixel's depth test. These
        # totals alone cannot identify fully occluded spans or saved divisions.
        occluded_perspective_pixels=sum(modes[key]['depth_rejected_pixels'] for key in ('texture', 'cutout', 'quarter_blend')) if report.get('perspective_enabled') else 0,
        busiest_coverage_windows=sorted(windows, key=lambda x: x['per_frame']['covered_pixels'], reverse=True)[:10],
        busiest_perspective_windows=sorted(windows, key=lambda x: x['per_frame']['perspective_divisions'], reverse=True)[:10],
        windows=windows,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = summarize(json.loads(args.report.read_text()))
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: value for key, value in result.items() if key not in ('modes', 'windows', 'busiest_coverage_windows', 'busiest_perspective_windows')}, indent=2))


if __name__ == '__main__':
    main()
