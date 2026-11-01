"""Matching passive SMG counter/PC captures from a GWProv Python console.

    import sys
    sys.path.insert(0, '/path/to/smg-retro-go-sd/tools')
    from profile_runtime import capture
    report = capture(dbg, '/path/to/report.json', label='hardware')

Hardware PCs come from DWT.PCSR, without halting. GWemu PCs and memory come
from QMP. Neither path writes target memory or changes execution state.
"""

from __future__ import annotations

import csv
import hashlib
import json
import math
import struct
import time
from bisect import bisect_right
from collections import Counter
from contextlib import nullcontext, contextmanager
from datetime import datetime, timezone
from pathlib import Path


def _quantile(values, fraction):
    if not values:
        return None
    values = sorted(values)
    return values[round((len(values) - 1) * fraction)]


PERF_STAGES = ("logic", "world", "visibility", "transform", "emit", "raster",
               "palette", "scale", "hud")


def _perf_snapshot(reader, address, *, history=True):
    for _ in range(5):
        data = reader(address, 1200 if history else 176)
        version, seq, frames, target = struct.unpack_from("<4I", data)
        if version != 1:
            raise ValueError(f"unsupported SMG perf ABI {version}")
        if seq & 1 or int.from_bytes(reader(address + 4, 4), "little") != seq:
            continue
        return {"frames": frames, "target_fps": target,
                "cycles": list(struct.unpack_from("<9Q", data, 16)),
                "work_cycles": struct.unpack_from("<Q", data, 88)[0],
                "max_work_cycles": struct.unpack_from("<I", data, 96)[0],
                "over_30": struct.unpack_from("<I", data, 100)[0],
                "over_60": struct.unpack_from("<I", data, 104)[0],
                "histogram": list(struct.unpack_from("<16I", data, 108)),
                "ring_index": struct.unpack_from("<I", data, 172)[0],
                "recent_work_cycles": list(struct.unpack_from("<256I", data, 176)) if history else []}
    raise RuntimeError("could not obtain a consistent SMG performance snapshot")


def _perf_summary(before, after, clock_hz, intermediate=()):
    frames = (after["frames"] - before["frames"]) & 0xffffffff
    if not frames or after["work_cycles"] == before["work_cycles"]:
        return {"available": False, "reason": "no DWT cycle measurements", "target_fps": after["target_fps"]}
    ms_per_cycle = 1000 / clock_hz
    recent_count = min(frames, 256)
    recent = [after["recent_work_cycles"][(after["ring_index"] - 1 - i) & 255] * ms_per_cycle
              for i in range(recent_count)]
    work_times = []
    last_index = before["ring_index"]
    missed = 0
    for snapshot in [*intermediate, after]:
        if snapshot is None or not snapshot["recent_work_cycles"]:
            continue
        delta = (snapshot["ring_index"] - last_index) & 0xffffffff
        missed += max(0, delta - 256)
        work_times.extend(snapshot["recent_work_cycles"][(snapshot["ring_index"] - i) & 255] * ms_per_cycle
                          for i in range(min(delta, 256), 0, -1))
        last_index = snapshot["ring_index"]
    return {"available": True, "frames": frames, "target_fps": after["target_fps"],
            "work_ms_per_frame": (after["work_cycles"] - before["work_cycles"]) * ms_per_cycle / frames,
            "stage_ms_per_frame": {name: (b-a) * ms_per_cycle / frames
                for name, a, b in zip(PERF_STAGES, before["cycles"], after["cycles"])},
            "over_30_budget": after["over_30"] - before["over_30"],
            "over_60_budget": after["over_60"] - before["over_60"],
            "work_histogram_2ms": [b-a for a,b in zip(before["histogram"], after["histogram"])],
            "max_work_ms_since_boot": after["max_work_cycles"] * ms_per_cycle,
            "recent_frames": recent_count, "recent_work_ms_p50": _quantile(recent, .5),
            "recent_work_ms_p95": _quantile(recent, .95), "recent_work_ms_max": max(recent),
            "observed_work_frames": len(work_times), "unobserved_work_frames": missed,
            "observed_work_ms_max": max(work_times) if work_times else None,
            "observed_work_ms_p95": _quantile(work_times, .95),
            "observed_work_ms_p99": _quantile(work_times, .99),
            "note": "world includes visibility/transform/emit/raster; these nested times must not be added together"}


@contextmanager
def _input_driver(dbg, events):
    events = list(events or [])
    if any(not math.isfinite(t) or t < 0 or not isinstance(mask, int) or mask < 0
           for t, mask in events) or events != sorted(events, key=lambda e: e[0]):
        raise ValueError("input events must be sorted nonnegative (seconds, shadow-mask) pairs")
    address = dbg.at("__remote_input__") if events else None
    fired = []
    cursor = 0
    def apply(elapsed):
        nonlocal cursor
        while cursor < len(events) and events[cursor][0] <= elapsed:
            when, mask = events[cursor]
            dbg.write_u32(address, mask)
            fired.append({"scheduled_s": when, "elapsed_s": elapsed, "mask": mask})
            cursor += 1
    try:
        yield apply, fired
    finally:
        if address is not None:
            dbg.write_u32(address, 0)


def capture(dbg, output, *, label, duration=60.0, hz=10.0, startup_timeout=120.0,
            input_events=None):
    """Capture cumulative timing counters at fixed deadlines after gameplay starts."""
    if not math.isfinite(duration) or duration <= 0 or not math.isfinite(hz) or hz <= 0:
        raise ValueError("duration and sampling frequency must be positive")
    from gwprov.qmp import QMPConnection

    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    stats_address = dbg.at("smg_render_stats")
    vm = dbg.transport == "gwemu"
    if vm and not dbg.qmp_socket:
        raise ValueError("VM capture requires QMP")
    clock_hz = dbg.read_value("SystemCoreClock")
    context_paths = [
        "g_game.stage_index", "g_game.scenario", "g_game.scene", "g_game.tick",
        "g_game.player.figure.transform.o", "g_game.player.health",
    ]
    symbols = dbg.symbols
    warnings = []
    with (QMPConnection(dbg.qmp_socket) if vm else nullcontext()) as qmp, _input_driver(dbg, input_events) as input_state:
        reader = qmp.read_memory if vm else dbg.read
        apply_input, fired_inputs = input_state

        def counters():
            frames, game_ms, blit_ms, width, height = struct.unpack("<5I", reader(stats_address, 20))
            return {"frames": frames, "game_ms": game_ms, "blit_ms": blit_ms,
                    "width": width, "height": height}

        deadline = time.monotonic() + startup_timeout
        previous = None
        while time.monotonic() < deadline:
            values = counters()
            scene = dbg.read_path("g_game.scene")
            if (previous is not None and values["frames"] > previous["frames"]
                    and scene == 1 and values["width"] and values["height"]):
                break
            previous = values
            time.sleep(0.2)
        else:
            raise RuntimeError("gameplay did not start advancing before the timeout; unpause before capture")

        rebases = symbols.rebase_from_runtime_pointers(reader)
        functions = sorted(
            (row for row in symbols.functions() if row["type"] == "STT_FUNC" and row["size"] > 0),
            key=lambda row: (row["address"] & ~1, row["name"]),
        )
        starts = [row["address"] & ~1 for row in functions]

        def resolve(pc):
            index = bisect_right(starts, pc & ~1) - 1
            if index >= 0:
                row = functions[index]
                if (pc & ~1) < (row["address"] & ~1) + row["size"]:
                    return row["name"]
            return None

        before_context = dbg.read_paths(context_paths)["values"]
        initial_faults = {"cfsr": int.from_bytes(reader(0xE000ED28, 4), "little"),
                          "hfsr": int.from_bytes(reader(0xE000ED2C, 4), "little")}
        baseline = counters()
        try:
            perf_address = dbg.at("smg_perf")
        except (KeyError, ValueError, RuntimeError):
            perf_address = None
        perf_before = _perf_snapshot(reader, perf_address) if perf_address is not None else None
        started_at = datetime.now(timezone.utc).isoformat()
        started = time.monotonic()
        samples = []
        contexts = []
        count = round(duration * hz)
        for index in range(1, count + 1):
            scheduled = index / hz
            remaining = started + scheduled - time.monotonic()
            if remaining > 0:
                time.sleep(remaining)
            apply_input(time.monotonic() - started)
            acquired = time.monotonic()
            values = counters()
            dwt = struct.unpack("<8I", reader(0xE0001000, 32))
            regs = qmp.registers() if vm else None
            pc = (regs["pc"] if regs else dwt[7]) & ~1
            finished = time.monotonic()
            function = resolve(pc)
            row = {
                "index": index, "scheduled_s": scheduled, "elapsed_s": finished - started,
                "lateness_s": max(0.0, acquired - started - scheduled),
                "acquisition_s": finished - acquired,
                **values, "pc": pc, "function": function,
                "dwt_ctrl": dwt[0], "dwt_cyccnt": dwt[1],
            }
            prior = samples[-1] if samples else {**baseline, "elapsed_s": 0.0}
            frames = (values["frames"] - prior["frames"]) & 0xFFFFFFFF
            if frames > 10000:
                raise RuntimeError("frame counter reset during capture")
            span = row["elapsed_s"] - prior["elapsed_s"]
            row["frames_delta"] = frames
            row["bin_fps"] = frames / span
            row["bin_game_ms_per_frame"] = (
                ((values["game_ms"] - prior["game_ms"]) & 0xFFFFFFFF) / frames if frames else None
            )
            row["bin_blit_ms_per_frame"] = (
                ((values["blit_ms"] - prior["blit_ms"]) & 0xFFFFFFFF) / frames if frames else None
            )
            samples.append(row)
            if index % round(hz) == 0:
                contexts.append({"elapsed_s": time.monotonic() - started,
                                 "values": dbg.read_paths(context_paths)["values"],
                                 "perf": _perf_snapshot(reader, perf_address) if perf_address is not None else None})
            if index % max(1, round(hz * 10)) == 0:
                elapsed = time.monotonic() - started
                print(f"{label}: {index}/{count} samples, {(values['frames'] - baseline['frames']) / elapsed:.2f} frames/s", flush=True)

        elapsed = samples[-1]["elapsed_s"]
        perf_after = _perf_snapshot(reader, perf_address) if perf_address is not None else None
        final = samples[-1]
        frames = (final["frames"] - baseline["frames"]) & 0xFFFFFFFF
        histogram = Counter(row["function"] or "unresolved" for row in samples)
        gaps = [b["elapsed_s"] - a["elapsed_s"] for a, b in zip(samples, samples[1:])]
        if max(row["lateness_s"] for row in samples) >= 1 / hz:
            warnings.append("At least one acquisition missed its 100 ms deadline; consult lateness and sample gaps.")
        if not vm and not any(row["function"] for row in samples):
            warnings.append("DWT PCSR did not provide symbolized hardware PCs.")
        report = {
            "schema_version": 1, "label": label, "started_at": started_at,
            "transport": dbg.transport, "requested_duration_s": duration, "requested_hz": hz,
            "duration_s": elapsed, "sample_count": len(samples), "observed_hz": len(samples) / elapsed,
            "raster": [baseline["width"], baseline["height"]], "lcd": [320, 240],
            "clock_hz": clock_hz, "rebases": rebases,
            "pc_method": "QMP running-register snapshot" if vm else "DWT PCSR passive read",
            "counter_method": "cumulative game-frame and compositor HAL_GetTick milliseconds",
            "dwt_cycles_note": "Raw CYCCNT observations; verify enable state, resets, and wrap behavior before deriving elapsed cycles.",
            "workload": "scripted shadow input; live player/stage context recorded every second" if input_events else "idle gameplay; live player/stage context recorded every second; no injected input during capture",
            "input_events": fired_inputs,
            "frames": frames, "wall_fps": frames / elapsed,
            "game_ms_per_frame": ((final["game_ms"] - baseline["game_ms"]) & 0xFFFFFFFF) / frames if frames else None,
            "composite_ms_per_frame": ((final["blit_ms"] - baseline["blit_ms"]) & 0xFFFFFFFF) / frames if frames else None,
            "sampling": {
                "acquisition_ms_mean": sum(row["acquisition_s"] for row in samples) / len(samples) * 1000,
                "acquisition_ms_p95": _quantile([row["acquisition_s"] * 1000 for row in samples], .95),
                "lateness_ms_max": max(row["lateness_s"] for row in samples) * 1000,
                "gap_ms_p95": _quantile([gap * 1000 for gap in gaps], .95),
                "gap_ms_max": max(gaps) * 1000,
            },
            "hotspots": [{"function": name, "samples": hits, "percent": hits * 100 / len(samples)}
                         for name, hits in histogram.most_common()],
            "initial_faults": initial_faults,
            "final_faults": {"cfsr": int.from_bytes(reader(0xE000ED28, 4), "little"),
                             "hfsr": int.from_bytes(reader(0xE000ED2C, 4), "little")},
            "context_before": before_context, "contexts": contexts,
            "warnings": warnings, "baseline": baseline, "samples": samples,
            "symbol_files": [{"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                             for path in symbols.sources],
        }
        if perf_before is not None:
            report["instrumentation"] = _perf_summary(perf_before, perf_after, clock_hz,
                                                       [c.get("perf") for c in contexts])
            report["instrumentation_raw"] = {"before": perf_before, "after": perf_after}
    output.write_text(json.dumps(report, indent=2) + "\n")
    with output.with_suffix(".csv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=list(samples[0]))
        writer.writeheader()
        writer.writerows(samples)
    print(json.dumps({key: report[key] for key in ("label", "sample_count", "duration_s", "wall_fps", "game_ms_per_frame", "composite_ms_per_frame", "sampling")}, indent=2), flush=True)
    return report
