"""Verify ARM-side RF arithmetic against unchanged local E830 instructions.

This is an isolated arithmetic check, not a phone boot or a DSP model. It
does not generate a DSP result, completion flag, IRQ or a no-signal value.
Requires the local E830XEGG1 CLA and Unicorn's Python bindings; neither is
copied into source. Instruction fingerprints reject other firmware versions.
"""
import argparse
import hashlib
from pathlib import Path
import sys


def signed_halfword(value):
    value &= 0xffff
    return value - 0x10000 if value & 0x8000 else value


def trunc_div(value, divisor):
    return -(abs(value) // divisor) if value < 0 else value // divisor


def correct_result(raw_halfword, correction_halfword):
    # 2098de1a..26: LDRSH result, truncate /4, LDRH correction, STRH.
    return signed_halfword(trunc_div(signed_halfword(raw_halfword), 4)
                           + (correction_halfword & 0xffff))


def clamp_corrected(value_halfword):
    # 208af8e8 -> 215b4ab8: ordinary numeric clamp, not a DSP sentinel.
    return max(-220, min(100, signed_halfword(value_halfword)))


def encode_rxlev(value_halfword):
    # 20998b40..7a: the ARM buffer recognizes 510 and, after /2, 511
    # as unavailable. This does NOT establish a DSP-side no-signal value.
    value = trunc_div(signed_halfword(value_halfword), 2)
    if value == 255:
        return 255
    return max(0, min(63, value + 110))


# Addresses, sizes and hashes only; no retail firmware bytes in this tool.
SLICES = {
    "correction": (0x2098de1a, 14,
                   "3eb195ca95cf4650a06640988b7a984d05854f1d6159aa2dbb45f93e1d263418"),
    "clamp": (0x215b4ab8, 20,
              "7e0f571b2d870c88ba9db49bdfb4564fb5b5e374aba25e85a49a6aecfd52fdee"),
    "rxlev": (0x20998b40, 60,
              "b1fe1cea96cf725d78645b8630b3c55618c69543d0bb9e183ab7c48fdd27872c"),
}


def checked_slices(image):
    result = {}
    for name, (address, size, digest) in SLICES.items():
        offset = address - 0x20000000
        code = image[offset:offset + size]
        if len(code) != size or hashlib.sha256(code).hexdigest() != digest:
            raise ValueError(f"{name}: instructions do not match local E830XEGG1")
        result[name] = code
    return result


def verify(image_path):
    # Read and fingerprint before executing anything; no boot state is used.
    code = checked_slices(image_path.read_bytes())
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / ".tools/analysis"))
    from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB
    from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R4,
                                   UC_ARM_REG_LR, UC_ARM_REG_PC)

    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    for name, (address, _, _) in SLICES.items():
        cpu.mem_map(address & ~0xfff, 0x1000)
        cpu.mem_write(address, code[name])
    scratch, output, return_address = 0x30000000, 0x30000100, 0x30000f00
    cpu.mem_map(scratch, 0x1000)
    count = 0

    def run(start, end, budget):
        cpu.emu_start(start | 1, end, count=budget)
        if cpu.reg_read(UC_ARM_REG_PC) != end:
            raise ValueError(f"instruction slice at 0x{start:08x} did not return")

    # Observed 0335, 0333/channel512, 033f/band2, 033d/band4 and
    # 033f/band4 corrections. Cover every signed16-bit DSP input with each
    # through the unchanged original consumer.
    cpu.reg_write(UC_ARM_REG_R4, scratch)
    for correction in (0xff74, 0xff75, 0xff60, 0xff61, 0xff5d):
        cpu.mem_write(scratch + 2, correction.to_bytes(2, "little"))
        for bits in range(0x10000):
            cpu.reg_write(UC_ARM_REG_R0, signed_halfword(bits) & 0xffffffff)
            run(0x2098de1a, 0x2098de28, 7)
            actual = int.from_bytes(cpu.mem_read(scratch + 8, 2), "little", signed=True)
            expected = correct_result(bits, correction)
            if actual != expected:
                raise ValueError(f"correction {correction:04x} mismatch for raw halfword 0x{bits:04x}")
            count += 1

    # Also exercise unsigned LDRH correction and 16-bit wrap at the edges.
    for correction in (0, 1, 0x7fff, 0x8000, 0xffff):
        cpu.mem_write(scratch + 2, correction.to_bytes(2, "little"))
        for bits in (0, 1, 3, 4, 0x7fff, 0x8000, 0xfffc, 0xfffd, 0xffff):
            cpu.reg_write(UC_ARM_REG_R0, signed_halfword(bits) & 0xffffffff)
            run(0x2098de1a, 0x2098de28, 7)
            actual = int.from_bytes(cpu.mem_read(scratch + 8, 2), "little", signed=True)
            if actual != correct_result(bits, correction):
                raise ValueError("correction edge/wrap mismatch")
            count += 1

    for bits in range(0x10000):
        cpu.reg_write(UC_ARM_REG_R0, signed_halfword(bits) & 0xffffffff)
        cpu.reg_write(UC_ARM_REG_LR, return_address | 1)
        run(0x215b4ab8, return_address, 12)
        actual = cpu.reg_read(UC_ARM_REG_R0) & 0xffffffff
        if actual != (clamp_corrected(bits) & 0xffffffff):
            raise ValueError(f"clamp mismatch for halfword 0x{bits:04x}")
        count += 1

        cpu.reg_write(UC_ARM_REG_R4, scratch)
        cpu.reg_write(UC_ARM_REG_R0, output)
        cpu.mem_write(scratch + 0x10, bits.to_bytes(2, "little"))
        run(0x20998b40, 0x20998b7c, 30)
        if cpu.mem_read(output + 2, 1)[0] != encode_rxlev(bits):
            raise ValueError(f"RXLEV mismatch for halfword 0x{bits:04x}")
        count += 1
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    args = parser.parse_args()
    try:
        count = verify(args.image)
    except (OSError, ValueError, ImportError) as error:
        parser.error(str(error))
    print(f"Verified {count:,} cases against unchanged ARM instruction slices.")
    corrected = correct_result(0, 0xff74)
    print(f"Observed correction ff74: raw zero -> {corrected} half-dB -> RXLEV {encode_rxlev(corrected)}.")
    for correction in (0xff74, 0xff75, 0xff60, 0xff61, 0xff5d):
        corrected = correct_result(-400, correction)
        clamped = clamp_corrected(corrected)
        print(f"Synthetic raw -400 / correction {correction:04x}: {corrected} half-dB -> clamp {clamped} -> RXLEV {encode_rxlev(clamped)}.")
    print("ARM unavailable marker: corrected 510/511 -> RXLEV 255; not a proven DSP sentinel.")
    print("These are ARM consumer checks, not verification of DSP timing or physical no-signal output.")


if __name__ == "__main__":
    main()
