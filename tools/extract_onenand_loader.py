"""Statically extract the two original ARM payloads from the supplied downloader.

Only the two examined executables are supported. Offsets and lengths come from
its x86 serial-send routines, not from string/signature carving. This tool never
executes the EXE or opens a serial port. Extracted vendor code is local research
material and must stay outside source/binary packages.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct


PROFILES = {
    "0d1c46e5c0090e554e1921d84b01c67032015d63e37e9a04f63173f0bfc65dc0": {
        "name": "Corona v1.7 (XSR1)",
        "payloads": (("boot-loader", 0x434840, 0x2E3B4, 0x18000000),
                     ("pre-loader", 0x462BF8, 0x734, 0x0C000000)),
        "evidence": ((0x408185, "8a8840484300"), (0x407200, "68b4e30200"),
                     (0x4080F0, "8a1c85f82b4600"), (0x40811D, "3dcd010000")),
    },
    "7d2b418b2d8059d6c67bab5b131d6b0b63091478e8855767932765f1f6a79353": {
        "name": "Clifford VE v0.1 Lite with CSC (XSR2)",
        "payloads": (("boot-loader", 0x43B850, 0x3D140, 0x30000000),
                     ("pre-loader", 0x478994, 0x740, 0x1C000000)),
        # Read array base and exact send length; pre-loader transmits 464 words.
        "evidence": ((0x40967A, "8a8850b84300"), (0x40850D, "6840d10300"),
                     (0x4095C0, "8a1c8594894700"), (0x4095ED, "3dd0010000")),
    },
}


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def extract(executable, output):
    content = executable.read_bytes()
    source_hash = sha256(content)
    profile = PROFILES.get(source_hash)
    if profile is None:
        raise ValueError("unexamined downloader version: payload boundaries need verification")
    pe = struct.unpack_from("<I", content, 0x3C)[0]
    if content[pe:pe + 4] != b"PE\0\0":
        raise ValueError("not a PE executable")
    machine, section_count = struct.unpack_from("<HH", content, pe + 4)
    optional_size = struct.unpack_from("<H", content, pe + 20)[0]
    optional = pe + 24
    if machine != 0x14C or struct.unpack_from("<H", content, optional)[0] != 0x10B:
        raise ValueError("expected an i386 PE32 executable")
    image_base = struct.unpack_from("<I", content, optional + 28)[0]
    sections = []
    for index in range(section_count):
        offset = optional + optional_size + index * 40
        name, virtual_size, rva, raw_size, raw_offset = struct.unpack_from("<8sIIII", content, offset)
        sections.append({"name": name.rstrip(b"\0").decode("ascii"),
                         "rva": rva, "virtual_size": virtual_size,
                         "raw_size": raw_size, "raw_offset": raw_offset})

    def file_offset(virtual_address, size):
        rva = virtual_address - image_base
        for section in sections:
            delta = rva - section["rva"]
            if 0 <= delta and delta + size <= section["raw_size"]:
                return section["raw_offset"] + delta
        raise ValueError("payload is outside the PE file-backed sections")

    evidence = profile["evidence"]
    for address, hex_bytes in evidence:
        expected = bytes.fromhex(hex_bytes)
        offset = file_offset(address, len(expected))
        if content[offset:offset + len(expected)] != expected:
            raise ValueError(f"serial-send evidence mismatch at 0x{address:x}")

    payloads = []
    for name, source_va, size, guest_address in profile["payloads"]:
        offset = file_offset(source_va, size)
        data = content[offset:offset + size]
        payloads.append((name, data, {"name": name, "source_va": source_va,
                         "file_offset": offset, "size": size,
                         "load_address": guest_address, "sha256": sha256(data)}))
    output.mkdir(parents=True, exist_ok=False)
    for name, data, _ in payloads:
        with (output / f"{name}.bin").open("xb") as stream:
            stream.write(data)
    manifest = {"source": str(executable.resolve()), "source_sha256": source_hash,
                "profile": profile["name"],
                "sections": sections,
                "send_routine_evidence": [{"address": address, "bytes": value}
                                           for address, value in evidence],
                "payloads": [entry for _, _, entry in payloads],
                "status": "extracted original bytes; compatibility and installation unverified"}
    with (output / "manifest.json").open("x", encoding="utf-8") as stream:
        json.dump(manifest, stream, indent=2, ensure_ascii=False)
        stream.write("\n")
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("output", type=Path, help="new local research directory")
    args = parser.parse_args()
    manifest = extract(args.executable, args.output)
    for payload in manifest["payloads"]:
        print(f"{payload['name']}: {payload['size']} bytes at 0x{payload['load_address']:08x}, "
              f"SHA256 {payload['sha256']}")
