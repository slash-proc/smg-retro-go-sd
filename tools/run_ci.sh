#!/bin/sh
set -eu

mkdir -p tmp
python3 -m unittest discover -s tools/tests -p 'test_*.py'
python3 tools/audit_render_contracts.py
for test in \
    raster visibility figure_paths scene_mmio sky_wrap depth_quantize \
    perspective_raster opaque_raster water_blend texture_address compact_points
do
    echo "Running $test"
    python3 "tools/test_$test.py"
done
