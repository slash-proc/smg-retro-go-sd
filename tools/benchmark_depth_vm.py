#!/usr/bin/env python3
"""Short stationary depth off/on comparison in an already running GWemu.

Uses GWProv Python bindings. Sends no game inputs; leaves depth enabled.
VM wall/guest timing is not an STM32 cycle benchmark. Camera changes invalidate
the stationary comparison and are reported rather than silently averaged.
"""
import argparse
import json
from pathlib import Path
import time
from types import SimpleNamespace

from gwprov.debug_shell import DebugSession, GwemuGDBBackend
from gwprov.qmp import QMPConnection


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("device", help="gwprov://PID")
    parser.add_argument("elf", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--port", type=int, default=1235)
    parser.add_argument("--seconds", type=float, default=5)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--startup-timeout", type=float, default=60)
    args = parser.parse_args()
    report = {"device": args.device, "elf": str(args.elf), "windows": [],
              "note": "VM timing only; no DWT cycles or STM32 performance claim"}
    with QMPConnection(args.device) as q:
        d = DebugSession(SimpleNamespace(read_memory=q.read_memory), "gwemu", qmp_socket=args.device)
        d.symbols.load(args.elf)

        def set_depth(enabled):
            backend = GwemuGDBBackend(port=args.port)
            was_running = q.execute("query-status")["return"]["running"]
            try:
                backend.open(halt=True)
                backend.write_memory(d.at("g_depth_enabled"), bytes([enabled]))
            finally:
                if backend._socket and was_running:
                    backend.resume()
                backend.close()

        def sample():
            return {"time": time.monotonic(),
                    "stats": d.read_path("smg_render_stats"),
                    "camera": d.read_path("g_game.camera"),
                    "scene": d.read_path("g_game.scene"),
                    "stage": d.read_path("g_game.stage_index"),
                    "detail": d.read_path("g_game.view_distance")}

        # ELF addresses are not game globals until the homebrew has loaded.
        # Never toggle them while the launcher (or its fault screen) is active.
        deadline = time.monotonic() + args.startup_timeout
        before = sample()
        while True:
            time.sleep(.1)
            after = sample()
            if (after["scene"] == before["scene"] == 1 and
                after["stats"]["raster_width"] == 160 and
                after["stats"]["raster_height"] == 120 and
                after["stats"]["frames"] > before["stats"]["frames"] and
                q.u32(0xE000ED28) == 0 and q.u32(0xE000ED2C) == 0):
                break
            if time.monotonic() >= deadline:
                raise RuntimeError("No advancing, fault-free gameplay; refusing to write game globals")
            before = after

        try:
            for round_number in range(args.rounds):
                for enabled in (False, True):
                    set_depth(enabled)
                    time.sleep(.25)
                    samples = [sample()]
                    deadline = samples[0]["time"] + args.seconds
                    while time.monotonic() < deadline:
                        time.sleep(min(.1, max(0, deadline-time.monotonic())))
                        samples.append(sample())
                    before, after = samples[0], samples[-1]
                    frames = after["stats"]["frames"] - before["stats"]["frames"]
                    if frames <= 0:
                        raise RuntimeError("Game frames stopped; timing window is invalid")
                    elapsed = after["time"] - before["time"]
                    stable = all(all(s[key] == before[key] for key in ("camera", "scene", "stage", "detail")) for s in samples)
                    window = {"round": round_number, "depth": enabled, "stationary": stable,
                              "frames": frames, "wall_seconds": elapsed, "fps": frames/elapsed,
                              "game_ms_per_frame": (after["stats"]["game_ms"]-before["stats"]["game_ms"])/frames if frames else None,
                              "samples": samples}
                    report["windows"].append(window)
                    args.output.write_text(json.dumps(report, indent=2)+"\n")
                    print({k:v for k,v in window.items() if k != "samples"}, flush=True)
        finally:
            set_depth(True)
        report["geometry"] = d.read_path("smg_geometry_stats")
        report["faults"] = {"CFSR": q.u32(0xE000ED28), "HFSR": q.u32(0xE000ED2C)}
        args.output.write_text(json.dumps(report, indent=2)+"\n")


if __name__ == "__main__":
    main()
