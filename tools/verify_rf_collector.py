"""Check the ARM RF-result collector in an isolated synthetic harness.

Runs fingerprinted original ARM instructions, NOT a phone boot or DSP model.
All masks, timer values and payloads below are synthetic test inputs. They
are never injected into the emulator. No result, dump or firmware is written.
This checks the consumer interface; it cannot establish DSP opcodes, timing,
measurement generation or the value of an empty RF channel.
"""
import argparse
from dataclasses import dataclass
import hashlib
from pathlib import Path
import sys


CODE_ADDRESS = 0x205769f4
CODE_SIZE = 0x90  # Entire function and its six literal words.
CODE_SHA256 = "9d1d64d5e17269f984c3660ee303fd7724b472d70da74f51a5b1afb2857704b9"
FLAGS = 0x5050501c
TIMER = 0x50501000
MASK = 0x3049198c
SHARED_RESULTS = 0x600000dc
SHARED_AUXILIARY = 0x600000fc
RESULTS = 0x304919a0
AUXILIARY = 0x304919b0
TRACE_POINTER = 0x3001a684
TRACE_BUFFER = 0x30002000
RETURN_ADDRESS = 0x30000f00
STACK = 0x30010000


def checked_code(image):
    offset = CODE_ADDRESS - 0x20000000
    code = image[offset:offset + CODE_SIZE]
    if len(code) != CODE_SIZE or hashlib.sha256(code).hexdigest() != CODE_SHA256:
        raise ValueError("collector instructions do not match local E830XEGG1")
    return code


def pack_words(words):
    return b"".join(word.to_bytes(2, "little") for word in words)


def unpack_words(data):
    return tuple(int.from_bytes(data[i:i + 2], "little")
                 for i in range(0, len(data), 2))


@dataclass(frozen=True)
class Expected:
    mask: int
    acknowledgements: tuple
    event_words: tuple
    timer_reads: int


def expected_control(flags, previous_mask, first_timer=0x1234, second_timer=0x5678):
    if not 0 <= flags <= 0xffff or not 0 <= previous_mask <= 0xffff:
        raise ValueError("flags and previous mask must fit halfwords")
    low = flags & 0xff
    event_words = (0x1703, 0xff, low, first_timer)
    if low:
        event_words += (0x1783, low, second_timer)
    return Expected(previous_mask | low, ((~low & 0xffff,) if low else ()),
                    event_words, 2 if low else 1)


def verify(image_path):
    code = checked_code(image_path.read_bytes())
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / ".tools/analysis"))
    from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
    from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC

    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    for address, size in ((CODE_ADDRESS & ~0xfff, 0x1000),
                          (0x30000000, 0x11000), (0x3001a000, 0x1000),
                          (0x30491000, 0x1000),
                          (0x50501000, 0x1000), (0x50505000, 0x1000),
                          (0x60000000, 0x1000)):
        cpu.mem_map(address, size)
    cpu.mem_write(CODE_ADDRESS, code)
    timers = (0x1234, 0x5678)
    state = {"flags": 0, "timer_reads": 0, "accesses": [], "acks": []}

    def read_hook(uc, access, address, size, value, user_data):
        if address == FLAGS:
            if size != 2:
                raise ValueError("collector result flags are not a halfword read")
            uc.mem_write(address, pack_words((state["flags"],)))
            state["accesses"].append(("flags", address))
        elif address == TIMER:
            index = state["timer_reads"]
            if size != 2 or index >= len(timers):
                raise ValueError("unexpected timer read")
            uc.mem_write(address, pack_words((timers[index],)))
            state["timer_reads"] += 1
            state["accesses"].append(("timer", address))
        elif SHARED_RESULTS <= address < SHARED_RESULTS + 16:
            state["accesses"].append(("payload", address))
        elif SHARED_AUXILIARY <= address < SHARED_AUXILIARY + 60:
            state["accesses"].append(("payload", address))

    def write_hook(uc, access, address, size, value, user_data):
        if address == FLAGS:
            if size != 2:
                raise ValueError("collector ACK is not a halfword write")
            state["acks"].append(value & 0xffff)
            state["accesses"].append(("ack", address))
        elif address == MASK:
            state["accesses"].append(("mask", address))

    cpu.hook_add(UC_HOOK_MEM_READ, read_hook)
    cpu.hook_add(UC_HOOK_MEM_WRITE, write_hook)

    # All low-byte masks against both empty and populated ARM accumulated state.
    cases = [(flags, previous) for flags in range(256) for previous in (0, 0xa580)]
    # Upper bits alone must not be ACKed or added to the accumulated mask.
    cases += [(flags, previous) for flags in (0x100, 0xff00, 0xffff, 0xab80)
              for previous in (0, 0xffff)]
    for number, (flags, previous) in enumerate(cases):
        state.update(flags=flags, timer_reads=0, accesses=[], acks=[])
        expected = expected_control(flags, previous, *timers)
        # Vary every word on every call, including sign and halfword boundaries.
        results = tuple((number * 977 + i * 8191) & 0xffff for i in range(8))
        auxiliary = tuple((number * 1237 + i * 4093) & 0xffff for i in range(30))
        cpu.mem_write(SHARED_RESULTS, pack_words(results))
        cpu.mem_write(SHARED_AUXILIARY, pack_words(auxiliary))
        cpu.mem_write(MASK, pack_words((previous,)))
        cpu.mem_write(RESULTS - 2, bytes([0xa5]) * 80)
        cpu.mem_write(TRACE_BUFFER - 2, bytes([0xa5]) * 20)
        cpu.mem_write(TRACE_POINTER, TRACE_BUFFER.to_bytes(4, "little"))
        cpu.reg_write(UC_ARM_REG_SP, STACK)
        cpu.reg_write(UC_ARM_REG_LR, RETURN_ADDRESS | 1)
        cpu.emu_start(CODE_ADDRESS | 1, RETURN_ADDRESS, count=400)
        if cpu.reg_read(UC_ARM_REG_PC) != RETURN_ADDRESS:
            raise ValueError("original collector did not return within its instruction budget")
        actual_mask = unpack_words(cpu.mem_read(MASK, 2))[0]
        end = int.from_bytes(cpu.mem_read(TRACE_POINTER, 4), "little")
        trace_size = len(expected.event_words) * 2
        if end != TRACE_BUFFER + trace_size:
            raise ValueError(f"case {number}: event-stream pointer mismatch")
        actual_trace = unpack_words(cpu.mem_read(TRACE_BUFFER, trace_size))
        if (actual_mask, tuple(state["acks"]), actual_trace, state["timer_reads"]) != (
                expected.mask, expected.acknowledgements, expected.event_words, expected.timer_reads):
            raise ValueError(f"case {number}: flags, accumulated mask, ACK or event-stream mismatch")
        if unpack_words(cpu.mem_read(RESULTS, 16)) != results:
            raise ValueError(f"case {number}: eight result words not copied verbatim")
        if unpack_words(cpu.mem_read(AUXILIARY, 60)) != auxiliary:
            raise ValueError(f"case {number}: thirty auxiliary words not copied verbatim")
        if (unpack_words(cpu.mem_read(SHARED_RESULTS, 16)) != results
                or unpack_words(cpu.mem_read(SHARED_AUXILIARY, 60)) != auxiliary):
            raise ValueError(f"case {number}: collector modified a shared source payload")
        if cpu.reg_read(UC_ARM_REG_SP) != STACK:
            raise ValueError(f"case {number}: collector did not restore the stack pointer")
        for address in (RESULTS - 2, AUXILIARY + 60,
                        TRACE_BUFFER - 2, TRACE_BUFFER + trace_size):
            if bytes(cpu.mem_read(address, 2)) != b"\xa5\xa5":
                raise ValueError(f"case {number}: write past an established buffer boundary")
        expected_accesses = [("flags", FLAGS), ("timer", TIMER), ("mask", MASK)]
        if flags & 0xff:
            expected_accesses += [("ack", FLAGS), ("timer", TIMER)]
        expected_accesses += [("payload", SHARED_RESULTS + 2 * i) for i in range(8)]
        expected_accesses += [("payload", SHARED_AUXILIARY + 2 * i) for i in range(30)]
        if state["accesses"] != expected_accesses:
            raise ValueError(f"case {number}: mask/ACK/payload access order mismatch")
    return len(cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path, help="unchanged local E830XEGG1 CLA")
    args = parser.parse_args()
    try:
        count = verify(args.image)
    except (OSError, ValueError, ImportError) as error:
        parser.error(str(error))
    print(f"Verified {count} synthetic cases against the original ARM RF collector.")
    print("Only low eight flags accumulate; nonzero low flags receive a complement ACK.")
    print("ACK precedes the unconditional verbatim copy of 8 result and 30 auxiliary words.")
    print("Consumer interface only: no DSP execution, RF result or phone boot is supplied.")


if __name__ == "__main__":
    main()
