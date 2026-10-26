"""Find ARMv5 ARM/Thumb direct branch references, including interworking calls."""
import argparse
from pathlib import Path
import struct

parser = argparse.ArgumentParser()
parser.add_argument('image', type=Path)
parser.add_argument('targets', nargs='+', type=lambda value: int(value, 0))
parser.add_argument('--base', type=lambda value: int(value, 0), default=0x20000000)
args = parser.parse_args()
content = args.image.read_bytes()
targets = {target & ~1 for target in args.targets}
for offset in range(0, len(content) - 3, 2):
    first, second = struct.unpack_from('<HH', content, offset)
    address = args.base + offset
    if first & 0xf800 == 0xf000 and second & 0xf800 in (0xe800, 0xf800):
        displacement = ((first & 0x7ff) << 12) | ((second & 0x7ff) << 1)
        if displacement & (1 << 22): displacement -= 1 << 23
        destination = (address + 4 + displacement) & 0xffffffff
        if second & 0xf800 == 0xe800: destination &= ~3
        if destination in targets:
            print(f'0x{address:08x} Thumb {"BLX" if second & 0xf800 == 0xe800 else "BL"} -> 0x{destination:08x}')
    if offset % 4 == 0:
        word = first | (second << 16)
        if word & 0x0e000000 == 0x0a000000:
            displacement = (word & 0xffffff) << 2
            if displacement & (1 << 25): displacement -= 1 << 26
            destination = (address + 8 + displacement) & 0xffffffff
            if destination in targets:
                print(f'0x{address:08x} ARM B/BL -> 0x{destination:08x}')
