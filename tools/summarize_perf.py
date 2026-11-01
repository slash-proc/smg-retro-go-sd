#!/usr/bin/env python3
"""Summarize SMG runtime captures, retaining workload and frame-budget context."""
import argparse
import json
from pathlib import Path


def summarize(folder):
    rows = []
    for path in sorted(folder.glob("*.json")):
        report = json.loads(path.read_text())
        if "samples" not in report or "wall_fps" not in report:
            continue
        if any("Rejected timing comparison" in warning for warning in report.get("warnings", [])):
            continue
        contexts = [report["context_before"], *(r["values"] for r in report["contexts"])]
        one_second = []
        previous_frames = report["baseline"]["frames"]
        previous_time = 0
        for sample in report["samples"][9::10]:
            one_second.append((sample["frames"] - previous_frames) / (sample["elapsed_s"] - previous_time))
            previous_frames, previous_time = sample["frames"], sample["elapsed_s"]
        rows.append({"file": path.name, "label": report["label"],
            "fps": report["wall_fps"], "duration_s": report["duration_s"],
            "one_second_fps_min": min(one_second) if one_second else None,
            "mean_work_ms": report["game_ms_per_frame"] + report["composite_ms_per_frame"],
            "stages": sorted(set(c["g_game.stage_index"] for c in contexts)),
            "scenes": sorted(set(c["g_game.scene"] for c in contexts)),
            "player_positions": len({tuple(c["g_game.player.figure.transform.o"].values()) for c in contexts}),
            "simulation_hz": (contexts[-1]["g_game.tick"] - contexts[0]["g_game.tick"]) / report["duration_s"],
            "final_faults": report["final_faults"],
            "instrumentation": report.get("instrumentation")})
    (folder / "summary.json").write_text(json.dumps(rows, indent=2) + "\n")
    return rows


def plot(folder, rows):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    fig, axes = plt.subplots(2, 1, figsize=(11, 8), constrained_layout=True)
    selected = [r for r in rows if r["label"].startswith("hardware")]
    axes[0].barh([r["label"] for r in selected], [r["mean_work_ms"] for r in selected], color="#0072B2")
    axes[0].axvline(1000/60, color="#D55E00", linestyle="--", label="60 FPS budget")
    axes[0].axvline(1000/30, color="#009E73", linestyle="--", label="30 FPS budget")
    axes[0].set(xlabel="Mean measured work per rendered frame (ms)", title="Real STM32H7B0: frame work")
    axes[0].invert_yaxis()
    axes[0].legend()
    for name in ("hardware-c-60", "hardware-c-moving-60", "hardware-c-moving-30"):
        path = folder / (name + ".json")
        if not path.exists():
            continue
        r = json.loads(path.read_text())
        prev_t, prev_f, xs, ys = 0, r["baseline"]["frames"], [], []
        for sample in r["samples"][9::10]:
            xs.append(sample["elapsed_s"])
            ys.append((sample["frames"]-prev_f)/(sample["elapsed_s"]-prev_t))
            prev_t, prev_f = sample["elapsed_s"], sample["frames"]
        axes[1].plot(xs, ys, label=name)
    axes[1].axhline(30, color="gray", linestyle="--", linewidth=.8)
    axes[1].set(xlabel="Wall time (s)", ylabel="Rendered FPS, one-second windows", ylim=(0, 70))
    axes[1].grid(alpha=.2)
    axes[1].legend()
    fig.suptitle("SMG performance round — 160×120 world, 320×240 HUD/output")
    for extension in ("png", "svg"):
        fig.savefig(folder / ("optimization." + extension), dpi=150)
    plt.close(fig)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("folder", type=Path)
    parser.add_argument("--plot", action="store_true")
    args = parser.parse_args()
    rows = summarize(args.folder)
    if args.plot:
        plot(args.folder, rows)
    for row in rows:
        print(f"{row['label']}: {row['fps']:.2f} FPS, {row['mean_work_ms']:.2f} ms work, stages={row['stages']}, scenes={row['scenes']}")
