"""Synthetic control-contract tests; do not boot or inject RF measurements."""
import hashlib
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

import verify_rf_collector as collector


class RfCollectorTests(unittest.TestCase):
    def test_zero_flags_do_not_ack(self):
        result = collector.expected_control(0, 0xa580)
        self.assertEqual(result.mask, 0xa580)
        self.assertEqual(result.acknowledgements, ())
        self.assertEqual(result.event_words, (0x1703, 0xff, 0, 0x1234))
        self.assertEqual(result.timer_reads, 1)

    def test_upper_flags_are_ignored(self):
        self.assertEqual(collector.expected_control(0xff00, 0x1234),
                         collector.expected_control(0, 0x1234))

    def test_index_seven_flag_ack_and_second_timer(self):
        result = collector.expected_control(0xab80, 0x4001, 0x1111, 0x2222)
        self.assertEqual(result.mask, 0x4081)
        self.assertEqual(result.acknowledgements, (0xff7f,))
        self.assertEqual(result.event_words, (0x1703, 0xff, 0x80, 0x1111,
                                            0x1783, 0x80, 0x2222))
        self.assertEqual(result.timer_reads, 2)

    def test_all_low_masks_accumulate_without_clearing_old_bits(self):
        for flags in range(256):
            result = collector.expected_control(flags, 0xa580)
            self.assertEqual(result.mask, 0xa580 | flags)
            self.assertEqual(result.acknowledgements,
                             ((~flags & 0xffff,) if flags else ()))

    def test_halfword_bounds_rejected(self):
        for flags, previous in ((-1, 0), (0x10000, 0), (0, -1), (0, 0x10000)):
            with self.subTest(flags=flags, previous=previous), self.assertRaises(ValueError):
                collector.expected_control(flags, previous)

    def test_word_codec_preserves_unsigned_halfwords(self):
        words = (0, 1, 0x7fff, 0x8000, 0xffff)
        self.assertEqual(collector.unpack_words(collector.pack_words(words)), words)

    def test_firmware_fingerprint_and_literal_bytes_checked(self):
        code = bytes(range(collector.CODE_SIZE))
        image = bytes(collector.CODE_ADDRESS - 0x20000000) + code
        with patch.object(collector, "CODE_SHA256", hashlib.sha256(code).hexdigest()):
            self.assertEqual(collector.checked_code(image), code)
            with self.assertRaisesRegex(ValueError, "do not match"):
                collector.checked_code(image[:-1] + b"\xff")
            with self.assertRaisesRegex(ValueError, "do not match"):
                collector.checked_code(image[:-1])

    def test_wrong_image_rejected_before_cpu_import(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / "empty.cla"
            path.touch()
            with patch.dict("sys.modules", {"unicorn": None}):
                with self.assertRaisesRegex(ValueError, "do not match"):
                    collector.verify(path)


if __name__ == "__main__":
    unittest.main()
