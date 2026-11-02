import argparse
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / ".tools/analysis"))
from capstone import Cs, CS_ARCH_ARM, CS_MODE_LITTLE_ENDIAN, CS_MODE_THUMB


def parse_address(value: str) -> int:
    return int(value, 0)


def normalize(instruction) -> str:
    mnemonic = instruction.mnemonic
    operands = instruction.op_str
    if mnemonic.startswith("b"):
        return mnemonic
    if mnemonic in {"ldr", "ldrb", "ldrh", "str", "strb", "strh"}:
        offsets = ",".join(re.findall(r"#-?(?:0x[0-9a-f]+|[0-9]+)", operands))
        return f"{mnemonic}:{offsets}"
    if mnemonic in {"push", "pop"}:
        return mnemonic
    if mnemonic in {"cmp", "movs", "adds", "subs", "lsls", "lsrs", "muls"}:
        immediate = re.search(r"#-?(?:0x[0-9a-f]+|[0-9]+)", operands)
        return f"{mnemonic}:{immediate.group(0) if immediate else 'reg'}"
    return mnemonic


parser = argparse.ArgumentParser(
    description="Find firmware windows resembling a normalized Thumb sequence."
)
parser.add_argument("reference", type=Path)
parser.add_argument("reference_address", type=parse_address)
parser.add_argument("image", type=Path)
parser.add_argument("--reference-base", type=parse_address, default=0x10000000)
parser.add_argument("--image-base", type=parse_address, default=0x20000000)
parser.add_argument("--instructions", type=int, default=48)
parser.add_argument("--limit", type=int, default=30)
args = parser.parse_args()

engine = Cs(CS_ARCH_ARM, CS_MODE_THUMB | CS_MODE_LITTLE_ENDIAN)
reference_data = args.reference.read_bytes()
reference_offset = args.reference_address - args.reference_base
reference_instructions = list(
    engine.disasm(reference_data[reference_offset : reference_offset + 512], args.reference_address)
)[: args.instructions]
reference = [normalize(instruction) for instruction in reference_instructions]

image_data = args.image.read_bytes()
results = []
window_size = len(reference)
for offset in range(0, len(image_data) - 512, 2):
    halfword = int.from_bytes(image_data[offset : offset + 2], "little")
    if halfword & 0xFE00 != 0xB400:
        continue
    instructions = list(
        engine.disasm(image_data[offset : offset + 512], args.image_base + offset)
    )[:window_size]
    if len(instructions) != window_size:
        continue
    window = [normalize(instruction) for instruction in instructions]
    exact = sum(left == right for left, right in zip(reference, window))
    bigrams = sum(
        reference[position : position + 2] == window[position : position + 2]
        for position in range(window_size - 1)
    )
    results.append((bigrams * 3 + exact, bigrams, exact, instructions[0].address))

for score, bigrams, exact, address in sorted(results, reverse=True)[: args.limit]:
    print(f"0x{address:08x} score={score} bigrams={bigrams} positional={exact}")
