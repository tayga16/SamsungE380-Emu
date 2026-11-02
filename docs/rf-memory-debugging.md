# Focused RF memory debugging

The previous SIM-present freeze reached the firmware's missing-result fatal
and subsequent unmapped read. The latest exact033f-tail/wrap candidate passes
that boundary and completes900M without a memory fault. The offline
debugger identifies unsupported RF generations without changing guest memory
or masking the error. Local OCR reads Menu/Contacts on the new LCD, but direct
visual standby verification and application-list navigation remain unverified.

## Latest candidate — 2026-11-01: tagged033f scan wrap

Use `dist/Run-E830-RF-Tagged033f-Wrap.cmd`. It adds only the captured
438,569,060 tagged807..810/033f four-request history after exact806/033f,
and443,366,255 same-profile810/033f-to512..516/033f wrap with exact accepted
eight-byte residue plus24zeros. No blanket wrap/profile rule.176Rust/66Python
tests and41,135/458,797/520ARM contracts pass. Both captured replays validate
exact5/4/5/5history and19deadlines, including independent264active-byte peeks.

rf-tagged033f-wrap-candidate-900m reaches900M/InstructionBudget at2107f49e,
memory_faultNone,60LCD equivalents/134standby callbacks/15764IRQs.
Live wrap phases12399/12163 match replay; results443,531,008..443,647,488
pass the old missing generation. Full LCD changed; local OCR reads Menu and
Contacts, middle text remains unreadable. Saved4xPNG is not visually inspected.
The separate reporter watch470M..500M captures190of300entries, no severity1;
it does not cover500M..900M. No-SIM600M report/full LCD exactly match baseline.
Firmware/NAND/raw -400/deadlines/ACK unchanged. UserGUI14708 and old binaries
are preserved. Current detailed evidence is in boot-investigation.md.
SoftLeft down900M/up901M draws text recognized as Messages/Select/Back,
then missing-result EA020101908,604,350 precedes READ_UNMAPPED908,612,174
at the same20847af4/e24ee600. Application-menu graphics are suggested by OCR,
but stable navigation/visual verification remain absent. Early600M input also
fails606,692,352. Investigate the preceding908.16M RF group with independently
matched active bytes; do not suppress the fatal or blame the keypad from this
observation alone. Use the new candidate, not old pre-fix trace expectations.
The follow-up captures908,166,322 tag-clear0..4/0335 retaining tagged810/033f
0e550800/067f0000 plus24zeros. All264active bytes match independent peeks;
156inactive bytes remain unknown. Gain0335/correctionff74 and packerband0
are watched. This transition remains unsupported; verify prior accepted
history and exact extension/tag/range checks before enabling it. The
pre-reporter breakpoint908,604,349 has memory_faultNone.

```powershell
python -B tools/replay_tagged_rf_boundary.py `
  proof/native-windows/rf-tagged033f-next-tail-controls.mmio `
  proof/native-windows/rf-tagged033f-next-controls.mmio `
  proof/native-windows/rf-tagged033f-next-active-peeks.log `
  --boundary 033f-wrap `
  --rustc .tools/rustup/toolchains/stable-x86_64-pc-windows-gnu/bin/rustc.exe
```

## Previous candidate — 2026-11-01

The current tagged033f candidate supersedes the tagged033d checkpoint below.
It adds only the captured033d four-request tail/history and exact tagged
810/033d-to512..516/033f residue transition. Tagged033f format/restarts retain
strict reserved/alignment/extension checks. Future033f four-request history
is NOT enabled.172Rust/66Python tests and41,135/458,797/520original ARM
contract cases pass. tools/replay_tagged_rf_boundary.py independently replays
5/4/5/5 accepted batches, exact history and19deadlines with two inactive-byte
fillers;264active transition bytes match independent peeks.

Use `dist/Run-E830-RF-Tagged033f.cmd`. Old binaries/config and the user's
running tagged033d GUI remain untouched. The900M-budget run passes427.3M,
but stops443,812,212 with READ_UNMAPPED at20847af4/address e24ee600 after
the original missing-result check443,675,858 and reporter443,804,337.
43LCD equivalent writes/109standby callbacks; full LCD bytes match tagged033d.
No visual menu claim. Default no-SIM600M BootReport and full LCD bytes exactly
match the prior baseline. Raw -400, deadlines, pending work and ACK unchanged.

Focused rf-tagged033f-candidate-next-memory captures accepted517..521/033f
at443,526,019 with zero extensions. Collector205769f8 reads zero443,664,746
before that accepted generation's443,691,008 first deadline. Earlier missing
work, not merely this later zero-extension group, needs investigation.
Reporter watch starting427.3M exhausts100entries on severity2/70012c01;
the focused443.5M watch confirms severity1/EA020101. Do not infer no fatal
from an exhausted watch or map the subsequent bad pointer.

Next exact boundary: tagged802..806/033f at438,410,791 accepts5;
807..810/033f tail at438,569,060 accepts4 with zero extensions, but history
remains806/033f. At443,366,255 tagged512..516/033f retains810/033f
0e550800/067f0000 plus24zeros. All264active bytes match independent peeks;
156inactive remain unknown. This same-profile tagged scan wrap is still
unsupported. Reporter breakpoint443,804,336 has no memory fault. Reproduce:

```powershell
python -B tools/inspect_rf_memory_trace.py `
  proof/native-windows/rf-tagged033f-next-controls.mmio `
  --after 443350000 --active-only `
  --compare-peeks proof/native-windows/rf-tagged033f-next-active-peeks.log `
  --compare-at 443366255
```

Those two private output names were accidentally reused; their old426.88M
files are no longer available. Replacement accepted426.88M captures are
rf-tagged033f-accepted-transition-controls/peeks. run-probe.py now rejects
existing .log/.mmio names, preventing capture replacement. The historical
033d commands below need the replacement accepted-stage files for current
verification; their original pre-fix rejection evidence is not recreated.

### Previous tagged033d checkpoint

The historical410.8M boundary below is now passed. src/dsp_rf.rs admits
tagged033d and validates the exact tagged810/0333-to512..516/033d residue.
The captured807..810/0333 four-request tail now updates history only when
its previous806/0333, mask00ff, complete slot records, profile/tag, extension
and timing match. No generic count4 or unknown-profile exception is added.
Raw -400, individual deadlines, pending results across clears and ACK behavior
are unchanged.168Rust and62Python tests pass, as do the original ARM
41,135packer/458,797arithmetic/520collector cases. The no-SIM600M BootReport
and full LCD bytes exactly match the previous baseline.

Use the separate `dist/Run-E830-RF-Tagged033d.cmd` candidate. MainGUI,
distCLI and the old RF Debug candidate remain unchanged. The new GUI's brief
startup/close smoke passes; long CLI boot is not a verified SIM-menu boot.
A900M-budget run stops early at427,332,247 with READ_UNMAPPED at
20847af4/address e24ee600. Before that it reports severity1 EA020101 at
427,324,372. A breakpoint before that reporter has memory_faultNone.
The later bad pointer is not established as an RF RAM overrun, and no missing
memory is mapped to hide it. The candidate LCD has43equivalent writes;
PPM/PNG are saved but visual inspection was unavailable in this session.

The next unsupported boundary is now captured independently:

| Control time | Requests | Observation |
| --- | --- | --- |
| 421,930,768 | tagged802..806/033d, mask03ff | Accepted5, history806/033d |
| 422,088,363 | tagged807..810/033d, mask00ff | Accepted4; collector readsf0 at422,384,626; history still806/033d |
| 426,886,248 | tagged512..516/033f, mask03ff | Rejected by tag/profile guard; residue810/033d, then24zero bytes |
| 427,046,014 | tagged517..521/033f, mask03ff | Rejected;32zero extension bytes |

Original gain033f/correctionff5d (-163) and independent band8/channel512
packer inputs are captured. The first packed pair is0c010800/067f0000;
scratch residue is0e550800/067b0000. All264captured active bytes at the
first control match rf-tagged033f-next-active-peeks;156inactive bytes remain
unknown. Collector reads zero at427,024,746 and427,184,776, then original
2098dde6 missing-result check has R2=0/R7=1f at427,195,893.

Historical command (the original files were replaced; see the correction
above, and use accepted-transition-controls/peeks for current426.88M data):

```powershell
python -B tools/inspect_rf_memory_trace.py `
  proof/native-windows/rf-tagged033f-next-controls.mmio `
  --after 426870000 --active-only `
  --compare-peeks proof/native-windows/rf-tagged033f-next-active-peeks.log `
  --compare-at 426886248
```

To reproduce the bounded controls capture with the four HLE options used
below, use RAM/DSP AFTER426870000, RAM LIMIT850, DSP I/O LIMIT1400,
WATCH AFTER426870000/LIMIT80, break reporter200141c0 after427300000,
and budget427400000. Use a fresh output name. The controls log is374,858bytes.
An earlier450-entry I/O budget exhausted on polling before these controls;
absence of controls in that log does not imply their absence in the guest.
For independent peeks, break205747f4 after426880000, budget426900000,
and peek every aligned word60000658..600007f8. CLI breakpoint counts and
watch/control counts can differ by one; compare at the control timestamp.

That scoped033d tail/033f transition is implemented in the current candidate
above. Preserve original firmware/NAND, raw -400/deadlines and the no-SIM
baseline; do not suppress the reporter or later memory fault.

The remaining sections record older checkpoints, not current decoder behavior.

## Captured boundary before the tagged0333 fix (E830XEGG1, 2026-10-31)

| Control time (instructions) | Active request | HLE observation |
| --- | --- | --- |
| 392,007,511 | mask003f, channels883..885, tag clear, profile033f | Accepted3; final cached pair06eb0800/067f0000 |
| 393,915,273 | mask0; old bytes remain in slots | Idle, no replay |
| 394,085,629 | mask03ff, channels512..516, tag set, profile0333 | No submit; +8 retains the accepted885/033f pair |
| 394,245,410 | mask03ff, channels517..521, tag set, profile0333 | No submit;32extension bytes zero |

The band tag is a packed-command bit, not a newly established physical band
identity. At this checkpoint the decoder rejected it, even with a zero
extension. The later candidate below verifies original gain/correction and
accepts tagged0333 only; the table records the pre-fix observations.

At394,384,590 the collector reads zero flags; at394,395,852 its original
missing-result check2098dde6 has R2=0/R7=1f and reports EA020101. It later
enters fatal sleep. Both CLI probes record `memory_fault: None`.
The captured GUI still shows an animation fragment and lower-screen artifacts.

Local evidence (private probes, not firmware/public package contents):

- `proof/native-windows/rf-memory-last-generation.log` and `.mmio`: bounded
  stores, control writes, ARM registers and missing-result check.
- `proof/native-windows/rf-memory-active-submit.log`: independent breakpoint
  at205747f4 just after394,245,410 control,105aligned RF-window peeks.
- `proof/native-windows/gui-user-rf-memory-freeze.png`: passive screenshot of
  the user's existing GUI; its process and executable were not replaced.

The reconstruction matches the independent peeks for all264captured bytes:
4mask,60active-record bytes,200active-command bytes. The remaining156bytes
are inactive/uncaptured, not inferred as zero. No full RAM/NAND dump was made.

## Verified fix and next boundary

The new `dist/Run-E830-RF-Debug.cmd` uses the separate
`e830-emulator-rf-debug.exe`; the existing main GUI/CLI are not overwritten.
Original band8/channel512 inputs, gain0333 and correctionff75 (-139) are
verified in `rf-bandtag0333-gain` and `rf-bandtag0333-input`. The decoder
admits tagged0333, preserves ten-bit channel decoding and same-tag restart
history, and requires the exact accepted clear885/033f residue for the
captured transition to tagged512..516/0333. Raw -400 and timing are unchanged.
The original collector now readsf8 at394,384,590. The900M probe passes the
old fatal but still ends in fatal sleep, with37LCD writes and no menu.

`rf-bandtag0333-next-memory.mmio` captures the new boundary:

| Control time (instructions) | Active request | HLE observation |
| --- | --- | --- |
| 410,405,639 | mask03ff, tagged512..516/profile033d | Unsupported; +8 pair0e550800/06670000 (tagged810/0333), then24zero bytes |
| 410,565,420 | mask03ff, tagged517..521/profile033d | Unsupported;32extension bytes zero |

Original gain033d and correctionff61 (-159) are watched. The collector reads
zero flags at410,544,590 and410,704,590; missing-result check2098dde6 at
410,715,852 has R2=0/R7=1f, and severity1 reporter at410,844,338 reports
EA020101. Tagged033d remains rejected. The bounded trace begins after the
preceding accepted tail; the residue alone cannot authorize that transition.
The later diagnostic below verifies accepted tagged810/0333 history and
exact first-group inputs; implementation still needs the scoped fix. No blanket
allowlist, error suppression, passing guest masks or full memory dumps.

164Rust and62Python tests pass, including10trace-parser regressions.
The no-SIM600M report and LCD hash exactly match the previous baseline.
Detailed probes/hashes are in [boot-investigation.md](boot-investigation.md).

## Follow-up: four-request tail and independent active snapshot

The user's open RF Debug GUI (process32600) shows the same pink animation
fragment; it was passively captured, not closed or attached to a debugger.
The matching CLI reproduces the original missing-result fatal with
`memory_fault: None`. The active RF buffer matches independent peeks; this
is not a demonstrated RAM overrun or proof that all RAM/graphics are correct.

`rf-tagged033d-memory-tail-fatal.mmio` starts at405.58M and captures:

| Control time | Requests | Acceptance/history |
| --- | --- | --- |
| 405,610,184 | mask03ff, tagged802..806/0333 | Accepted5; cache ends at806 |
| 405,768,363 | mask00ff, tagged807..810/0333 | Accepted4; results complete, collector flagsf0 at406,064,626; cache not updated |
| 410,405,639 | mask03ff, tagged512..516/033d | Actual+8 residue is810/0333, not cached806/0333 |

At this historical checkpoint `RfHle::submit` caches only count3/5, so the fully accepted count4
tail leaves stale history. The current fatal still first encounters the
tagged033d profile rejection; admitting033d alone would leave the independent
history/transition constraint unsatisfied. Fix both only with the exact
captured batch and transition checks, preserving partial-batch rejection.

`rf-tagged033d-first-input.log` breaks before original2098d854:
channel512, band8, requested -211, gain033d; original correction isff61
(-159). Peeks confirm signed byte+2 at3003341a and scratch+8 pair
0e550800/06670000 before packing. The emitted words0c010800/067b0000 match
the ARM contract. `rf-tagged033d-active-submit-peeks.log` stops at205747f4
immediately after410,405,639 control and independently reads105aligned words.
All264captured active bytes match;156inactive bytes remain unknown/inferred
neither as zero nor from peeks. No HLE/source behavior is changed here.

```powershell
python -B tools/inspect_rf_memory_trace.py `
  proof/native-windows/rf-tagged033d-memory-tail-fatal.mmio `
  --after 405580000 --active-only

python -B tools/inspect_rf_memory_trace.py `
  proof/native-windows/rf-tagged033d-memory-tail-fatal.mmio `
  --after 410405000 --before 410406000 --active-only `
  --compare-peeks proof/native-windows/rf-tagged033d-active-submit-peeks.log `
  --compare-at 410405639
```

For reproduction with the options below, start RAM/DSP/watch tracing at
405580000, use RAM limit2200, DSP I/O limit450 and watch limit180, and choose
a fresh output name. The expanded trace actually captured636RAM writes and
is about352KiB. To independently inspect the active control, break at205747f4
after410405000 with budget410500000 and peek60000658..600007f8 every4bytes.
For the first inputs, break at2098d854 after410390000 and peek the request
304a86d0/d4/d8,300333fc,30031d34,30033418 and scratch3002995c/60/64/68.

## Capture a small trace at the new boundary

Run from the project root with the newly built native release CLI and verified
local OneNAND image. `tools/run-probe.py` clears inherited E830 variables for
its child and writes separate UTF-8 stdout/stderr logs. This bounded probe
stops at the original fatal reporter and preserves real firmware/NAND/RTOS.

```powershell
python -B tools/run-probe.py `
  --env E830_SIM_PRESENT=1 `
  --env E830_DSP_BOOT_HLE=1 `
  --env E830_DSP_RF_HLE=empty `
  --env E830_ANALOG_HLE=battery `
  --env E830_TRACE_RAM_START=0x60000658 `
  --env E830_TRACE_RAM_END=0x600007fb `
  --env E830_TRACE_RAM_AFTER=410200000 `
  --env E830_TRACE_RAM_LIMIT=750 `
  --env E830_TRACE_DSP_IO=1 `
  --env E830_TRACE_DSP_IO_AFTER=410200000 `
  --env E830_TRACE_DSP_IO_LIMIT=200 `
  --env E830_WATCH_PC=0x205760b8,0x205760f6,0x2098d854,0x2098dde6,0x200141c0 `
  --env E830_WATCH_AFTER=410200000 `
  --env E830_WATCH_LIMIT=120 `
  rf-bandtag0333-next-memory-repeat -- `
  --break-at 0x200141c0 --break-after 410800000 `
  --max-instructions 411000000 `
  --onenand-image proof/native-windows/direct-call-side/tfs-v4/full-tfs-2k.onenand.bin `
  --peek 0x3049198c --peek 0x60000658 --peek 0x304a8f44 --peek 0x304a8f48
```

The new bounded capture took54.331s on the current host; performance is not a
guarantee. The previous pre-fix trace was about299KiB, not a full RAM dump.
The tracing hook already exists in current binaries. Output goes to stderr;
there is no new GUI memory panel at this checkpoint.

## Reconstruct and verify

```powershell
python -B tools/inspect_rf_memory_trace.py `
  proof/native-windows/rf-bandtag0333-next-memory.mmio `
  --after 410200000 --active-only

# Historical pre-fix trace and its matching independent peeks:
python -B tools/inspect_rf_memory_trace.py `
  proof/native-windows/rf-memory-last-generation.mmio `
  --after 394000000 --active-only

python -B tools/inspect_rf_memory_trace.py `
  proof/native-windows/rf-memory-last-generation.mmio `
  --after 394000000 --active-only `
  --compare-peeks proof/native-windows/rf-memory-active-submit.log `
  --compare-at 394245410

python -B -m unittest discover -s tools -p 'test_*.py'
```

The tool applies captured1/2/4/8byte little-endian stores to an offline map,
snapshots it at actual16bit `50505000=40` writes, and reuses the ARM schedule
inspector. It shows channels, tag/profile, raw words, extension coverage and
last captured store PC/time. Later mask clears cannot erase earlier snapshots.
`--active-only` hides known zero-mask controls; unknown masks stay explicitly
incomplete. `--after`/`--before` filter output, retaining earlier write history.

Result-flag reads are chronological, not automatically assigned to generations.
HLE submit entries match only exact control timestamps. No submit log alone
does not prove rejection: enablement, image gate, timer phase, trace completeness
and decoder code must also be checked. No stored request proves a DSP result.

For an independent final active snapshot, run the same four HLE options,
break at205747f4 after394245000 with budget394300000, and request aligned
`--peek` addresses60000658 through600007f8 inclusive. The existing local
`rf-memory-active-submit.log` contains that report. Comparison fails on a
mismatch and never fills unknown bytes from peeks. Only compare the same
original control boundary; a later fatal/idle snapshot can contain cleared
masks or reused commands.

Limitations: initial RAM, stores before `AFTER`, stores after `LIMIT` and
direct host `mem_write` calls are not reconstructed. An earlier captured
value cannot prove that no uncaptured writer subsequently changed it. Keep
the trace bounded and verify relevant active bytes independently.

That historical next step is implemented in the2026-11-01 candidate above.
Continue with its separately captured033d tail and tagged033f transition;
older snapshots cannot validate the later generation.
