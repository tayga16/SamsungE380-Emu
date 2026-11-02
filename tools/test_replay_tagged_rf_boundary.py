import unittest

import inspect_dsp_schedule as dsp
import inspect_rf_memory_trace as ram
import replay_tagged_rf_boundary as replay


class ActiveFixtureTests(unittest.TestCase):
    def snapshot(self):
        memory = {}
        for offset, byte in enumerate((3).to_bytes(4, "little")):
            memory[dsp.MASK_ADDRESS + offset] = byte
        for index, header in enumerate((0xe500, 0xe20a)):
            record = dsp.RECORD_ADDRESS + index * 6
            for offset, byte in enumerate(header.to_bytes(2, "little") + bytes(4)):
                memory[record + offset] = byte
        command = dsp.COMMAND_ADDRESS + 7 * 40
        memory.update({address: 0 for address in range(command, command + 40)})
        return ram.Snapshot(1, 0x205747f2, memory, {})

    def test_only_unknown_inactive_bytes_receive_fillers(self):
        snapshot = self.snapshot()
        first = replay.active_fixture(snapshot, 0xa5)
        second = replay.active_fixture(snapshot, 0x5a)
        self.assertEqual(len(first), ram.WINDOW_END - dsp.MASK_ADDRESS)
        for address, byte in snapshot.memory.items():
            self.assertEqual(first[address - dsp.MASK_ADDRESS], byte)
            self.assertEqual(second[address - dsp.MASK_ADDRESS], byte)
        inactive = dsp.COMMAND_ADDRESS - dsp.MASK_ADDRESS
        self.assertEqual((first[inactive], second[inactive]), (0xa5, 0x5a))

    def test_missing_active_extension_byte_is_not_inferred_zero(self):
        snapshot = self.snapshot()
        del snapshot.memory[dsp.COMMAND_ADDRESS + 7 * 40 + 39]
        with self.assertRaisesRegex(ValueError, "uncaptured active byte"):
            replay.active_fixture(snapshot, 0xa5)

    def test_missing_active_record_is_not_fabricated(self):
        snapshot = self.snapshot()
        del snapshot.memory[dsp.RECORD_ADDRESS + 5]
        with self.assertRaisesRegex(ValueError, "uncaptured active byte"):
            replay.active_fixture(snapshot, 0x5a)

    def test_partial_mask_is_not_treated_as_known(self):
        snapshot = self.snapshot()
        del snapshot.memory[dsp.MASK_ADDRESS + 3]
        with self.assertRaises(KeyError):
            replay.active_fixture(snapshot, 0xa5)


if __name__ == "__main__":
    unittest.main()
