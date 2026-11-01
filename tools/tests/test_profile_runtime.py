import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from profile_runtime import _perf_summary


def snapshot(frame, *, index_base=0):
    ring = [0] * 256
    for i in range(max(0, frame - 256), frame):
        ring[(index_base + i) & 255] = 10000 if i == 20 else 100
    return {"frames": frame, "target_fps": 60, "cycles": [0] * 9,
            "work_cycles": frame * 100 + (9900 if frame > 20 else 0),
            "max_work_cycles": 10000 if frame > 20 else 100,
            "over_30": 0, "over_60": 0, "histogram": [frame] + [0] * 15,
            "ring_index": (index_base + frame) & 0xffffffff,
            "recent_work_cycles": ring}


class FrameHistoryTests(unittest.TestCase):
    def test_full_window_keeps_early_spike_after_it_leaves_final_ring(self):
        result = _perf_summary(snapshot(0), snapshot(600), 1000000,
                               [snapshot(i) for i in range(100, 601, 100)])
        self.assertEqual(result["observed_work_frames"], 600)
        self.assertEqual(result["unobserved_work_frames"], 0)
        self.assertEqual(result["observed_work_ms_max"], 10)
        self.assertEqual(result["recent_work_ms_max"], .1)

    def test_ring_counter_wrap(self):
        result = _perf_summary(snapshot(0, index_base=0xfffffffc),
                               snapshot(12, index_base=0xfffffffc), 1000000)
        self.assertEqual(result["observed_work_frames"], 12)
        self.assertEqual(result["unobserved_work_frames"], 0)

    def test_missing_snapshots_report_lost_history(self):
        result = _perf_summary(snapshot(0), snapshot(600), 1000000)
        self.assertEqual(result["observed_work_frames"], 256)
        self.assertEqual(result["unobserved_work_frames"], 344)

    def test_vm_without_dwt_has_no_cycle_claim(self):
        before, after = snapshot(0), snapshot(60)
        before["work_cycles"] = after["work_cycles"] = 0
        self.assertFalse(_perf_summary(before, after, 1000000)["available"])


if __name__ == "__main__":
    unittest.main()
