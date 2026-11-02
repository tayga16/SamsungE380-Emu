"""Synthetic packer-contract tests, without firmware or boot-state injection."""
import unittest

import verify_rf_command as command


class RfCommandTests(unittest.TestCase):
    def test_captured_band8_profile0333_and_tail_residue(self):
        self.assertEqual(command.expected_words(512, 8, 0x333), (0x0c010800, 0x06670000))
        self.assertEqual(command.expected_words(516, 8, 0x333), (0x0c090800, 0x06670000))
        self.assertEqual(command.expected_words(517, 8, 0x333), (0x0c0b0800, 0x06670000))
        self.assertEqual(command.expected_words(521, 8, 0x333), (0x0c130800, 0x06670000))
        self.assertEqual(command.expected_words(885, 4, 0x33f), (0x06eb0800, 0x067f0000))

    def test_observed_channel522_residue_and_restart_first_word(self):
        self.assertEqual(command.expected_words(522, 4, 0x333), (0x04150800, 0x06670000))
        self.assertEqual(command.expected_words(523, 4, 0x333), (0x04170800, 0x06670000))

    def test_channel_mask_and_band_tag_are_independent(self):
        self.assertEqual(command.expected_words(1024, 2, 0x335), (0x10800, 0x066b0000))
        self.assertEqual(command.expected_words(0, 8, 0x335), (0x08010800, 0x066b0000))

    def test_original_band_adjustment_boundary(self):
        self.assertEqual(command.expected_words(0, 4, 7)[1], 0x10000)
        self.assertEqual(command.expected_words(0, 4, 9)[1], 0x30000)
        self.assertEqual(command.expected_words(0, 2, 9)[1], 0x130000)

    def test_observed_band2_profile033f_and_previous_band4_command(self):
        self.assertEqual(command.expected_words(0, 2, 0x33f), (0x00010800, 0x067f0000))
        self.assertEqual(command.expected_words(4, 2, 0x33f), (0x00090800, 0x067f0000))
        self.assertEqual(command.expected_words(885, 4, 0x333), (0x06eb0800, 0x06670000))

    def test_fingerprint_rejects_missing_or_unknown_code(self):
        for image in (b"", bytes(0x98d854 + command.CODE_SIZE)):
            with self.assertRaisesRegex(ValueError, "do not match"):
                command.checked_code(image)

    def test_observed_band4_profile033d_in_mixed_and_next_group(self):
        self.assertEqual(command.expected_words(512, 4, 0x33d), (0x04010800, 0x067b0000))
        self.assertEqual(command.expected_words(513, 4, 0x33d), (0x04030800, 0x067b0000))
        self.assertEqual(command.expected_words(517, 4, 0x33d), (0x040b0800, 0x067b0000))

    def test_observed_band4_profile033f_restart_and_previous_command(self):
        self.assertEqual(command.expected_words(522, 4, 0x33f), (0x04150800, 0x067f0000))
        self.assertEqual(command.expected_words(523, 4, 0x33f), (0x04170800, 0x067f0000))
        self.assertEqual(command.expected_words(527, 4, 0x33f), (0x041f0800, 0x067f0000))


if __name__ == "__main__":
    unittest.main()
