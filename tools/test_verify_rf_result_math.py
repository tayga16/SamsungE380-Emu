"""Synthetic ARM arithmetic tests; no firmware or Unicorn required."""
import unittest

import verify_rf_result_math as rf


class RfResultMathTests(unittest.TestCase):
    def test_division_truncates_toward_zero(self):
        self.assertEqual([rf.trunc_div(v, 4) for v in range(-5, 6)],
                         [-1, -1, 0, 0, 0, 0, 0, 0, 0, 1, 1])

    def test_observed_correction_does_not_make_zero_no_signal(self):
        self.assertEqual(rf.correct_result(0, 0xff74), -140)
        self.assertEqual(rf.encode_rxlev(-140), 40)
        self.assertEqual(rf.correct_result(-320, 0xff74), -220)
        self.assertEqual(rf.encode_rxlev(-220), 0)

    def test_signed_inputs_and_wrapping_unsigned_correction(self):
        self.assertEqual(rf.signed_halfword(0xffff), -1)
        self.assertEqual(rf.correct_result(0xffff, 0), 0)
        self.assertEqual(rf.correct_result(0x7fff, 0x7fff), -24578)
        self.assertEqual(rf.correct_result(0x8000, 0x8000), 24576)

    def test_empty_air_in_observed_gain_profiles(self):
        for correction, expected in ((0xff74, -240), (0xff75, -239), (0xff60, -260),
                                     (0xff61, -259), (0xff5d, -263)):
            with self.subTest(correction=correction):
                corrected = rf.correct_result(-400, correction)
                self.assertEqual(corrected, expected)
                self.assertEqual(rf.clamp_corrected(corrected), -220)
                self.assertEqual(rf.encode_rxlev(rf.clamp_corrected(corrected)), 0)

    def test_half_db_boundaries_and_truncation(self):
        self.assertEqual([rf.encode_rxlev(v) for v in (-221, -220, -219, -218)],
                         [0, 0, 1, 1])
        self.assertEqual([rf.encode_rxlev(v) for v in (-96, -95, -94, -93)],
                         [62, 63, 63, 63])

    def test_unavailable_is_an_arm_buffer_marker_not_a_dsp_value(self):
        self.assertEqual([rf.encode_rxlev(v) for v in (509, 510, 511, 512)],
                         [63, 255, 255, 63])
        self.assertEqual(rf.encode_rxlev(0xffff), 63)

    def test_numeric_clamp_does_not_preserve_unavailable_marker(self):
        self.assertEqual([rf.clamp_corrected(v) for v in (-32768, -220, 100, 32767)],
                         [-220, -220, 100, 100])
        self.assertEqual(rf.clamp_corrected(510), 100)

    def test_wrong_firmware_rejected_before_execution(self):
        with self.assertRaisesRegex(ValueError, "do not match"):
            rf.checked_slices(b"")


if __name__ == "__main__":
    unittest.main()
