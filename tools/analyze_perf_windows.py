#!/usr/bin/env python3
"""Recover interval costs from saved frame counters without touching a target."""
import argparse
from collections import Counter
import json
from pathlib import Path

STAGES = ("logic", "world", "visibility", "transform", "emit", "raster", "palette", "scale", "hud")


def analyze(path):
    report = json.loads(path.read_text())
    if not report.get("instrumentation", {}).get("available"):
        return None
    if any("Rejected timing comparison" in w for w in report.get("warnings", [])):
        return None
    previous = report["instrumentation_raw"]["before"]
    previous_time = 0
    windows = []
    snapshots = [(c["elapsed_s"], c.get("perf")) for c in report["contexts"]]
    snapshots.append((report["duration_s"], report["instrumentation_raw"]["after"]))
    for elapsed, current in snapshots:
        if current is None:
            continue
        frames = (current["frames"] - previous["frames"]) & 0xffffffff
        if frames:
            scale = 1000 / report["clock_hz"] / frames
            costs = {name: (b-a)*scale for name, a, b in zip(STAGES, previous["cycles"], current["cycles"])}
            windows.append({"start_s": previous_time, "end_s": elapsed, "frames": frames,
                "work_ms": (current["work_cycles"]-previous["work_cycles"])*scale,
                "cost_ms": costs, "over_60": current["over_60"]-previous["over_60"],
                "over_30": current["over_30"]-previous["over_30"]})
        previous, previous_time = current, elapsed
    phases = []
    for start, end in ((0, 6), (6, float("inf"))):
        selected = [w for w in windows if start <= w["start_s"] < end]
        frames = sum(w["frames"] for w in selected)
        if not frames:
            continue
        mean = sum(w["work_ms"]*w["frames"] for w in selected)/frames
        phases.append({"start_s": selected[0]["start_s"], "end_s": selected[-1]["end_s"],
            "frames": frames, "work_ms": mean,
            "reduction_to_60_percent": max(0, 100*(1-(1000/60)/mean)),
            "cost_ms": {s: sum(w["cost_ms"][s]*w["frames"] for w in selected)/frames for s in STAGES},
            "pc_counts_not_cycle_shares": Counter(s["function"] for s in report["samples"]
                if selected[0]["start_s"] < s["elapsed_s"] <= selected[-1]["end_s"]),
            "over_60": sum(w["over_60"] for w in selected),
            "over_30": sum(w["over_30"] for w in selected)})
    return {"source": str(path), "phases": phases, "windows": windows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("folder", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    results = []
    for path in sorted(args.folder.glob("hardware-d-*.json")):
        result = analyze(path)
        if result:
            results.append(result)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps({"notes": [
        "Intervals crossing six seconds stay together; exact phase bounds are recorded.",
        "World includes visibility/transform/emission/raster; do not sum parent and child counters.",
        "Emission also includes the far pass; transform only covers the main stage, not every figure.",
        "Old scripted routes are not representative; these costs describe only recorded workloads.",
        "PC counts are sparse fixed-rate samples, not reliable cycle percentages."
    ], "captures": results}, indent=2) + "\n")
    for result in results:
        print(Path(result["source"]).stem)
        for p in result["phases"]:
            costs = ", ".join(f"{s}={p['cost_ms'][s]:.2f}" for s in STAGES)
            print(f"  {p['start_s']:.2f}–{p['end_s']:.2f}s: {p['work_ms']:.2f} ms; {costs}")


if __name__ == "__main__":
    main()
