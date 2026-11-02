"""Synthetic DSP-upload audit tests; no firmware or DSP execution required."""
from dataclasses import replace
import hashlib
import unittest
from unittest.mock import patch

import inspect_dsp_upload as dsp


class DspUploadTests(unittest.TestCase):
    def block(self, space=1, start=3, data=b"\x11\x22\x33\x44"):
        return dsp.UploadBlock(space, dsp.CLA_BASE, start, len(data) // 2,
                               hashlib.sha256(data).hexdigest(), dsp.fnv64(data))

    def snapshot(self, block, data):
        result = bytearray(dsp.SNAPSHOT_SIZE)
        start = block.word_start * 2
        result[start:start + len(data)] = data
        return bytes(result)

    def test_known_fnv_vectors(self):
        self.assertEqual(dsp.fnv64(b""), 0xcbf29ce484222325)
        self.assertEqual(dsp.fnv64(b"hello"), 0xa430d84680aabd0b)

    def test_word_addresses_are_not_byte_offsets(self):
        self.assertEqual(dsp.word_window(b"\x11\x22\x33\x44", 1, 1), b"\x33\x44")

    def test_invalid_windows_rejected(self):
        for data, start, count in ((b"a", 0, 1), (b"abcd", -1, 1),
                                   (b"abcd", 0, 0), (b"abcd", 1, 2)):
            with self.subTest(start=start, count=count), self.assertRaises(ValueError):
                dsp.word_window(data, start, count)

    def test_loader_write_preserves_original_and_neighbour_words(self):
        original = b"\x11\x22\x33\x44\x55\x66"
        self.assertEqual(dsp.replace_loader_word(original, 1, 0x4433, 0x8877),
                         b"\x11\x22\x77\x88\x55\x66")
        self.assertEqual(original, b"\x11\x22\x33\x44\x55\x66")

    def test_loader_write_requires_exact_source_word(self):
        with self.assertRaisesRegex(ValueError, "source does not match"):
            dsp.replace_loader_word(b"\x11\x22", 0, 0x1122, 0)
        with self.assertRaisesRegex(ValueError, "fit a halfword"):
            dsp.replace_loader_word(b"\x11\x22", 0, 0x2211, 0x10000)

    def test_source_fingerprint_checked(self):
        block = self.block()
        with patch.object(dsp, "BLOCKS", (block,)):
            self.assertEqual(dsp.checked_upload(b"\x11\x22\x33\x44"),
                             {1: b"\x11\x22\x33\x44"})
            with self.assertRaisesRegex(ValueError, "source block does not match"):
                dsp.checked_upload(b"\x11\x22\x33\x45")

    def test_reconstructed_fingerprint_checked(self):
        block = replace(self.block(), uploaded_fnv64=0)
        with patch.object(dsp, "BLOCKS", (block,)):
            with self.assertRaisesRegex(ValueError, "upload hash mismatch"):
                dsp.checked_upload(b"\x11\x22\x33\x44")

    def test_complete_space0_reference_uses_loader_write_without_mutating_image(self):
        source = bytes(0x28) + b"\xce\xfa\x11\x22"
        expected = bytes(0x28) + b"\x34\x46\x11\x22"
        block = replace(self.block(space=0, start=0, data=source),
                        uploaded_fnv64=dsp.fnv64(expected))
        with patch.object(dsp, "BLOCKS", (block,)):
            self.assertEqual(dsp.checked_upload(source), {0: expected})
        self.assertEqual(source[0x28:], b"\xce\xfa\x11\x22")

    def test_exact_snapshot_and_nonzero_word_start(self):
        data = b"\x11\x22\x33\x44"
        block = self.block(start=0x7000, data=data)
        dsp.compare_snapshot(self.snapshot(block, data), block, data)

    def test_snapshot_size_mismatch_rejected(self):
        with self.assertRaisesRegex(ValueError, "CLI snapshot"):
            dsp.compare_snapshot(b"", self.block(), b"\x11\x22\x33\x44")

    def test_snapshot_reports_word_address_of_first_mismatch(self):
        block = self.block(start=0x7000)
        with self.assertRaisesRegex(ValueError, "word 0x7001: expected 4433, found 0033"):
            dsp.compare_snapshot(self.snapshot(block, b"\x11\x22\x33\x00"),
                                 block, b"\x11\x22\x33\x44")

    def test_reference_size_mismatch_rejected(self):
        with self.assertRaisesRegex(ValueError, "reference size"):
            dsp.compare_snapshot(bytes(dsp.SNAPSHOT_SIZE), self.block(), b"")

    def test_other_uploads_rejected_before_and_after_block(self):
        block = self.block()
        for address in (0, dsp.SNAPSHOT_SIZE - 1):
            data = bytearray(self.snapshot(block, b"\x11\x22\x33\x44"))
            data[address] = 1
            with self.subTest(address=address), self.assertRaisesRegex(ValueError, "outside"):
                dsp.compare_snapshot(bytes(data), block, b"\x11\x22\x33\x44")

    def test_raw_output_has_word_addresses_and_no_instruction_labels(self):
        self.assertEqual(dsp.format_raw_words(b"\x11\x22\x33\x44", 1, 1),
                         ["word 0x0001: 4433"])


if __name__ == "__main__":
    unittest.main()
