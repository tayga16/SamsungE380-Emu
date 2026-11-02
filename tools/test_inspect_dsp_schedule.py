"""Synthetic tests: no firmware, NAND, RAM dumps or emulator state needed."""
import unittest

import inspect_dsp_schedule as dsp


def store_halfword(words, address, value):
    shift = 16 if address & 2 else 0
    aligned = address & ~3
    words[aligned] = (words.get(aligned, 0) & ~(0xffff << shift)) | value << shift


class DspScheduleTests(unittest.TestCase):
    def test_parses_aligned_decimal_peeks_and_ignores_report_fields(self):
        words = dsp.parse_peeks("pc: 42,\npeek 0x60000658: Some(1023)\npeek 0x6000065c: None\n")
        self.assertEqual(words, {0x60000658: 1023})

    def test_rejects_misaligned_overflow_and_conflicting_peeks(self):
        for text in ("peek 0x60000659: Some(1)", "peek 0x60000658: Some(4294967296)",
                     "peek 0x60000658: Some(1)\npeek 0x60000658: Some(2)"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                dsp.parse_peeks(text)

    def test_decodes_observed_ten_record_shape_without_mutating_words(self):
        words = {dsp.MASK_ADDRESS: 0x03ff}
        expected = []
        for index in range(10):
            slot = 7 - index // 2
            header = slot << 13 | (0x20a if index & 1 else 0x500)
            time = 37 + (index // 2) * 910 + (499 if index & 1 else 0)
            address = dsp.RECORD_ADDRESS + index * 6
            for at, value in enumerate((header, time, 0)):
                store_halfword(words, address + at * 2, value)
            expected.append((slot, time))
        before = words.copy()
        mask, records = dsp.decode_schedule(words)
        self.assertEqual(mask, 0x03ff)
        self.assertEqual([(r.slot, r.time_raw) for r in records], expected)
        self.assertEqual([(r.table_class, r.opcode) for r in records], [(5, 0), (2, 10)] * 5)
        self.assertEqual(words, before)

    def test_cleared_mask_does_not_decode_stale_records_or_commands(self):
        words = {dsp.MASK_ADDRESS: 0, dsp.RECORD_ADDRESS: 0x0025e500,
                 dsp.COMMAND_ADDRESS + 7 * 0x28: 0x00010800}
        self.assertEqual(dsp.decode_schedule(words), (0, []))
        self.assertIn("may be stale", dsp.format_schedule(words))
        self.assertNotIn("channel=", dsp.format_schedule(words))

    def test_missing_active_halfword_and_unsupported_mask_fail_explicitly(self):
        with self.assertRaisesRegex(ValueError, "missing --peek"):
            dsp.decode_schedule({dsp.MASK_ADDRESS: 1})
        with self.assertRaisesRegex(ValueError, "16-record"):
            dsp.decode_schedule({dsp.MASK_ADDRESS: 1 << 16})

    def test_unaligned_record_boundary_uses_correct_half_of_each_word(self):
        words = {dsp.MASK_ADDRESS: 2, 0x60000660: 0xe20adead, 0x60000664: 0x12340218}
        record = dsp.decode_schedule(words)[1][0]
        self.assertEqual((record.slot, record.time_raw, record.argument_raw), (7, 536, 0x1234))

    def test_packer_channels_are_not_upper_halfword_values(self):
        for channel, tagged in ((0, False), (4, False), (9, False), (1023, True)):
            address = dsp.COMMAND_ADDRESS + 7 * 0x28
            words = {address: ((0x10000 if tagged else 0) | channel << 6 | 0x21) << 11,
                     address + 4: (0x335 << 6 | 0x20) << 11}
            command = dsp.decode_packed_command(words, 7)
            self.assertEqual((command.channel, command.band_tag_set, command.calibration_code),
                             (channel, tagged, 0x335))

    def test_other_command_shapes_are_not_identified_as_measurements(self):
        words = {dsp.COMMAND_ADDRESS: 0x1800, dsp.COMMAND_ADDRESS + 4: 1}
        self.assertIsNone(dsp.decode_packed_command(words, 0))
        with self.assertRaises(ValueError):
            dsp.decode_packed_command(words, 8)

    def test_profile0333_exact_active_command_words(self):
        address = dsp.COMMAND_ADDRESS + 3 * 0x28
        command = dsp.decode_packed_command({address: 67176448,
                                             address + 4: 107413504}, 3)
        self.assertEqual(command, dsp.PackedCommand(3, 512, False, 0x333))
        # 06668000 was a documentation transcription error, not the wire
        # word in the decimal CLI peeks; it has a different packer opcode.
        self.assertIsNone(dsp.decode_packed_command({address: 0x04010800,
                                                     address + 4: 0x06668000}, 3))


if __name__ == "__main__":
    unittest.main()
