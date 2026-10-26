"""Create erased E830 NAND and unpack CFG/TFS resources for later installation.

This does not format BML/STL/KFATFS or install files into the NAND image.
The existing emulator automatically loads <CLA-stem>.onenand.bin beside the CLA.
"""
import argparse
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import re
import tempfile


BLOCKS = 512
PAGES_PER_BLOCK = 64
MAIN_PER_PAGE = 1024
SPARE_PER_PAGE = 32
MAIN_BYTES = BLOCKS * PAGES_PER_BLOCK * MAIN_PER_PAGE
RAW_BYTES = BLOCKS * PAGES_PER_BLOCK * (MAIN_PER_PAGE + SPARE_PER_PAGE)


@dataclass(frozen=True)
class Resource:
    path: str
    size: int
    offset: int


def validate_path(path):
    """Validate a phone path before mapping it to a Windows staging directory."""
    if not path.startswith("/"):
        raise ValueError(f"Expected an absolute phone path: {path!r}")
    for part in path[1:].split("/"):
        if not part or part in (".", "..") or part.endswith((".", " ")):
            raise ValueError(f"Invalid phone path: {path!r}")
        if re.search(r'[\\<>:"|?*\x00-\x1f]', part):
            raise ValueError(f"Path cannot be extracted on Windows: {path!r}")
        if part.split(".", 1)[0].upper() in {
            "CON", "PRN", "AUX", "NUL", *[f"COM{i}" for i in range(1, 10)],
            *[f"LPT{i}" for i in range(1, 10)],
        }:
            raise ValueError(f"Reserved Windows path: {path!r}")
    return path


def parse_config(text):
    """Read the external CFG in order; never substitute its older embedded copy."""
    fields, directories, resources = {}, [], []
    state, pending, offset = "header", None, 0
    transitions = {
        ("header", "SET_DIR"): "directories",
        ("directories", "END_DIR"): "after_directories",
        ("after_directories", "SET_FILE"): "files",
        ("files", "END_FILE"): "after_files",
        ("after_files", "END_TFS"): "done",
    }
    for line_number, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if (state, line) in transitions:
            if pending is not None:
                raise ValueError(f"Missing FILE_SIZE at line {line_number}")
            state = transitions[state, line]
            continue
        key, separator, value = line.partition(":")
        key, value = key.strip(), value.strip()
        if not separator:
            raise ValueError(f"Unexpected CFG line {line_number}: {line!r}")
        if state == "header" and key in ("TFSVERSION", "NUM_DIR", "MAXFILES"):
            if key in fields:
                raise ValueError(f"Duplicate {key}")
            fields[key] = value
        elif state == "directories" and key == "DIR_NAME":
            directories.append(validate_path(value))
        elif state == "files" and key == "FILE_NAME" and pending is None:
            pending = validate_path(value)
        elif state == "files" and key == "FILE_SIZE" and pending is not None:
            if not value.isascii() or not value.isdecimal():
                raise ValueError(f"Invalid FILE_SIZE at line {line_number}")
            size = int(value)
            resources.append(Resource(pending, size, offset))
            offset += size
            pending = None
        else:
            raise ValueError(f"Unexpected CFG entry at line {line_number}: {line!r}")
    if state != "done" or pending is not None:
        raise ValueError("Incomplete CFG")
    if fields.get("TFSVERSION") != "TFS4.0_CORONA_01":
        raise ValueError("Unsupported TFS version")
    if int(fields.get("NUM_DIR", "-1")) != len(directories):
        raise ValueError("NUM_DIR does not match directory entries")
    if int(fields.get("MAXFILES", "-1")) != len(resources):
        raise ValueError("MAXFILES does not match file entries")
    # Windows paths are case insensitive; reject aliases before writing any files.
    dir_keys = {path.casefold() for path in directories}
    file_keys = {entry.path.casefold() for entry in resources}
    if len(dir_keys) != len(directories) or len(file_keys) != len(resources):
        raise ValueError("Duplicate or case-colliding CFG paths")
    if dir_keys & file_keys:
        raise ValueError("A CFG path is both a file and a directory")
    for path in directories + [entry.path for entry in resources]:
        parent = path.rsplit("/", 1)[0]
        if parent and parent.casefold() not in dir_keys:
            raise ValueError(f"Missing parent directory for {path!r}")
    return directories, resources, offset


def create_erased_image(path):
    """Use exclusive creation so an existing dump or formatted image stays intact."""
    with path.open("xb") as output:
        chunk = b"\xff" * (1024 * 1024)
        remaining = RAW_BYTES
        while remaining:
            count = min(len(chunk), remaining)
            output.write(chunk[:count])
            remaining -= count


def stage_resources(destination, directories, resources, payload, config_bytes):
    if destination.exists():
        raise FileExistsError(f"Staging destination already exists: {destination}")
    with tempfile.TemporaryDirectory(prefix=".e830-tfs-", dir=destination.parent) as temp:
        candidate = Path(temp) / "package"
        file_root = candidate / "files"
        file_root.mkdir(parents=True)
        for path in directories:
            (file_root / path[1:]).mkdir(parents=True, exist_ok=True)
        entries = []
        for entry in resources:
            content = payload[entry.offset:entry.offset + entry.size]
            (file_root / entry.path[1:]).write_bytes(content)
            entries.append({"path": entry.path, "offset": entry.offset, "size": entry.size,
                            "sha256": hashlib.sha256(content).hexdigest()})
        manifest = {
            "nand_state": "erased, unformatted; resources NOT installed in NAND",
            "resource_state": "extracted from external CFG/TFS for later installation",
            "geometry": {"blocks": BLOCKS, "pages_per_block": PAGES_PER_BLOCK,
                         "main_per_page": MAIN_PER_PAGE, "spare_per_page": SPARE_PER_PAGE,
                         "main_bytes": MAIN_BYTES, "raw_bytes": RAW_BYTES},
            "tfs_version": "TFS4.0_CORONA_01",
            "tfs_sha256": hashlib.sha256(payload).hexdigest(),
            "cfg_sha256": hashlib.sha256(config_bytes).hexdigest(),
            "payload_bytes": len(payload), "file_count": len(entries),
            "directory_count": len(directories), "directories": directories, "files": entries,
        }
        (candidate / "manifest.json").write_text(
            json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        candidate.rename(destination)
    return manifest


def main():
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path,
                        default=root / "firmware" / "E830XEGG1.onenand.bin")
    parser.add_argument("--cfg", type=Path, help="External CFG matching the retail TFS")
    parser.add_argument("--tfs", type=Path)
    parser.add_argument("--stage", type=Path, help="New directory for extracted resources/manifest")
    args = parser.parse_args()
    if any((args.cfg, args.tfs, args.stage)) and not all((args.cfg, args.tfs, args.stage)):
        parser.error("--cfg, --tfs and --stage must be supplied together")
    try:
        if args.output.exists():
            raise FileExistsError(f"NAND image already exists: {args.output}")
        if not args.output.parent.is_dir():
            raise ValueError("NAND output parent directory must already exist")
        if args.cfg:
            if args.stage.exists() or not args.stage.parent.is_dir():
                raise ValueError("Staging directory must be new and its parent must exist")
            if args.output.resolve().is_relative_to(args.stage.resolve()):
                raise ValueError("Keep the NAND image outside the resource staging directory")
            config_bytes, payload = args.cfg.read_bytes(), args.tfs.read_bytes()
            directories, resources, total = parse_config(config_bytes.decode("utf-8-sig"))
            if total != len(payload):
                raise ValueError(f"CFG describes {total} bytes; TFS has {len(payload)}")
            # Validate the whole package before creating either output.
            stage_resources(args.stage, directories, resources, payload, config_bytes)
            print(f"Resources staged: {len(resources)} files, {len(directories)} directories")
            print(f"Manifest: {args.stage / 'manifest.json'}")
        create_erased_image(args.output)
    except (OSError, ValueError) as error:
        parser.exit(1, f"Preparation failed: {error}\n")
    print(f"NAND created: {args.output}")
    print(f"Main: {MAIN_BYTES} bytes (32 MiB); main+spare: {RAW_BYTES} bytes")
    print("State: erased (0xFF), unformatted; TFS resources are not yet installed.")


if __name__ == "__main__":
    main()
