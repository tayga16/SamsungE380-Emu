import unittest

import inspect_dsp_schedule as dsp
import inspect_rf_memory_trace as ram


def access(source, operation, address, size, value, instructions, pc=0x20143dea):
    return (f"[e830-{source}] {operation} addr=0x{address:08x} size={size} "
            f"value=0x{value:08x} pc=0x{pc:08x} insns={instructions}\n")


def store(address, size, value, instructions=1):
    return access("ram", "write", address, size, value, instructions)


def control(instructions=2):
    return access("dsp-io", "write", 0x50505000, 2, 0x40, instructions, 0x205747f2)


class RfMemoryTraceTests(unittest.TestCase):
    def test_partial_and_overlapping_stores_are_little_endian(self):
        trace = ram.parse_trace([store(dsp.MASK_ADDRESS, 4, 0x11223344),
                                 store(dsp.MASK_ADDRESS + 1, 2, 0xaabb), control()])
        self.assertEqual(trace.snapshots[0].words(), {dsp.MASK_ADDRESS: 0x11aabb44})

    def test_unknown_bytes_are_not_zero_or_decoded_as_idle(self):
        snapshot = ram.parse_trace([store(dsp.MASK_ADDRESS, 2, 0), control()]).snapshots[0]
        self.assertNotIn(dsp.MASK_ADDRESS, snapshot.words())
        self.assertIn("Incomplete/unsupported", ram.format_snapshot(snapshot))
        self.assertNotIn("No active records", ram.format_snapshot(snapshot))

    def test_control_snapshot_precedes_later_clear_and_reuse(self):
        trace = ram.parse_trace([store(dsp.MASK_ADDRESS, 4, 0x3ff), control(),
                                 store(dsp.MASK_ADDRESS, 4, 0, 3), control(4),
                                 store(dsp.MASK_ADDRESS, 4, 1, 5), control(6)])
        self.assertEqual([snapshot.words()[dsp.MASK_ADDRESS] for snapshot in trace.snapshots],
                         [0x3ff, 0, 1])
        self.assertIn("may be stale", ram.format_snapshot(trace.snapshots[1]))

    def test_missing_active_record_is_explicit(self):
        snapshot = ram.parse_trace([store(dsp.MASK_ADDRESS, 4, 1), control()]).snapshots[0]
        self.assertIn("missing --peek 0x6000065c", ram.format_snapshot(snapshot))

    def test_band_tagged0333_command_and_zero_extension(self):
        address = dsp.COMMAND_ADDRESS + 3 * 40
        lines = [store(dsp.MASK_ADDRESS, 4, 1), store(dsp.RECORD_ADDRESS, 4, 37 << 16 | 0x6500),
                 store(dsp.RECORD_ADDRESS + 4, 4, 0), store(address, 4, 0x0c130800),
                 store(address + 4, 4, 0x06670000)]
        lines += [store(at, 8, 0) for at in range(address + 8, address + 40, 8)]
        snapshot = ram.parse_trace(lines + [control()]).snapshots[0]
        result = ram.format_snapshot(snapshot)
        self.assertIn("channel=521 band_tag_set=True calibration_code=0x0333", result)
        self.assertIn("0c130800/06670000", result)
        self.assertIn("known=32/32; all zero", result)
        self.assertIsNone(snapshot.accepted_slots)
        partial = ram.parse_trace(lines[:5] + [control()]).snapshots[0]
        self.assertIn("unknown bytes, not proven zero", ram.format_snapshot(partial))

    def test_submit_is_only_attached_to_same_control_timestamp(self):
        trace = ram.parse_trace([control(5), "[e830-dsp-rf-hle] submit empty-air slots=5 time=5 phase_clocks=0\n",
                                 control(10), "[e830-dsp-rf-hle] submit empty-air slots=3 time=11 phase_clocks=0\n"])
        self.assertEqual([snapshot.accepted_slots for snapshot in trace.snapshots], [5, None])

    def test_flags_reads_and_unrelated_io_do_not_submit(self):
        trace = ram.parse_trace([access("dsp-io", "write", 0x50505000, 2, 1, 1),
                                 access("dsp-io", "read", 0x5050501c, 2, 0xf8, 2),
                                 access("dsp-io", "write", 0x50505000, 4, 0x40, 3)])
        self.assertEqual(trace.snapshots, [])
        self.assertEqual(trace.flag_reads, [(2, 0x20143dea, 2, 0xf8)])

    def test_window_overlap_is_clipped_without_inventing_bytes(self):
        snapshot = ram.parse_trace([store(dsp.MASK_ADDRESS - 2, 4, 0x12345678),
                                   store(ram.WINDOW_END - 1, 2, 0xabcd), control()]).snapshots[0]
        self.assertEqual(snapshot.memory, {dsp.MASK_ADDRESS: 0x34, dsp.MASK_ADDRESS + 1: 0x12,
                                          ram.WINDOW_END - 1: 0xcd})

    def test_independent_peeks_compare_only_known_bytes_and_fail_mismatch(self):
        snapshot = ram.parse_trace([store(dsp.MASK_ADDRESS, 2, 0x1234), control()]).snapshots[0]
        self.assertEqual(ram.compare_peeks(snapshot, {dsp.MASK_ADDRESS: 0xffff1234}), 2)
        with self.assertRaisesRegex(ValueError, "mismatch"):
            ram.compare_peeks(snapshot, {dsp.MASK_ADDRESS: 0xffff1235})
        with self.assertRaisesRegex(ValueError, "no known"):
            ram.compare_peeks(snapshot, {dsp.COMMAND_ADDRESS: 0})

    def test_bad_accesses_and_backwards_timestamps_fail(self):
        for lines in ([store(dsp.MASK_ADDRESS, 3, 0)], [store(dsp.MASK_ADDRESS, 1, 0x100)],
                      [control(3), store(dsp.MASK_ADDRESS, 4, 0, 2)],
                      ["[e830-ram] truncated\n"], ["[e830-dsp-io] truncated\n"]):
            with self.subTest(lines=lines), self.assertRaises(ValueError):
                ram.parse_trace(lines)


if __name__ == "__main__":
    unittest.main()
