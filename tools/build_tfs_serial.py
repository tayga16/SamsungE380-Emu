"""Build a verified Clifford VE UART stream for an E830 CFG/TFS package.

The stream mounts the already formatted KFATFS volume, creates every declared
directory, writes every file in CFG order, then unmounts it. The companion
response file lets onenand-loader-probe reject a partial or desynchronized run
before saving a NAND image.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from prepare_nand import parse_config


CHUNK = 8192


def packet_checksum(content):
    return sum(content) & 0xFF


def build(cfg_path, tfs_path, stream_path, response_path, manifest_path):
    config_bytes = cfg_path.read_bytes()
    payload = tfs_path.read_bytes()
    directories, resources, total = parse_config(config_bytes.decode("utf-8-sig"))
    if total != len(payload):
        raise ValueError(f"CFG describes {total} bytes; TFS has {len(payload)}")
    if any(not path.isascii() for path in directories + [entry.path for entry in resources]):
        raise ValueError("Clifford VE path protocol requires ASCII paths")

    stream = bytearray([0xDA])
    response = bytearray([0xAB, 0xDA])

    for path in directories:
        encoded = path.encode("ascii")
        header = len(encoded).to_bytes(2, "big") + encoded
        stream.extend(b"\xDB" + header)
        response.extend((packet_checksum(header), 0xDC))

    for entry in resources:
        encoded = entry.path.encode("ascii")
        header = (len(encoded).to_bytes(2, "big") + encoded
                  + entry.size.to_bytes(4, "big"))
        stream.extend(b"\xDD" + header)
        response.append(packet_checksum(header))
        content = payload[entry.offset:entry.offset + entry.size]
        for offset in range(0, len(content), CHUNK):
            chunk = content[offset:offset + CHUNK]
            stream.extend(b"\xFE" + chunk)
            if len(chunk) == CHUNK:
                response.append(packet_checksum(chunk))
            else:
                response.append(0xDF)
        if content and len(content) % CHUNK == 0:
            stream.append(0xFE)
            response.append(0xDF)

    stream.append(0xE4)
    response.append(0xE4)

    for path in (stream_path, response_path, manifest_path):
        if path.exists():
            raise FileExistsError(f"output already exists: {path}")
        if not path.parent.is_dir():
            raise ValueError(f"output parent does not exist: {path.parent}")
    stream_path.write_bytes(stream)
    response_path.write_bytes(response)
    manifest = {
        "cfg": str(cfg_path.resolve()),
        "cfg_sha256": hashlib.sha256(config_bytes).hexdigest(),
        "tfs": str(tfs_path.resolve()),
        "tfs_sha256": hashlib.sha256(payload).hexdigest(),
        "directories": len(directories),
        "files": len(resources),
        "payload_bytes": len(payload),
        "serial_bytes": len(stream),
        "expected_response_bytes": len(response),
        "protocol": "DA mount; DB directory; DD file header; FE chunks; E4 unmount",
    }
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cfg", type=Path, required=True)
    parser.add_argument("--tfs", type=Path, required=True)
    parser.add_argument("--stream", type=Path, required=True)
    parser.add_argument("--response", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = build(args.cfg, args.tfs, args.stream, args.response, args.manifest)
    except (OSError, ValueError) as error:
        parser.exit(1, f"Stream build failed: {error}\n")
    print(json.dumps(result, indent=2))
