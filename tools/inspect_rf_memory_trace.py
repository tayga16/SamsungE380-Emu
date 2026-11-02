"""Reconstruct RF producer memory at actual control writes, offline only.

Reads bounded E830_TRACE_RAM_* and E830_TRACE_DSP_IO logs. Unknown bytes stay
unknown: a late-started or exhausted trace is not a zero-filled RAM snapshot.
No firmware, emulator state or completion flags are modified by this tool.
"""
import argparse
from dataclasses import dataclass, field
from pathlib import Path
import re

import inspect_dsp_schedule as dsp


WINDOW_END = dsp.COMMAND_ADDRESS + 8 * 40
ACCESS = re.compile(
    r"\[e830-(ram|dsp-io)\] (write|read) addr=0x([0-9a-fA-F]+) "
    r"size=(\d+) value=0x([0-9a-fA-F]+) pc=0x([0-9a-fA-F]+) insns=(\d+)"
)
SUBMIT = re.compile(r"\[e830-dsp-rf-hle\] submit empty-air slots=(\d+) time=(\d+)\b")


@dataclass
class Snapshot:
    instructions: int
    pc: int
    memory: dict
    writers: dict
    accepted_slots: int | None = None

    def words(self):
        # Reuse the existing ARM schedule decoder only for fully known words.
        return {address: int.from_bytes(bytes(self.memory[address + i] for i in range(4)), "little")
                for address in range(dsp.MASK_ADDRESS, WINDOW_END, 4)
                if all(address + i in self.memory for i in range(4))}


@dataclass
class Trace:
    snapshots: list = field(default_factory=list)
    flag_reads: list = field(default_factory=list)


def parse_trace(lines):
    memory, writers = {}, {}
    trace = Trace()
    last_instructions = -1
    for line_number, line in enumerate(lines, 1):
        match = ACCESS.fullmatch(line.strip())
        if match:
            source, operation = match[1], match[2]
            address, size, value, pc, instructions = (int(match[3], 16), int(match[4]),
                                                     int(match[5], 16), int(match[6], 16),
                                                     int(match[7]))
            if size not in (1, 2, 4, 8) or value >= 1 << (8 * size):
                raise ValueError(f"line {line_number}: invalid access size/value")
            if instructions < last_instructions:
                raise ValueError(f"line {line_number}: access timestamps go backwards")
            last_instructions = instructions
            if source == "ram" and operation == "write":
                for offset, byte in enumerate(value.to_bytes(size, "little")):
                    at = address + offset
                    if dsp.MASK_ADDRESS <= at < WINDOW_END:
                        memory[at] = byte
                        writers[at] = (pc, instructions)
            elif source == "dsp-io":
                if operation == "write" and address == 0x50505000 and size == 2 and value == 0x40:
                    trace.snapshots.append(Snapshot(instructions, pc, memory.copy(), writers.copy()))
                elif operation == "read" and address == 0x5050501c:
                    trace.flag_reads.append((instructions, pc, size, value))
        elif line.startswith(("[e830-ram]", "[e830-dsp-io]")):
            raise ValueError(f"line {line_number}: malformed memory/access trace")
        else:
            submit = SUBMIT.match(line)
            if submit and trace.snapshots and trace.snapshots[-1].instructions == int(submit[2]):
                trace.snapshots[-1].accepted_slots = int(submit[1])
    return trace


def format_snapshot(snapshot):
    lines = [f"RF control insns={snapshot.instructions} pc=0x{snapshot.pc:08x}",
             f"Known window bytes: {len(snapshot.memory)}/{WINDOW_END - dsp.MASK_ADDRESS}"]
    if snapshot.accepted_slots is None:
        lines.append("HLE submit log: no matching entry (not alone proof of rejection)")
    else:
        lines.append(f"HLE submit log: accepted_slots={snapshot.accepted_slots}")
    words = snapshot.words()
    try:
        mask, records = dsp.decode_schedule(words)
        lines.append(dsp.format_schedule(words))
    except ValueError as error:
        lines.append(f"Incomplete/unsupported snapshot: {error}")
        return "\n".join(lines)
    if not mask:
        return "\n".join(lines)
    for slot in sorted({record.slot for record in records}):
        address = dsp.COMMAND_ADDRESS + slot * 40
        if address in words and address + 4 in words:
            lines.append(f"slot {slot} raw: {words[address]:08x}/{words[address + 4]:08x}")
        known = [(at, snapshot.memory[at]) for at in range(address + 8, address + 40)
                 if at in snapshot.memory]
        nonzero = " ".join(f"+{at - address:02x}={value:02x}" for at, value in known if value)
        lines.append(f"slot {slot} extension: known={len(known)}/32; "
                     + (f"nonzero {nonzero}" if nonzero else
                        "all zero" if len(known) == 32 else "unknown bytes, not proven zero"))
        command_writers = [snapshot.writers[at] for at in range(address, address + 40)
                           if at in snapshot.writers]
        if command_writers:
            pc, instructions = max(command_writers, key=lambda writer: writer[1])
            lines.append(f"slot {slot} last captured store: pc=0x{pc:08x} insns={instructions}")
    return "\n".join(lines)


def compare_peeks(snapshot, words):
    compared = 0
    for address, value in words.items():
        if not dsp.MASK_ADDRESS <= address < WINDOW_END:
            continue
        for offset, byte in enumerate(value.to_bytes(4, "little")):
            at = address + offset
            if at in snapshot.memory:
                if snapshot.memory[at] != byte:
                    raise ValueError(f"peek mismatch at 0x{at:08x}: "
                                     f"trace={snapshot.memory[at]:02x} peek={byte:02x}")
                compared += 1
    if not compared:
        raise ValueError("no known RF bytes overlap the supplied peeks")
    return compared


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--after", type=int, default=0, help="filter output, retain earlier write history")
    parser.add_argument("--before", type=int, default=2**64 - 1)
    parser.add_argument("--active-only", action="store_true", help="hide known zero-mask controls only")
    parser.add_argument("--compare-peeks", type=Path, help="independent CLI snapshot report")
    parser.add_argument("--compare-at", type=int, help="exact control timestamp for comparison")
    args = parser.parse_args()
    if (args.compare_peeks is None) != (args.compare_at is None):
        parser.error("--compare-peeks and --compare-at must be supplied together")
    try:
        with args.trace.open(encoding="utf-8") as source:
            trace = parse_trace(source)
        if args.compare_peeks is not None:
            matches = [snapshot for snapshot in trace.snapshots if snapshot.instructions == args.compare_at]
            if len(matches) != 1:
                raise ValueError("comparison needs exactly one control at --compare-at")
            count = compare_peeks(matches[0], dsp.parse_peeks(args.compare_peeks.read_text(encoding="utf-8")))
            print(f"Independent peeks match {count} captured bytes at insns={args.compare_at}; "
                  "uncaptured bytes remain unknown.\n")
        for snapshot in trace.snapshots:
            if not args.after <= snapshot.instructions <= args.before:
                continue
            if args.active_only and snapshot.words().get(dsp.MASK_ADDRESS) == 0:
                continue
            print(format_snapshot(snapshot), "\n")
        for instructions, pc, size, value in trace.flag_reads:
            if args.after <= instructions <= args.before:
                print(f"Result flags read: insns={instructions} pc=0x{pc:08x} size={size} value=0x{value:08x}")
        print("Write traces may omit initial RAM or exceed their limit. No absent byte is inferred as zero.\n"
              "Flags are raw chronological reads, not correlated to a producer generation by this tool.")
    except (OSError, ValueError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
