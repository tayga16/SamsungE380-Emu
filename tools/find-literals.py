"""Locate aligned firmware words for a reproducible reverse-engineering probe."""
import argparse
from pathlib import Path
import struct

parser = argparse.ArgumentParser()
parser.add_argument('image', type=Path)
parser.add_argument('values', nargs='+', type=lambda value: int(value, 0))
parser.add_argument('--base', type=lambda value: int(value, 0), default=0x20000000)
args = parser.parse_args()
content = args.image.read_bytes()
for value in args.values:
    needle = struct.pack('<I', value)
    start = 0
    print(f'Word 0x{value:08x}')
    while (offset := content.find(needle, start)) != -1:
        print(f'  0x{args.base + offset:08x} (file +0x{offset:x})')
        start = offset + 1
