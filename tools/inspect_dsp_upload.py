"""Audit the original E830 DSP upload without executing or modifying it.

The original ARM loader selects these blocks after signature 0305 and writes
one word in space 0 before starting the DSP. This tool reconstructs that
observed upload in memory, fingerprints it, and optionally compares native CLI
DSP snapshots. It is NOT a DSP disassembler or an RF-result generator. Space
numbers are boot-protocol identifiers, not established program/X/Y mappings.
No firmware bytes, output dumps, results, flags or IRQs are generated on disk.
"""
import argparse
from dataclasses import dataclass
import hashlib
from pathlib import Path


CLA_BASE = 0x20000000
SNAPSHOT_SIZE = 0x20000  # CLI exports 65536 little-endian halfwords per space.
MASK64 = (1 << 64) - 1


@dataclass(frozen=True)
class UploadBlock:
    space: int
    source: int
    word_start: int
    word_count: int
    source_sha256: str
    uploaded_fnv64: int


# Addresses and fingerprints only; no original DSP instruction bytes.
BLOCKS = (
    UploadBlock(0, 0x20bf90be, 0, 0x6000,
                "5aec3489d2ac7a3ea09ff895f1bd80183be12c05f94f16b5568b7c3bd16b2c64",
                0xb25c3385c96d670d),
    UploadBlock(1, 0x20c050be, 0x7000, 0x1000,
                "731b92a449ea9752447d47ebf492e7e192b79bcb49a8dd3166dff5432400c1ea",
                0xe39e195e7cdc9922),
)


def fnv64(data):
    value = 0xcbf29ce484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001b3) & MASK64
    return value


def word_window(data, word_start, word_count):
    if len(data) & 1:
        raise ValueError("halfword data has an odd byte count")
    if word_start < 0 or word_count <= 0 or word_start + word_count > len(data) // 2:
        raise ValueError("word window is outside the supplied memory")
    return data[word_start * 2:(word_start + word_count) * 2]


def replace_loader_word(data, word_address, expected, replacement):
    """Apply an established ARM-loader write only to a local reference copy."""
    original = int.from_bytes(word_window(data, word_address, 1), "little")
    if original != expected:
        raise ValueError("loader-word source does not match the established upload")
    if not 0 <= replacement <= 0xffff:
        raise ValueError("loader replacement must fit a halfword")
    result = bytearray(data)
    result[word_address * 2:word_address * 2 + 2] = replacement.to_bytes(2, "little")
    return bytes(result)


def checked_upload(image):
    result = {}
    for block in BLOCKS:
        offset = block.source - CLA_BASE
        data = image[offset:offset + block.word_count * 2]
        if hashlib.sha256(data).hexdigest() != block.source_sha256:
            raise ValueError(f"space {block.space}: source block does not match E830XEGG1")
        if block.space == 0:
            # 2098c95e/c9a2/c9e6/ca2c send command1, address14, count1, 4634.
            # This is original guest behavior, not a change to the CLA file.
            data = replace_loader_word(data, 0x14, 0xface, 0x4634)
        if fnv64(data) != block.uploaded_fnv64:
            raise ValueError(f"space {block.space}: reconstructed upload hash mismatch")
        result[block.space] = data
    return result


def compare_snapshot(snapshot, block, expected):
    if len(snapshot) != SNAPSHOT_SIZE:
        raise ValueError(f"space {block.space}: expected a {SNAPSHOT_SIZE}-byte CLI snapshot")
    if len(expected) != block.word_count * 2:
        raise ValueError("reference size does not match the upload block")
    start, end = block.word_start * 2, (block.word_start + block.word_count) * 2
    actual = snapshot[start:end]
    if actual != expected:
        for offset in range(0, len(actual), 2):
            if actual[offset:offset + 2] != expected[offset:offset + 2]:
                address = block.word_start + offset // 2
                wanted = int.from_bytes(expected[offset:offset + 2], "little")
                found = int.from_bytes(actual[offset:offset + 2], "little")
                raise ValueError(f"space {block.space}: word 0x{address:04x}: "
                                 f"expected {wanted:04x}, found {found:04x}; "
                                 "check the capture stage and loader writes")
    # A full CLI snapshot zero-fills words never uploaded. Check that the image
    # has not acquired other uploads; no inference about DSP memory mapping.
    if any(snapshot[:start]) or any(snapshot[end:]):
        raise ValueError(f"space {block.space}: nonzero data outside the established upload")


def format_raw_words(data, start, count):
    window = word_window(data, start, count)
    lines = []
    for offset in range(0, len(window), 16):
        words = [int.from_bytes(window[i:i + 2], "little")
                 for i in range(offset, min(offset + 16, len(window)), 2)]
        lines.append(f"word 0x{start + offset // 2:04x}: " +
                     " ".join(f"{word:04x}" for word in words))
    return lines


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path, help="original local E830XEGG1 CLA")
    parser.add_argument("--space0", type=Path, help="optional native --dump-dsp-space0 snapshot")
    parser.add_argument("--space1", type=Path, help="optional native --dump-dsp-space1 snapshot")
    parser.add_argument("--words", type=lambda value: int(value, 0),
                        help="optional DSP word address (not an ARM byte address)")
    parser.add_argument("--space", type=int, choices=(0, 1), default=0)
    parser.add_argument("--count", type=lambda value: int(value, 0), default=16)
    args = parser.parse_args()
    try:
        uploads = checked_upload(args.image.read_bytes())
        lines = []
        for block, path in zip(BLOCKS, (args.space0, args.space1)):
            data = uploads[block.space]
            if path is not None:
                compare_snapshot(path.read_bytes(), block, data)
            lines.append(f"space {block.space}: CLA 0x{block.source:08x}; "
                         f"word start 0x{block.word_start:04x}; count 0x{block.word_count:04x}; "
                         f"FNV64 {fnv64(data):016x}; "
                         + ("snapshot matches" if path else "reference only; no snapshot supplied"))
        lines.append("Original ARM-loader write: space0 word 0x0014, face -> 4634, before DSP start.")
        if args.words is not None:
            block = BLOCKS[args.space]
            # Present word addresses in their protocol space, including the
            # zero-filled unuploaded part, not relative to the CLA block.
            memory = bytes(block.word_start * 2) + uploads[args.space]
            memory += bytes(SNAPSHOT_SIZE - len(memory))
            lines.extend(format_raw_words(memory, args.words, args.count))
        lines.append("Upload identity only: no ISA decoding, DSP execution or RF completion is established.")
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print("\n".join(lines))


if __name__ == "__main__":
    main()
