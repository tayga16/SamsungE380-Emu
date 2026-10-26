"""Show non-erased NAND pages/blocks without dumping user data."""
import argparse
from pathlib import Path
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("image", type=Path)
parser.add_argument("--skip", type=lambda s: int(s, 0), default=0)
args = parser.parse_args()
data = args.image.read_bytes()[args.skip:]
for start in range(0, len(data), 0x20000):
    block = data[start:start + 0x20000]
    pages = [p for p in range(0, len(block), 2048) if any(b != 255 for b in block[p:p+2048])]
    print(f"block {start//0x20000:3}: {len(pages):2} used pages, first {pages[0]//2048 if pages else '-'}")
    if pages:
        offset = start + pages[0]
        print(f"  first-page signature at 0x{offset:x}: {data[offset:offset+8].hex()}")
