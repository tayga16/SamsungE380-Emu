"""Check extraction boundaries and rejection of unusable NAND/package inputs."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import prepare_nand as nand


CONFIG = """TFSVERSION : TFS4.0_CORONA_01
NUM_DIR : 2
MAXFILES : 3
SET_DIR
DIR_NAME : /icons
DIR_NAME : /icons/empty
END_DIR
SET_FILE
FILE_NAME : /icons/first.bin
FILE_SIZE : 3
FILE_NAME : /icons/zero.bin
FILE_SIZE : 0
FILE_NAME : /last.bin
FILE_SIZE : 4
END_FILE
END_TFS
"""


class PrepareNandTests(unittest.TestCase):
    def test_extracts_in_cfg_order_including_empty_files_and_directories(self):
        directories, resources, total = nand.parse_config(CONFIG)
        self.assertEqual(total, 7)
        self.assertEqual([item.offset for item in resources], [0, 3, 3])
        with tempfile.TemporaryDirectory() as temp:
            stage = Path(temp) / "resources"
            nand.stage_resources(stage, directories, resources, b"ABC1234", CONFIG.encode())
            self.assertEqual((stage / "files/icons/first.bin").read_bytes(), b"ABC")
            self.assertEqual((stage / "files/icons/zero.bin").read_bytes(), b"")
            self.assertEqual((stage / "files/last.bin").read_bytes(), b"1234")
            self.assertTrue((stage / "files/icons/empty").is_dir())
            manifest = json.loads((stage / "manifest.json").read_text())
            self.assertEqual(manifest["files"][-1]["sha256"], hashlib.sha256(b"1234").hexdigest())
            with self.assertRaises(FileExistsError):
                nand.stage_resources(stage, directories, resources, b"XYZ5678", CONFIG.encode())
            self.assertEqual((stage / "files/last.bin").read_bytes(), b"1234")

    def test_erased_image_has_all_main_and_spare_bytes_and_does_not_replace_a_dump(self):
        with tempfile.TemporaryDirectory() as temp:
            image = Path(temp) / "phone.onenand.bin"
            nand.create_erased_image(image)
            self.assertEqual(image.stat().st_size, 34_603_008)
            with image.open("rb") as source:
                while chunk := source.read(1024 * 1024):
                    self.assertEqual(chunk.count(0xff), len(chunk))
            image.write_bytes(b"existing dump")
            with self.assertRaises(FileExistsError):
                nand.create_erased_image(image)
            self.assertEqual(image.read_bytes(), b"existing dump")

    def test_rejects_paths_that_escape_or_alias_windows_files(self):
        for path in ("/../outside", "/icons/../outside", "/icons\\outside", "/NUL.txt",
                     "/icons/a.", "/icons/a ", "/icons/x:y", "relative", "//server"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                nand.validate_path(path)

    def test_rejects_stale_counts_duplicate_paths_and_missing_sizes(self):
        for config in (
            CONFIG.replace("MAXFILES : 3", "MAXFILES : 2"),
            CONFIG.replace("NUM_DIR : 2", "NUM_DIR : 1"),
            CONFIG.replace("/last.bin", "/ICONS/FIRST.BIN"),
            CONFIG.replace("FILE_SIZE : 4\n", ""),
            CONFIG.replace("/last.bin", "/missing/file.bin"),
            CONFIG.replace("END_TFS", ""),
        ):
            with self.subTest(config=config), self.assertRaises(ValueError):
                nand.parse_config(config)

    def test_truncated_or_extra_payload_creates_neither_image_nor_staging(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            cfg, tfs = root / "input.cfg", root / "input.tfs"
            cfg.write_text(CONFIG)
            for payload in (b"ABC", b"ABC1234EXTRA"):
                tfs.write_bytes(payload)
                result = subprocess.run([
                    sys.executable, str(Path(nand.__file__)), "--output", str(root / "nand.bin"),
                    "--cfg", str(cfg), "--tfs", str(tfs), "--stage", str(root / "staging"),
                ], capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((root / "nand.bin").exists())
                self.assertFalse((root / "staging").exists())


if __name__ == "__main__":
    unittest.main()
