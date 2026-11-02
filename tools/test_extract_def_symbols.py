import importlib.util
from pathlib import Path
import tempfile
import unittest


MODULE_PATH = Path(__file__).with_name("extract_def_symbols.py")
SPEC = importlib.util.spec_from_file_location("extract_def_symbols", MODULE_PATH)
symbols = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(symbols)


class ExtractDefSymbolsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = Path(self.temp.name) / "sample.def"
        self.path.write_text(
            """/* symbols
a_hkbd_Idle  0x20a8e874
a_hkbd_Init 0x20e82eac
s_other_State 0x21000000
#define NOT_AN_ADDRESS_SYMBOL 0x2203
a_hkbd_Idle 0x20a8e874
*/
""",
            encoding="utf-8",
        )

    def tearDown(self):
        self.temp.cleanup()

    def test_parser_preserves_duplicate_symbols_and_source_lines(self):
        parsed = list(symbols.read_symbols(self.path))
        self.assertEqual([item["name"] for item in parsed], [
            "a_hkbd_Idle",
            "a_hkbd_Init",
            "s_other_State",
            "a_hkbd_Idle",
        ])
        self.assertEqual(parsed[0]["address"], 0x20A8E874)
        self.assertEqual(parsed[-1]["line"], 6)

    def test_filters_can_be_combined(self):
        selected = list(
            symbols.select_symbols(
                symbols.read_symbols(self.path),
                ["a_hkbd_"],
                "init|idle",
                0x20B00000,
                0x20FFFFFF,
            )
        )
        self.assertEqual([(item["name"], item["address"]) for item in selected], [
            ("a_hkbd_Init", 0x20E82EAC),
        ])


if __name__ == "__main__":
    unittest.main()
