"""Run e830-cli with separate UTF-8 stdout/stderr logs (PowerShell 5 safe)."""
import argparse
import os
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("name")
parser.add_argument("--env", action="append", default=[], metavar="NAME=VALUE")
parser.add_argument("args", nargs=argparse.REMAINDER)
options = parser.parse_args()
root = Path(__file__).resolve().parent.parent
output = root / "proof" / "native-windows"
output.mkdir(parents=True, exist_ok=True)
environment = {key: value for key, value in os.environ.items() if not key.startswith("E830_")}
for assignment in options.env:
    key, value = assignment.split("=", 1)
    environment[key] = value
arguments = options.args
if arguments and arguments[0] == "--":
    arguments = arguments[1:]
command = [str(root / "target" / "release" / "e830-cli.exe"),
           str(root / "firmware" / "E830XEGG1.cla"),
           str(root / "firmware" / "E830 eeprom.nsf"),
           "--tfs", str(root / "firmware" / "E830XEGG1.tfs"), *arguments]
for suffix in (".log", ".mmio"):
    destination = output / f"{options.name}{suffix}"
    if destination.exists():
        parser.error(f"capture already exists: {destination}; choose a fresh name")
with (output / f"{options.name}.log").open("wb") as stdout, \
        (output / f"{options.name}.mmio").open("wb") as stderr:
    started = time.perf_counter()
    result = subprocess.run(command, cwd=root, env=environment, stdout=stdout, stderr=stderr)
    elapsed = time.perf_counter() - started
print(f"Exit code: {result.returncode}; elapsed: {elapsed:.3f}s; report: {output / (options.name + '.log')}")
raise SystemExit(result.returncode)
