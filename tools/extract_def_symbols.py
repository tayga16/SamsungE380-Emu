import argparse
import csv
import json
from pathlib import Path
import re
import sys


SYMBOL_LINE = re.compile(
    r"^\s*(?P<name>[A-Za-z_$?][A-Za-z0-9_$?.@-]*)\s+"
    r"(?P<address>0x[0-9A-Fa-f]{1,8})\s*$"
)


def parse_address(value: str) -> int:
    return int(value, 0)


def read_symbols(path: Path):
    for line_number, line in enumerate(
        path.read_text(encoding="utf-8", errors="replace").splitlines(), start=1
    ):
        match = SYMBOL_LINE.match(line)
        if match:
            yield {
                "name": match.group("name"),
                "address": int(match.group("address"), 16),
                "line": line_number,
            }


def select_symbols(symbols, prefixes, name_pattern, address_min, address_max):
    regex = re.compile(name_pattern, re.IGNORECASE) if name_pattern else None
    for symbol in symbols:
        if prefixes and not any(symbol["name"].startswith(prefix) for prefix in prefixes):
            continue
        if regex and not regex.search(symbol["name"]):
            continue
        if address_min is not None and symbol["address"] < address_min:
            continue
        if address_max is not None and symbol["address"] > address_max:
            continue
        yield symbol


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Extract selected address symbols from a local Samsung DEF file."
    )
    parser.add_argument("def_file", type=Path)
    parser.add_argument("--prefix", action="append", default=[])
    parser.add_argument("--name-regex")
    parser.add_argument("--address-min", type=parse_address)
    parser.add_argument("--address-max", type=parse_address)
    parser.add_argument("--format", choices=("text", "csv", "json"), default="text")
    args = parser.parse_args(argv)

    selected = list(
        select_symbols(
            read_symbols(args.def_file),
            args.prefix,
            args.name_regex,
            args.address_min,
            args.address_max,
        )
    )

    if args.format == "json":
        json.dump(selected, sys.stdout, indent=2)
        sys.stdout.write("\n")
    elif args.format == "csv":
        writer = csv.writer(sys.stdout, lineterminator="\n")
        writer.writerow(("address", "name", "source_line"))
        for symbol in selected:
            writer.writerow(
                (f"0x{symbol['address']:08x}", symbol["name"], symbol["line"])
            )
    else:
        for symbol in selected:
            print(f"0x{symbol['address']:08x} {symbol['name']}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
