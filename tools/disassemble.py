import argparse
from pathlib import Path
import sys
import re

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / '.tools/analysis'))
from capstone import Cs, CS_ARCH_ARM, CS_MODE_ARM, CS_MODE_THUMB, CS_MODE_LITTLE_ENDIAN

parser = argparse.ArgumentParser()
parser.add_argument('image', type=Path)
parser.add_argument('addresses', nargs='+', type=lambda value: int(value, 0))
parser.add_argument('--arm', action='store_true')
parser.add_argument('--bytes', type=int, default=256)
parser.add_argument('--base', type=lambda value: int(value, 0), default=0x20000000)
args = parser.parse_args()
content = args.image.read_bytes()
engine = Cs(CS_ARCH_ARM, (CS_MODE_ARM if args.arm else CS_MODE_THUMB) | CS_MODE_LITTLE_ENDIAN)
for address in args.addresses:
    offset = address - args.base
    print(f'\nAddress 0x{address:08x}')
    for instruction in engine.disasm(content[offset:offset + args.bytes], address):
        annotation = ''
        literal = re.search(r'\[pc(?:, #(-?(?:0x[0-9a-f]+|[0-9]+)))?\]', instruction.op_str)
        if instruction.mnemonic == 'ldr' and literal:
            displacement = int(literal.group(1) or '0', 0)
            pc = instruction.address + (8 if args.arm else 4)
            literal_address = (pc & ~3) + displacement
            literal_offset = literal_address - args.base
            if 0 <= literal_offset <= len(content) - 4:
                value = int.from_bytes(content[literal_offset:literal_offset + 4], 'little')
                annotation = f' ; [0x{literal_address:08x}] = 0x{value:08x}'
        print(f'0x{instruction.address:08x} {instruction.mnemonic:10} {instruction.op_str}{annotation}')
