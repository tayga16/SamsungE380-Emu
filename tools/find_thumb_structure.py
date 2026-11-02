import argparse
from collections import Counter
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / ".tools/analysis"))
from capstone import Cs, CS_ARCH_ARM, CS_MODE_LITTLE_ENDIAN, CS_MODE_THUMB


def parse_address(value: str) -> int:
    return int(value, 0)


def shape(instruction) -> str:
    mnemonic = instruction.mnemonic
    operands = instruction.op_str
    immediates = re.findall(r"#-?(?:0x[0-9a-f]+|[0-9]+)", operands)
    if mnemonic in {"cmp", "cmn", "tst", "subs", "adds", "movs", "lsls", "lsrs"}:
        return f"{mnemonic}:{','.join(immediates)}"
    if mnemonic.startswith("b"):
        return mnemonic
    if mnemonic in {"push", "pop"}:
        return f"{mnemonic}:{len(re.findall(r'r(?:1[0-5]|[0-9])|lr|pc', operands))}"
    if mnemonic in {"ldr", "ldrb", "ldrh", "str", "strb", "strh"}:
        return f"{mnemonic}:{','.join(immediates)}"
    return mnemonic


parser = argparse.ArgumentParser(
    description="Rank Thumb function candidates by normalized instruction structure."
)
parser.add_argument("reference", type=Path)
parser.add_argument("reference_address", type=parse_address)
parser.add_argument("image", type=Path)
parser.add_argument("--reference-base", type=parse_address, default=0x10000000)
parser.add_argument("--image-base", type=parse_address, default=0x20000000)
parser.add_argument("--bytes", type=parse_address, default=0x370)
parser.add_argument("--limit", type=int, default=30)
args = parser.parse_args()

engine = Cs(CS_ARCH_ARM, CS_MODE_THUMB | CS_MODE_LITTLE_ENDIAN)
reference_data = args.reference.read_bytes()
reference_offset = args.reference_address - args.reference_base
reference_instructions = list(
    engine.disasm(
        reference_data[reference_offset : reference_offset + args.bytes],
        args.reference_address,
    )
)
reference_shapes = [shape(instruction) for instruction in reference_instructions]
reference_counts = Counter(reference_shapes)
reference_bigrams = Counter(zip(reference_shapes, reference_shapes[1:]))

image_data = args.image.read_bytes()
reference_push = reference_data[reference_offset : reference_offset + 2]
candidates = []
offset = image_data.find(reference_push)
while offset >= 0:
    address = args.image_base + offset
    instructions = list(engine.disasm(image_data[offset : offset + args.bytes], address))
    if len(instructions) >= 24:
        shapes = [shape(instruction) for instruction in instructions]
        counts = Counter(shapes)
        bigrams = Counter(zip(shapes, shapes[1:]))
        unigram_score = sum((reference_counts & counts).values())
        bigram_score = sum((reference_bigrams & bigrams).values())
        compared = min(len(reference_shapes), len(shapes))
        positional_score = sum(
            left == right
            for left, right in zip(reference_shapes[:compared], shapes[:compared])
        )
        score = bigram_score * 4 + positional_score * 2 + unigram_score
        candidates.append(
            (score, bigram_score, positional_score, unigram_score, address, len(instructions))
        )
    offset = image_data.find(reference_push, offset + 2)

for score, bigrams, positional, unigrams, address, count in sorted(
    candidates, reverse=True
)[: args.limit]:
    print(
        f"0x{address:08x} score={score} bigrams={bigrams} "
        f"positional={positional} unigrams={unigrams} instructions={count}"
    )
