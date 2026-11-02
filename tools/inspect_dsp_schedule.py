"""Read the ARM-written DSP schedule from CLI --peek output; never execute it.

The decoded fields follow 2098d5c8 and 2098d854 in E830XEGG1. Class and
opcode are raw timing-table bytes, not identified DSP instructions. Times
are unscaled schedule values; this tool does not infer clock units, RF power,
completion flags or a result from a stored request.
"""
import argparse
from dataclasses import dataclass
from pathlib import Path
import re


MASK_ADDRESS = 0x60000658
RECORD_ADDRESS = 0x6000065c
COMMAND_ADDRESS = 0x600006bc
MAX_RECORDS = 16  # ARM producer checks its count against 0x10.


def parse_peeks(text):
    words = {}
    for match in re.finditer(r"^peek 0x([0-9a-fA-F]+): Some\((\d+)\)\s*$", text, re.M):
        address, value = int(match[1], 16), int(match[2])
        if address & 3 or not 0 <= value <= 0xffffffff:
            raise ValueError("expected aligned 32-bit peeks")
        if address in words and words[address] != value:
            raise ValueError(f"conflicting peeks at 0x{address:08x}")
        words[address] = value
    return words


def halfword(words, address):
    if address & 1:
        raise ValueError("halfword address must be aligned")
    try:
        word = words[address & ~3]
    except KeyError:
        raise ValueError(f"missing --peek 0x{address & ~3:08x}") from None
    return (word >> (16 if address & 2 else 0)) & 0xffff


@dataclass(frozen=True)
class Record:
    index: int
    slot: int
    table_class: int
    opcode: int
    time_raw: int
    argument_raw: int
    header_raw: int


@dataclass(frozen=True)
class PackedCommand:
    slot: int
    channel: int
    band_tag_set: bool
    calibration_code: int


def decode_schedule(words):
    mask = halfword(words, MASK_ADDRESS) | halfword(words, MASK_ADDRESS + 2) << 16
    if mask >> MAX_RECORDS:
        raise ValueError("mask exceeds the observed 16-record ARM producer")
    records = []
    for index in range(MAX_RECORDS):
        if not mask & (1 << index):
            continue  # Inactive records can retain stale data from a previous frame.
        address = RECORD_ADDRESS + index * 6
        header = halfword(words, address)
        records.append(Record(index, header >> 13, (header >> 8) & 0x1f,
                              header & 0xff, halfword(words, address + 2),
                              halfword(words, address + 4), header))
    return mask, records


def decode_packed_command(words, slot):
    if not 0 <= slot <= 7:
        raise ValueError("slot must fit the three-bit schedule field")
    address = COMMAND_ADDRESS + slot * 0x28
    first = halfword(words, address) | halfword(words, address + 2) << 16
    second = halfword(words, address + 4) | halfword(words, address + 6) << 16
    # Only recognize the exact two-word packer shape; other operations share
    # the slot storage. Do not label every nonzero command as operation 3.
    if first & 0x7ff or second & 0x7ff:
        return None
    first >>= 11
    second >>= 11
    if first & 0x3f != 0x21 or first >> 17 or second & 0x3f != 0x20 or second >> 16:
        return None
    return PackedCommand(slot, (first >> 6) & 0x3ff, bool(first & 0x10000), second >> 6)


def format_schedule(words):
    mask, records = decode_schedule(words)
    lines = [f"Active schedule mask: 0x{mask:08x}; records: {len(records)}"]
    if not records:
        lines.append("No active records; any stored record/command bytes may be stale.")
    for record in records:
        lines.append(f"{record.index:2}: slot={record.slot} class=0x{record.table_class:02x} "
                     f"opcode=0x{record.opcode:02x} time_raw={record.time_raw} "
                     f"argument_raw=0x{record.argument_raw:04x} header=0x{record.header_raw:04x}")
    for slot in sorted({record.slot for record in records}):
        address = COMMAND_ADDRESS + slot * 0x28
        if address not in words or address + 4 not in words:
            lines.append(f"slot {slot}: command unavailable (peek 0x{address:08x}, 0x{address + 4:08x})")
            continue
        command = decode_packed_command(words, slot)
        if command is None:
            lines.append(f"slot {slot}: command does not match the 2098d854 packer")
        else:
            lines.append(f"slot {slot}: channel={command.channel} band_tag_set={command.band_tag_set} "
                         f"calibration_code=0x{command.calibration_code:04x}")
    lines.append("Stored requests only: DSP execution/result delivery is not established by this report.")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="CLI report containing aligned --peek values")
    args = parser.parse_args()
    try:
        print(format_schedule(parse_peeks(args.report.read_text(encoding="utf-8"))))
    except ValueError as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
