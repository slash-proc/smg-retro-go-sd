"""Summarize and plot the paired 60-second SMG hardware/VM captures."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def analyze(folder: Path):
    reports = {name: json.loads((folder / f"{name}.json").read_text()) for name in ("hardware", "vm")}
    summaries = {}
    for name, report in reports.items():
        samples = report["samples"]
        span = samples[-1]["elapsed_s"] - samples[0]["elapsed_s"]
        deltas = [(b["dwt_cyccnt"] - a["dwt_cyccnt"]) & 0xFFFFFFFF for a, b in zip(samples, samples[1:])]
        enabled = all(row["dwt_ctrl"] & 1 for row in samples)
        changed = any(deltas)
        rate = sum(deltas) / span if enabled and changed else None
        observation = {
            "enabled_throughout": enabled,
            "counter_changed": changed,
            "observed_advance_per_wall_second": rate,
            "modulo32_decreases": sum(b["dwt_cyccnt"] < a["dwt_cyccnt"] for a, b in zip(samples, samples[1:])),
            "maximum_modulo32_step": max(deltas),
            "note": (
                "Hardware trace is consistent with a continuous counter and four 32-bit wraps; the port does not call the firmware's per-loop reset helper."
                if name == "hardware" and enabled and changed
                else "Default VM mode reports CYCCNT disabled and constant zero; it supplies no elapsed-cycle measurement."
            ),
        }
        report["dwt_observation"] = observation
        report["dwt_cycles_note"] = observation["note"]
        report["sampling"]["acquisition_ms_max"] = max(row["acquisition_s"] for row in samples) * 1000
        report["sampling"]["aliasing_note"] = "Fixed 10 Hz PC snapshots can alias with a 60 Hz frame clock; shares are hotspot estimates, not cycle counts."
        (folder / f"{name}.json").write_text(json.dumps(report, indent=2) + "\n")
        contexts = [report["context_before"], *(item["values"] for item in report["contexts"])]
        summaries[name] = {
            key: report[key] for key in (
                "sample_count", "duration_s", "observed_hz", "wall_fps", "game_ms_per_frame",
                "composite_ms_per_frame", "raster", "lcd", "clock_hz", "sampling", "hotspots", "final_faults",
            )
        }
        summaries[name]["work_ms_per_frame"] = report["game_ms_per_frame"] + report["composite_ms_per_frame"]
        summaries[name]["dwt"] = observation
        summaries[name]["same_stage_throughout"] = all(c["g_game.stage_index"] == 0 and c["g_game.scenario"] == 0 and c["g_game.scene"] == 1 for c in contexts)
        summaries[name]["stationary_throughout"] = all(c["g_game.player.figure.transform.o"] == contexts[0]["g_game.player.figure.transform.o"] for c in contexts)
    h, v = summaries["hardware"], summaries["vm"]
    elf_hashes = [next(row["sha256"] for row in report["symbol_files"] if row["path"].endswith("/smg_core.elf")) for report in reports.values()]
    result = {
        "hardware": h, "vm": v,
        "same_game_elf": elf_hashes[0] == elf_hashes[1], "game_elf_sha256": elf_hashes[0],
        "same_player_position": reports["hardware"]["context_before"]["g_game.player.figure.transform.o"] == reports["vm"]["context_before"]["g_game.player.figure.transform.o"],
        "hardware_to_vm_work_ratio": h["work_ms_per_frame"] / v["work_ms_per_frame"],
        "hardware_to_vm_game_ratio": h["game_ms_per_frame"] / v["game_ms_per_frame"],
        "hardware_to_vm_compositor_ratio": h["composite_ms_per_frame"] / v["composite_ms_per_frame"],
        "vm_timing_mode": "default, unthrottled; firmware still paces output",
        "preflight": "Optional baseline one-instruction-one-cycle mode stalled in common_emu_sound_sync after one frame; no 60-second measurement was taken of that stalled startup.",
        "interpretation": [
            "Default VM mode underestimates the observed game/render/compositor work for this scene by about 6.2 times.",
            "The VM spends 67.5% of PC snapshots in cpumon_common, while the hardware is occupied by rendering and compositing.",
            "Use per-frame work and hotspot context for VM calibration; 60 FPS output alone hides excess virtual CPU headroom.",
            "Missing meshes shared by VM and hardware require a game renderer/asset investigation; the performance difference does not explain the shared visual defects.",
            "Movement tones come from the template tone_hz/submit_audio path; game audio is still stubbed.",
        ],
    }
    (folder / "comparison.json").write_text(json.dumps(result, indent=2) + "\n")
    return reports, result


def plot(folder: Path, reports, comparison):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import numpy as np

    colors = {"hardware": "#0072B2", "vm": "#D55E00"}
    fig = plt.figure(figsize=(12, 10), constrained_layout=True)
    grid = fig.add_gridspec(3, 2, height_ratios=[1, 1, 1.5])
    fps = fig.add_subplot(grid[0, :])
    for name, report in reports.items():
        samples = report["samples"]
        xs, ys = [], []
        previous_frames = report["baseline"]["frames"]
        previous_time = 0.0
        for index in range(9, len(samples), 10):
            row = samples[index]
            xs.append(row["elapsed_s"])
            ys.append((row["frames"] - previous_frames) / (row["elapsed_s"] - previous_time))
            previous_frames, previous_time = row["frames"], row["elapsed_s"]
        fps.plot(xs, ys, color=colors[name], label=f"{name}: {report['wall_fps']:.2f} FPS")
    fps.set(xlabel="Wall time (s)", ylabel="Rendered frames/s", title="One-second bins from 10 Hz counter samples", ylim=(0, 70))
    fps.grid(alpha=.2)
    fps.legend(loc="lower right")

    work = fig.add_subplot(grid[1, 0])
    names = ["hardware", "vm"]
    game = [reports[name]["game_ms_per_frame"] for name in names]
    blit = [reports[name]["composite_ms_per_frame"] for name in names]
    work.bar(names, game, label="Game + world renderer", color="#56B4E9")
    work.bar(names, blit, bottom=game, label="HUD + compositor", color="#E69F00")
    work.axhline(1000 / 60, color="black", linestyle="--", linewidth=1, label="60 FPS budget")
    work.set(ylabel="HAL milliseconds per rendered frame", title="Mean measured work per frame")
    work.legend(fontsize=8)
    note = fig.add_subplot(grid[1, 1])
    note.axis("off")
    note.text(0, 1, f"600 samples per target, 60 seconds each\n160×120 world; 320×240 HUD/output\nObservatory: identical stationary player position\nSame game ELF; no fault flags\n\nHardware / VM work ratio: {comparison['hardware_to_vm_work_ratio']:.2f}×\nHardware DWT: {comparison['hardware']['dwt']['observed_advance_per_wall_second']/1e6:.1f} M cycles/s\nDefault VM DWT: disabled / constant zero\n\nPC shares are samples, not cycle measurements.\nFixed 10 Hz sampling may alias with 60 Hz output.", va="top", fontsize=10)

    hot = fig.add_subplot(grid[2, :])
    functions = ["engine_raster", "blit", "smg_game_render_ui", "geometry_collect_leaves_c", "project_to_screen", "engine_emit_clipped", "cpumon_common"]
    y = np.arange(len(functions))
    for index, name in enumerate(names):
        values = {row["function"]: row["percent"] for row in reports[name]["hotspots"]}
        hot.barh(y + (index - .5) * .35, [values.get(fn, 0) for fn in functions], .35, label=name, color=colors[name])
    hot.set(yticks=y, yticklabels=functions, xlabel="PC snapshots (%)", title="Selected routine residency")
    hot.invert_yaxis()
    hot.grid(axis="x", alpha=.2)
    hot.legend()
    fig.suptitle("SMG runtime: real STM32H7B0 versus default GWemu", fontsize=15)
    fig.savefig(folder / "comparison.svg")
    fig.savefig(folder / "comparison.png", dpi=160)
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("folder", type=Path)
    args = parser.parse_args()
    reports, comparison = analyze(args.folder)
    plot(args.folder, reports, comparison)
    print(json.dumps({name: {key: comparison[name][key] for key in ("wall_fps", "game_ms_per_frame", "composite_ms_per_frame")} for name in ("hardware", "vm")}, indent=2))


if __name__ == "__main__":
    main()
