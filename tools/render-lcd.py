"""Convert a saved LCD PPM into a PNG for inspection."""
import argparse
from pathlib import Path
import re
import struct
import zlib

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('image', type=Path)
parser.add_argument('--scale', type=int, default=1)
args = parser.parse_args()
if args.scale < 1:
    parser.error('--scale must be positive')
data = args.image.read_bytes()
header = re.match(rb'P6\s+(\d+)\s+(\d+)\s+255\s', data)
if not header:
    parser.error('expected the RGB P6 PPM produced by e830-cli')
width, height = map(int, header.groups())
pixels = data[header.end():]
if len(pixels) != width * height * 3:
    parser.error('pixel length does not match the PPM header')
rows = bytearray()
for y in range(height):
    row = pixels[y * width * 3:(y + 1) * width * 3]
    enlarged = b''.join(row[x:x + 3] * args.scale for x in range(0, len(row), 3))
    rows.extend((b'\0' + enlarged) * args.scale)
def chunk(kind, body):
    return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
png = b'\x89PNG\r\n\x1a\n'
png += chunk(b'IHDR', struct.pack('>IIBBBBB', width * args.scale, height * args.scale, 8, 2, 0, 0, 0))
png += chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b'')
args.image.with_suffix('.png').write_bytes(png)
print(args.image.with_suffix('.png'))
