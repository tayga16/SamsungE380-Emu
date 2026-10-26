import argparse
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('image', type=Path)
parser.add_argument('--base', type=lambda value: int(value, 0), default=0x30000000)
parser.add_argument('addresses', nargs='+', type=lambda value: int(value, 0))
parser.add_argument('--bytes', type=int, default=64)
args = parser.parse_args()
data = args.image.read_bytes()
for address in args.addresses:
    print(f'\n0x{address:08x}')
    offset = address - args.base
    for start in range(offset, min(len(data), offset + args.bytes), 16):
        row = data[start:start + 16]
        words = ' '.join(f'{int.from_bytes(row[at:at + 4], "little"):08x}' for at in range(0, len(row), 4))
        ascii_text = ''.join(chr(byte) if 32 <= byte < 127 else '.' for byte in row)
        print(f'0x{args.base + start:08x}: {words:36} {ascii_text}')
