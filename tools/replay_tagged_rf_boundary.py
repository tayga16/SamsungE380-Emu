"""Replay private, bounded tagged033d/033f captures through the actual Rust HLE.

All active bytes must be captured. Unknown inactive bytes get two different
test fillers, never inferred zeros. No guest memory, firmware or NAND is changed.
The temporary compiled harness checks accepted history and individual deadlines;
it is not a physical DSP or successful phone-boot test.
"""
import argparse
from pathlib import Path
import subprocess
import tempfile

import inspect_dsp_schedule as dsp
import inspect_rf_memory_trace as ram


def active_fixture(snapshot, filler):
    mask_bytes = [snapshot.memory[dsp.MASK_ADDRESS + i] for i in range(4)]
    mask = int.from_bytes(bytes(mask_bytes), "little")
    required = set(range(dsp.MASK_ADDRESS, dsp.MASK_ADDRESS + 4))
    slots = set()
    for index in range(16):
        if mask & (1 << index):
            record = dsp.RECORD_ADDRESS + index * 6
            required.update(range(record, record + 6))
            header = snapshot.memory[record] | snapshot.memory[record + 1] << 8
            slots.add(header >> 13)
    for slot in slots:
        command = dsp.COMMAND_ADDRESS + slot * 40
        required.update(range(command, command + 40))
    missing = required - snapshot.memory.keys()
    if missing:
        raise ValueError(f"uncaptured active byte at 0x{min(missing):08x}")
    return [snapshot.memory.get(address, filler)
            for address in range(dsp.MASK_ADDRESS, ram.WINDOW_END)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tail_trace", type=Path)
    parser.add_argument("transition_trace", type=Path)
    parser.add_argument("transition_peeks", type=Path)
    parser.add_argument("--rustc", type=Path, required=True)
    parser.add_argument("--boundary", choices=("033d-to-033f", "033f-wrap"), default="033d-to-033f")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    try:
        captures = {}
        for path in (args.tail_trace, args.transition_trace):
            with path.open(encoding="utf-8") as source:
                for snapshot in ram.parse_trace(source).snapshots:
                    captures[snapshot.instructions] = snapshot
        wrap = args.boundary == "033f-wrap"
        times = ([438410791, 438569060, 443366255, 443526019] if wrap
                 else [421930768, 422088363, 426886248, 427046014])
        snapshots = [captures[at] for at in times]
        count = ram.compare_peeks(snapshots[2], dsp.parse_peeks(
            args.transition_peeks.read_text(encoding="utf-8")))
        if count != 264:
            raise ValueError(f"expected 264 independently compared bytes, got {count}")
        # Captured phases and unchanged-frame-origin phases for previously
        # rejected batches; both boundaries were subsequently verified live.
        phases = [16935, 15204, 12399, 12163] if wrap else [16912, 14507, 12392, 12158]
        counts = [5, 4, 5, 5]
        histories = [[0, 8, 0x4d, 0x0e, 0, 0, 0x7b, 6],
                     [0, 8, 0x55, 0x0e, 0, 0, 0x7b, 6],
                     [0, 8, 9, 0x0c, 0, 0, 0x7f, 6],
                     [0, 8, 0x13, 0x0c, 0, 0, 0x7f, 6]]
        deadline_groups = [
            [(7, 422091008), (6, 422120128), (5, 422149248), (4, 422178368), (3, 422207488)],
            [(7, 422251008), (6, 422280128), (5, 422309248), (4, 422338368)],
            [(7, 427051008), (6, 427080128), (5, 427109248), (4, 427138368), (3, 427167488)],
            [(7, 427211008), (6, 427240128), (5, 427269248), (4, 427298368), (3, 427327488)],
        ]
        if wrap:
            histories[0][6] = histories[1][6] = 0x7f
            deadline_groups = [
                [(7, 438571008), (6, 438600128), (5, 438629248), (4, 438658368), (3, 438687488)],
                [(7, 438731008), (6, 438760128), (5, 438789248), (4, 438818368)],
                [(7, 443531008), (6, 443560128), (5, 443589248), (4, 443618368), (3, 443647488)],
                [(7, 443691008), (6, 443720128), (5, 443749248), (4, 443778368), (3, 443807488)],
            ]
        rust_source = root / "src" / "dsp_rf.rs"
        # Inner module docs do not belong inside this isolated wrapper.
        # All implementation statements remain unchanged.
        implementation = rust_source.read_text(encoding="utf-8")
        implementation = "\n".join(line for line in implementation.splitlines() if not line.startswith("//!"))
        code = ["mod rf { " + implementation + "\npub fn replay() {"]
        for filler in (0xa5, 0x5a):
            code.append("{ let mut hle = RfHle::new(true);")
            for index, snapshot in enumerate(snapshots):
                fixture = active_fixture(snapshot, filler)
                literal = ",".join(map(str, fixture))
                code.append(f"hle.clear(); let bytes = vec![{literal}];")
                code.append(f"assert_eq!(hle.submit(bytes.clone(), {times[index]}, {phases[index]}, 32), {counts[index]});")
                code.append(f"assert_eq!(hle.previous_command, Some({histories[index]}));")
                code.append(f"assert_eq!(hle.submit(bytes, {times[index] + 1}, {phases[index] + 1}, 32), 0);")
                expected_pending = deadline_groups[index]
                if index in (1, 3):
                    expected_pending = deadline_groups[index - 1] + expected_pending
                expected = "vec![" + ",".join(f"({slot},{deadline})" for slot, deadline in sorted(expected_pending)) + "]"
                code.append("let mut pending: Vec<_> = hle.pending.iter().map(|(r,d)| (r.slot,*d)).collect(); pending.sort();")
                code.append(f"assert_eq!(pending, {expected});")
                if index in (1, 3):
                    for slot, deadline in sorted(expected_pending, key=lambda item: item[1]):
                        code.append(f"assert!(hle.take_ready({deadline - 1}).is_empty());")
                        code.append(f"assert_eq!(hle.take_ready({deadline}), vec![({slot}, -400)]);")
                    code.append("assert!(!hle.busy());")
                if index == 1:
                    code.append(f"hle.clear(); let mut idle = vec![{literal}]; idle[..4].fill(0);")
                    code.append(f"assert_eq!(hle.submit(idle, {times[index] + 500000}, 0, 32), 0);")
                    code.append(f"assert_eq!(hle.previous_command, Some({histories[index]}));")
            code.append(f"hle.reset(); assert_eq!(hle.previous_command, None); assert!(!hle.busy());")
            literal = ",".join(map(str, active_fixture(snapshots[2], filler)))
            code.append(f"assert_eq!(hle.submit(vec![{literal}], {times[3] + 500000}, 0, 32), 0);")
            code.append("}")
        code.append('} } fn main() { rf::replay(); println!("Captured batches accept5/4/5/5; exact history and19 deadlines match for both inactive fillers."); }')
        with tempfile.TemporaryDirectory(prefix="e830-rf-replay-") as temporary:
            executable = Path(temporary) / "replay.exe"
            subprocess.run([str(args.rustc.resolve()), "--edition=2021", "-A", "dead_code",
                            "-C", "link-self-contained=yes", "-o", str(executable), "-"],
                           input="\n".join(code), text=True, check=True)
            subprocess.run([str(executable)], check=True)
        print(f"Independent first-transition peeks match {count} captured bytes. No phone state modified.")
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
