"""Check the original ARM RF command packer and untouched scratch extension.

An isolated harness, not a phone boot or a DSP model. Synthetic arguments and
scratch bytes are never injected into the emulator. A packer contract does not
establish DSP timing, interpretation of extension bytes or RF signal strength.
"""
import argparse
import hashlib
from pathlib import Path
import sys


CODE_ADDRESS = 0x2098d854
CODE_SIZE = 0x54  # Function and its band-tag literal at 2098d8a4.
CODE_SHA256 = "07e523d3b240bb9a300316b6599d9ced0d65a867cff30ae9b748799f5eb4afb6"


def checked_code(image):
    offset = CODE_ADDRESS - 0x20000000
    code = image[offset:offset + CODE_SIZE]
    if len(code) != CODE_SIZE or hashlib.sha256(code).hexdigest() != CODE_SHA256:
        raise ValueError("packer instructions do not match local E830XEGG1")
    return code


def expected_words(channel, band, calibration):
    tag = 0x10000 if band in (8, 16) else 0
    first = (tag | (channel & 0x3ff) << 6 | 0x21) << 11
    shifted = (calibration << 6) & 0xffffffff
    if band in (4, 8) and not shifted & 0x8000:
        shifted = max(0, shifted - 0x200)
    second = ((shifted | 0x20) << 11) & 0xffffffff
    return first, second


def verify(image_path):
    code = checked_code(image_path.read_bytes())
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / ".tools/analysis"))
    from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB
    from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
                                   UC_ARM_REG_R4, UC_ARM_REG_SP, UC_ARM_REG_LR,
                                   UC_ARM_REG_PC)

    cpu = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    cpu.mem_map(CODE_ADDRESS & ~0xfff, 0x1000)
    cpu.mem_write(CODE_ADDRESS, code)
    scratch, argument, stack, return_address = 0x30000000, 0x30000100, 0x30000e00, 0x30000f00
    cpu.mem_map(scratch, 0x1000)
    residues = [bytes(range(40)), bytes(8) + bytes.fromhex("0008150400006706") + bytes(24)]
    count = 0

    def run(channel, band, calibration, residue):
        cpu.mem_write(scratch, residue)
        cpu.mem_write(argument, b"\0\0" + channel.to_bytes(2, "little")
                      + b"\0\0" + bytes([band, 0]))
        for register, value in ((UC_ARM_REG_R0, scratch), (UC_ARM_REG_R1, argument),
                                (UC_ARM_REG_R2, calibration), (UC_ARM_REG_R4, 0x12345678),
                                (UC_ARM_REG_SP, stack), (UC_ARM_REG_LR, return_address | 1)):
            cpu.reg_write(register, value)
        cpu.emu_start(CODE_ADDRESS | 1, return_address, count=60)
        if cpu.reg_read(UC_ARM_REG_PC) != return_address:
            raise ValueError("packer did not return within instruction budget")
        actual = bytes(cpu.mem_read(scratch, 40))
        expected = b"".join(word.to_bytes(4, "little")
                            for word in expected_words(channel, band, calibration)) + residue[8:]
        if actual != expected:
            raise ValueError(f"packer/scratch mismatch: channel={channel}, band={band}, cal={calibration:x}; "
                             f"actual={actual.hex()}, expected={expected.hex()}")
        if cpu.reg_read(UC_ARM_REG_R4) != 0x12345678 or cpu.reg_read(UC_ARM_REG_SP) != stack:
            raise ValueError("packer did not preserve caller R4/SP")

    for band in (1, 2, 4, 8, 16):
        for calibration in (0x333, 0x335, 0x33f, 0x33d):
            for channel in range(1024):
                for residue in residues:
                    run(channel, band, calibration, residue)
                    count += 1
        for calibration in (0, 1, 7, 8, 0x1ff, 0x200, 0x400):
            for channel in (0, 522, 523, 527, 1023):
                run(channel, band, calibration, residues[1])
                count += 1
    print(f"Verified {count:,} original ARM packer cases; bytes +8..+39 remain unchanged.")
    print("Synthetic harness only: this does not establish DSP interpretation or RF behavior.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    verify(parser.parse_args().image)
