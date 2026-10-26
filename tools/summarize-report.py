"""Print CLI boot reports with guest addresses in hexadecimal."""
import argparse
from pathlib import Path
import re
from collections import Counter

parser = argparse.ArgumentParser()
parser.add_argument('report', type=Path)
args = parser.parse_args()
raw_report = args.report.read_bytes()
report = raw_report.decode('utf-16' if raw_report[:2] in (b'\xff\xfe', b'\xfe\xff') else 'utf-8-sig')
report = report.replace('\r\n', '\n')
for name, raw in re.findall(r'^    (\w+): (\d+),$', report, re.MULTILINE):
    value = int(raw)
    print(f'{name}: {value} / 0x{value:08x}')
accesses = re.findall(
    r'MmioAccess \{\s*write: (\w+),\s*address: (\d+),\s*size: (\d+),\s*value: (\d+),\s*pc: (\d+),', report
)
for (write, address, size, value, pc), count in Counter(accesses).items():
    print(f'{"W" if write == "true" else "R"} 0x{int(address):08x} size={size} '
          f'value=0x{int(value):08x} pc=0x{int(pc):08x} ({count} accesses)')
