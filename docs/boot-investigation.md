# E830: current native Windows boot investigation

Updated 2026-10-31. This file describes the standalone Rust/Unicorn model.
The old QEMU, forced-IRQ and clock-acceleration notes are archived in
[boot-investigation-legacy.md](boot-investigation-legacy.md); they contain
superseded conclusions and must not guide the current board implementation.
The existing desktop layout, LCD scaling and firmware picker are preserved;
the Library now also offers an explicit physical NAND image selector.

## Current outcome

The original E830XEGG1 firmware draws its SGH-E830/Samsung splash, creates the
RTOS system stack and unmasks IRQs itself. No main menu is reached. A complete
TFS image is now installed and verified through a fresh original loader process.
Retail E830 BML initialization and FAT mounting also succeed on that image.
The default DSP boot interface reaches the DSP-start handshake described below;
RD16024 execution is still absent. With the opt-in, image-specific
`E830_DSP_BOOT_HLE=1`, genuine HKBD initialization and standby callbacks now
advance the LCD to the original Samsung Fun Club startup animation (21 frames).
The main menu is still not reached. The timer's fractional counter units,
packed one-word serial IRQ9 mode and observed UART0 TX DMA are corrected from
the original drivers. DMA completion now releases the SL3 transmit queue through
the genuine IRQ29 callback. Original LCD window/cursor addressing now preserves
partial updates. The 400M run retains the complete Fun Club image; the 600M run
displays the original readable **Insert SIM / SOS** screen (26 full-screen-
equivalent writes), retained through 900M. The SIM illustration still has
visibly unusual colours, already present in the guest's graphics buffer rather
than introduced by LCD addressing. This is a genuine post-splash screen, not
the main menu. Neither mounting nor animation frames prove boot.

### Checkpoint — 2026-10-31: instruction-hook overhead reduction

The existing CPU already uses Unicorn 2.1.5's TCG JIT. This change reduces
host work in the global instruction hook; it does not replace the CPU engine,
accelerate guest clocks, skip additional guest instructions or bypass boot.

- The RTOS nesting byte is read only for an active IRQ-service trace or actual
  IRQ entry, using a one-byte stack buffer instead of a heap allocation.
  It remains diagnostic, never an IRQ eligibility condition.
- CPSR is read for possible post-EOI architectural service return, enabled
  pending IRQ delivery, or active diagnostics. It is never cached across
  guest instructions, so guest MSR/exception returns are observed immediately.
- IRQ-service diagnostic keys are computed only while that trace is active
  and within its limits. Pending sources are not repeatedly recomputed during
  ordinary delivery. Disabled DSP/wake/timer traces skip their PC filters.
- DMA status returns immediately when the low eight raw terminal-count bits
  are zero, avoiding eight channel-config HashMap lookups. A fully masked
  INTC returns no enabled sources without traversing device IRQ levels.
  Neither shortcut acknowledges or discards a device status.
- Four per-instruction delay/memset signature-set lookups are replaced by one
  PC-to-kind bitmask lookup, built from the same detected addresses. Overlapping
  signatures retain all membership bits and the unchanged handler priority;
  no additional loops are skipped and existing budget accounting is unchanged.

DMA, DSP serial, I2C and system-timer advancement retain the original order
and exact per-instruction boundaries, including masked-IRQ time and existing
fast-forward accounting. No firmware, NAND, GUI layout or peripheral timing
was changed. The read-only input NAND hash remains
`d305c8e0b27e86ecec70534be1ee2c917c1556e675d5868573e600b7cf341a56`.

Same-host release CLI A/B runs use the verified physical NAND,
`E830_DSP_BOOT_HLE=1`, no tracing and a 600,000,000-instruction budget.
Original wall time was **237.077 s**; optimized time was **74.057 s**
(approximately **3.20x** throughput, **68.8%** less wall time). A second pair
took **298.466 s** original and **121.624 s** optimized (**2.45x**); its
optimized BootReport and LCD image also match the original exactly. Host load
affects these measurements, so 74 seconds is not a fixed loading-time claim. These are
bounded CLI execution measurements, not an exact GUI time-to-first-SIM-screen
or a hardware-clock validation. They do not imply that the main menu works.
`tools/run-probe.py` now reports child-process elapsed time for future probes.

The complete BootReport and both RAM peeks are identical after normalizing
only the LCD destination filename: PC `0x204b00d0`, CPSR `0xdf`, no fault,
11,792 IRQs, clock 1,308, one HKBD-init and 48 standby dispatches, 26 LCD
full-screen-equivalent writes. The final **Insert SIM / SOS** PPM is identical,
SHA-256 `a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07`.
Evidence stays local in `proof/native-windows/perf-before-600m.*` and
`perf-after-600m.*`, `perf-final-600m.*` and `perf-sites-600m.*`.

The final release including the combined loop lookup completed the same 600M
probe in **96.258 s** with identical full report, peeks and LCD SHA-256, and
empty diagnostic stderr. Its incremental speedup cannot be isolated reliably
from these variable host-load measurements: 150M probes were 14.849 s before
and 18.987 s after; alternating repeat runs were 23.227/19.427 s and
20.414/11.748 s (before/after). The substantial overall improvement is verified,
but no separate percentage is claimed for this second small optimization.

Regression tests cover every DMA channel mask, reserved status bits and
preserved raw status; selective CPSR reads with trace both on and off;
EOI plus unmask return ordering; unchanged RTOS nesting; and a guest MSR
that must deliver IRQ before the following instruction executes. Two additional
tests compare every combined-loop membership including overlaps/empty sets and
the real nested/fade signature overlap.
Native `build-windows.ps1 -Test` passes **78 core + 7 GUI/runner = 85 tests**.
Release GUI/CLI are rebuilt and copied into the existing local `dist`;
their SHA-256 hashes match the release outputs. The launch helper and selected
NAND configuration are preserved. No firmware/NAND copies or full dumps were
added. The initial validation stayed local; the user subsequently requested
publication of the source, tests and notes to the GitHub repository.
Final GUI SHA-256: `487523580e8ff7d2ed4230a6b84e444e37d4013201af0da8b8a1a130e21502c6`;
CLI: `ccfbca91dc8af90f0170591f9a213419a9b215e77865524ad7be1dea1b376f53`.

The global instruction callback remains in place. Block/event scheduling is
future work requiring stronger interrupt-boundary proofs; it was not needed
for this first measurable improvement. Keyboard/SIM/menu work is still pending.

### Checkpoint — 2026-10-31: GUI physical NAND selection

The Library now has `Select NAND...`, the resolved image path, and
`Use automatic NAND`. The selection is persisted as optional `firmware.onenand`
in `e830-config.json`; older configurations still load. An explicit path wins
over the historical CLA-stem `.onenand.bin` sibling and is never silently
replaced if missing. Returning to automatic mode restores sibling lookup or
erased flash. Re-selecting the same CLA preserves its explicit image; a
different CLA starts with automatic lookup.

Validation accepts only 33,554,432-byte main-only or 34,603,008-byte
1024+32/page physical images. Cancellation and invalid paths/sizes preserve
the prior selection and worker. A valid selection stops the worker, saves the
path and requests a fresh Run. The original source image is read-only;
guest flash changes remain in the existing in-memory overlay. A TFS package
must not be substituted for a physical image. No board/firmware behavior was
changed by this GUI work.

Native release tests pass **73 core + 7 GUI/runner tests**. The new tests cover
old settings, selection round trips, automatic/explicit precedence, missing
explicit paths, actual image loading, both geometries, invalid inputs, picker
separation and cancellation. Release GUI/CLI were rebuilt and copied to the
existing local `dist`; no additional firmware or NAND copies were made.

The local launch uses the verified `full-tfs-2k.onenand.bin` above, SHA-256
`d305c8e0b27e86ecec70534be1ee2c917c1556e675d5868573e600b7cf341a56`.
`dist/Run-E830.cmd` enables `E830_DSP_BOOT_HLE=1` for that launch only, then
invokes `--run`. It does not change the default DSP behavior or system settings.
A live screenshot `proof/native-windows/gui-nand-start.png` verifies the
rebuilt GUI rendering the original SGH-E830/Samsung splash; the process is
left open for the user. A separate brief Library-only GUI process verifies
the selected path and both NAND controls in
`proof/native-windows/gui-nand-library.png`; that verification process was
closed normally without stopping the main running GUI. This checkpoint
does not claim the menu is reached.
`tools/capture-open-window.ps1` can capture an existing GUI without launching
or closing it. Local configuration, launcher and NAND/proof assets remain
outside the public source mirror.

### Checkpoint — 2026-10-31: SL3 transmit self-requeue and missing DMA

The post-packed-mode `serial-packed-late-600m` reaches 600M without a memory
fault, with 13 LCD frames, clock 1308, 5220 IRQs and zero nesting depth. The
serial queue and control/status remain empty. Its final PPM is byte-identical
to the 400M image (SHA-256
`c5482eb59732a3a53a9c134c54473eda77378be1c0aa2731dfd33912e7736e9a`).
No reporter/fatal/delay watch is hit after 280M. The late task trace instead
repeats message `0x6908` (`SL3_SEND_FRAME`) in process `0x69` (`PROCESS_SL3`),
table `0x20e83448`, callback `0x20de42f0`, roughly every 817 instructions.

That callback sees type zero at `[0x301d3e6c+4]+4` and ready bit zero at
`0x300119ac`, so it re-sends the same message through `0x20847a82`. The
29-byte payload is a genuine diagnostic frame containing report `0x70012c01`,
not an invented phone event. This is an identified scheduler loop, not proof
that the report itself is fatal or that fixing it will reach the menu.

Focused `sl3-tx-origin-300m` tracing observes ready-bit initialization at
278,331,618, clearing by `0x209e9464` at 278,338,287, and the original
13-byte send at `0x211f7d2c` at 278,338,297. Completion `0x205c7c48` is never
called. Its real path is UART0 TX through DMA, not the DSP serial queue:
`0x200ea14c -> 0x20d137cc -> 0x204d4a20 -> 0x214f9914` programs the DMA
descriptor; IRQ29 ISR `0x208c8244` reads DMA status and dispatches the original
completion. DMA at `0x50100000` was passive register storage, so
no byte transfer or completion IRQ could occur. UART0 TX uses logical DMA
request 6 and allocated hardware channel 3 (`0x50100160`); RX uses request 7
and channel 4. RX is not completed without actual incoming cable data.

The model now accepts only the observed RAM-to-UART0 byte descriptor:
source `0x31f20400`, destination `0x50403000`, linked-list pointer zero,
control `0x8400000d`, config `0x8841`, global enable one. Its 13 bytes are
read from genuine guest RAM and passed to the UART transmitter. Source advances,
transfer count and enable clear, raw terminal count at `+0x14` sets, masked
terminal count feeds IRQ29, and the original ISR acknowledges through W1C `+8`.
INTC EOI alone does not clear the device. No CPU, stack, callback, guest queue or
ready flag is changed by DMA completion. RX, linked lists, wider transfers and
out-of-RAM sources receive no invented completion. Timing (32 instruction slots
per byte), error handling and other modes remain unverified/unimplemented.

`uart0-dma-clean-400m` observes IRQ29 ISR `0x208c8244` at 278,339,341,
completion `0x205c7c48` at 278,339,448, and firmware restoring ready bit
`0x300119ac` at 278,340,739 through `0x209e926a`. Later diagnostic frames
complete in the same way. At 400M: no memory fault, 21 LCD frames, one HKBD
initialization, 39 standby dispatches, 4,779 IRQs, clock 683 and ready bit one.
The full Fun Club PPM hash is again
`2aa46f28260edbe9a20c6a3d47933b8817e956d3c997f7e03b2e1cb0f82cfbb7`.

`uart0-dma-late-600m` stops on its ordinary instruction budget without a memory
fault: PC `0x204b00d0`, CPSR `0xdf`, 26 LCD frames, 48 standby dispatches,
11,792 IRQs, clock 1,308, ready bit one, raw DMA terminal count zero. The UART
capture reaches its bounded 65,536-byte limit and contains real diagnostic
frames, not injected cable input. Its final PPM SHA-256 is
`3fb2f558d38cffbbaa04264950780b27d393c529ec3edd8650ab49318ea525a1`.
Visual inspection shows a SIM-card illustration with displaced/color-corrupted
regions; it is not the menu. The 25 reporter/fatal watches after 310M are
exhausted by severity-2 diagnostics by about 325M, so their absence later is
not evidence that no later report occurs.

`uart0-dma-idle-400m` rules out a single-PC sleep hang: 1,500 late PC samples
contain 385 distinct addresses. The original idle function `0x20121498` saves
mask `0x0700fdfe`, temporarily writes `0x7fffffff`, calls the firmware-copied
RAM sleep stub `0x31f00140`, restores the original mask and returns to RTOS
work. This repeats about every 441 instructions; periodic IRQs remain active.
The missing IRQ31 wake model is not established as the current blocker. Do not
force a wake interrupt or clear the legitimate nesting byte (one at the sample).

Native tests pass **71/71** for the DMA checkpoint, including a CPU-level test
that completion moves bytes while CPSR.I is set, preserves task registers/RAM
and waits for real architectural IRQ29 entry after the CPU unmasks interrupts.

### Checkpoint — 2026-10-31: original LCD partial-window protocol

Original routine `0x2119f1d0` receives x0, y0, x1, y1 in R0..R3 and programs
`0x44=x0|(x1<<8)`, `0x45=y0|(y1<<8)`, `0x21=x0|(y0<<8)`. The
initialization table at `0x207e7cc8` sets `0x44=0xaf00`, `0x45=0xdb00` and
entry mode `0x03=0x1030`. Table `0x30a2cde4` resolves the original command/data
helpers through `0x215b7b80 -> 0x20cd8f64 -> 0x210db2dc` and
`0x2119f20c -> 0x20cd8f78 -> 0x2009f05c`. These helpers select the register
with bus mode zero, then write its data with bus mode one at `0x44000002`.

The old model ignored all data-register writes except GRAM `0x22` and appended
all pixels consecutively across the whole LCD. It could not implement partial
windows or cursor resets. The updated model stores the observed register data,
uses the original inclusive packed bounds/cursor, and wraps x then y inside
the selected rectangle for observed entry mode `0x1030`. Invalid bounds/cursors
and unsupported scan directions are rejected without host indexing outside the
framebuffer. No firmware graphics or pixels are synthesized. The `lcd_frames`
report remains a count of full-screen-equivalent pixel writes, not proof of
distinct complete refreshes. Two new tests cover partial-window preservation,
cursor reset/wrap, full-screen compatibility and invalid addressing.

Native validation passes **73/73** tests; release GUI and CLI are rebuilt.
`lcd-window-baseline-400m` preserves the previous CPU/IRQ/clock result and the
complete Fun Club PPM byte-for-byte (SHA-256
`2aa46f28260edbe9a20c6a3d47933b8817e956d3c997f7e03b2e1cb0f82cfbb7`).
`lcd-window-validated-600m` reaches its ordinary budget without a memory fault,
with exactly the old CPU state, IRQ count (11,792), clock (1,308), 48 standby
dispatches, ready bit one and raw DMA terminal count zero. Its changed image
hash is `a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07`.
Visual inspection now reads **Insert SIM** and **SOS** in their correct regions;
the SIM illustration's colour anomaly remains unverified. No menu is reached.

The bounded window-only trace is not exhausted (18 hits out of 50). At
494,410,362 the original driver sets full bounds `(0,0)..(175,219)`. From
528,535,206 onward it selects `(0,81)..(175,195)` for the animated SIM region,
using source `0x302eeae8`; eight such partial draws are observed through
593,486,619. This explains why the former global pixel stream displaced the
text and graphic: later animation must not overwrite the top and soft-key rows.
The original registers, firmware and RTOS state are untouched.

`lcd-window-state-900m` finishes without a memory fault: PC `0x20909d12`,
CPSR `0x2000003f`, 38 full-screen-equivalent writes, 73 standby dispatches,
22,245 IRQs, clock 2,246, ready bit one and raw DMA terminal count zero.
Its PPM is byte-identical to the 600M Insert SIM / SOS capture. All **38,720**
RGB565 pixels in genuine firmware graphics buffer `0x302e7b88` match the final
LCD image, with zero mismatches. Thus the remaining colour anomaly originates
upstream of LCD transport/addressing; do not change global colour conversion
to conceal it. The TFS contains original `/images/insert_sim_01..04.ifg`
resources (176x115); their decode/load path is a concrete next graphics lead.

The standby-only watch is not exhausted (33 of 70 hits from 490M, last at
895,838,667); no queue insertion at `0x215b8c50` is observed in this interval.
The 100-entry task trace after 600M is exhausted by 600,328,467, so it describes
only that short interval: real A/D, battery, tracer and SL3 callbacks run,
not a single-PC lockup. Final standby node `0x3197f3bc` still selects table
`0x21717f98`; the 560-byte UI callback queue is zero. These observations do not
prove that populating that queue would enable the menu. Phone key input remains
unimplemented, and there is no modeled external SIM card. The next step is to
trace the genuine missing-card UI/input path, not synthesize a card-in event.

Reproduce this checkpoint from the buildable Downloads tree:

```powershell
& 'C:\Users\Administrator\AppData\Local\Programs\Python\Python312\python.exe' .\tools\run-probe.py `
  --env 'E830_DSP_BOOT_HLE=1' `
  --env 'E830_WATCH_PC=0x2119f1d0,0x205870ec' `
  --env 'E830_WATCH_AFTER=400000000' --env 'E830_WATCH_LIMIT=50' `
  lcd-window-recheck-600m -- `
  --onenand-image .\proof\native-windows\direct-call-side\tfs-v4\full-tfs-2k.onenand.bin `
  --max-instructions 600000000 --peek 0x309011f0 --peek 0x300119ac `
  --dump-lcd-frame .\proof\native-windows\lcd-window-recheck-600m.ppm
```

The DSP opt-in is specifically `E830_DSP_BOOT_HLE`, not
`E830_DSP_RUNTIME_HLE` (the latter is not a recognized option). For PC tracing
use `E830_PC_LOG_AFTER` and `E830_PC_LOG`, not `E830_TRACE_PC_AFTER/LIMIT`.

### Checkpoint — 2026-10-30: packed one-word serial IRQ9 mode

`serial-irq9-late-400m` samples serial control/status at `0x5030100c/14` as
**4 / 4**, with no enabled pending interrupt. The last submitted head starts
at instruction 233,116,307; the two earlier starts at 233,015,346 and
233,079,072 complete through the original IRQ9 ISR. The late head's packed
flag byte +0x0f is 1 and its word count +4 is 1. Original start code
`0x204d03d6..ee` packs the words through `0x208c332c`, then writes
`2 << entry[+4]` to control, i.e. **4** here. The previous IRQ model tested
only control bits 0 and 1, so its completion status could not raise IRQ9.

The accepted source mask now covers this observed bit 2 as well as the two
FIFO modes. Longer packed modes are not inferred. A regression test covers
the deadline, INTC mask, source disable, W0C acknowledgement and absence of
duplicate completion. Native tests pass **68/68** and release binaries are
rebuilt. No guest queue, callback flag, CPU register or firmware is patched.

Fresh `serial-packed-clean-400m` confirms the missing original ISR now enters
at instruction **233,120,488**, then callback `0x215a7cb8` at 233,120,571.
Later queued transfers also complete in genuine ISR context. At 400M the
queue head is zero, serial control/status are **0 / 0**, and the slot callback
flag is cleared by firmware. The run has no memory fault, nesting depth zero,
clock 683, 2,720 IRQs, one HKBD initialization and 20 standby dispatches.
The LCD records 13 frames; the rendered last frame is visibly glitched, not
the former complete 21-frame Fun Club logo and not a verified menu. This is
an unresolved change in animation/boot behavior, not evidence of success.
The ordinary dispatcher calls message filter `0x20dff2e4` at the final stop.

### Checkpoint — 2026-10-30: fractional timer units and active 600M boot

Original timer API `0x2051e9e0` returns the raw counter shifted right by two;
`0x2051e9d6` reads its low two fractional bits. Period/compare setters at
`0x2051eb30/46/5c` update the low 14 bits while preserving the upper two bits.
The model now masks those 14-bit fields and shifts them left by two before
comparison/wrap: period 5000 means raw 20000, and match1 4700 means raw 18800.
The instruction/8 raw-counter rate remains provisional, as do other modes.
Native validation passes **67/67** tests, including fractional reads, field
masking, compare and wrap regression tests. Release GUI and CLI are rebuilt.

Do not treat the previous delay as the primary normal-animation wait. Focused
`funclub-timer-wait` tracing reaches reporter `0x200141c0` at instruction
296,394,127 with severity R0=1, code `0xeb020002`, LR=`0x2118d497` (serial
slot exhaustion), then delay `0x20914214` inside the reporter. The modulo-5000
raw model made that secondary delay impossible to finish. The serial queue
in that reporter's RAM capture has already been cleared by error cleanup;
it is not evidence that the original transfer started with no queued head.

Fresh `timer-fraction-clean-400m` and `timer-fraction-late-600m` stop only on
the instruction budget, without memory faults. Both have 21 genuine Fun Club
frames, one HKBD initialization and 28 standby dispatches. The clock advances
from 683 / 2,703 IRQs at 400M to **1,308 / 5,203 IRQs at 600M**. The late
3,000-PC trace remains in the original dispatcher (`0x208479xx/0x20847axx`),
not the old reporter delay or fatal trap. RTOS nesting depth is zero in both
final captures (`0x30900168=0x01000002`).

The serial queue `0x30a2c354` remains intact in both snapshots: head
`0x304917ec`, tail `0x304918ac`, original handler `0x204d0389`. Its head has
remaining word count at +0x24 equal to zero, but callback `0x215a7cb9` still
set at +0x10 and later entries linked. This narrows the next investigation to
the original serial completion/IRQ/FIFO lifecycle; it does not authorize
clearing queue slots or inventing DSP mailbox payloads. Menu remains unreached.

### Checkpoint — 2026-10-30: genuine IRQ9 completion and Fun Club

Two emulator context errors prevented the ENV initialization callback from
returning. INTC `+0x1c` must expose only the accepted source until EOI, not raw
pending peripheral levels. Queue helper `0x20142258` tests this register at
entry and exit to decide whether to acquire/release the RTOS lock. A new I2C
level between those tests previously skipped the unlock, leaking nesting depth
and eventually reporting `0x220604f1`. Accepted-source OR raw-pending was also
tested and rejected because it still leaks a task-context lock. `+0x14` keeps
the genuine enabled pending levels; architectural IRQ delivery is unchanged.

The old host-injected DSP callback also ran in the wrong context. Callback
`0x215a7cb8` temporarily swaps the main/auxiliary event-stream pointers and
restores them only on return. Direct task-context injection allowed IRQ21 to
drain while those pointers were swapped, overflowing the auxiliary stream and
reporting `0xe70204f0`. The saved CPU context/trampoline injection is removed.

The opt-in exact-image DSP HLE now schedules only serial transfer-done status
bit `4` at `0x50301014`. The original start routine `0x204d0388` enables
control bit 0 for the 32-bit FIFO at `0x50301100` or bit 1 for the 16-bit FIFO
at `0x50301200`; both feed IRQ9. The original firmware executes the complete
chain `IRQ9 -> 0x208a0fd8 -> 0x20010a40 -> 0x210d4fc0 -> 0x21122084 ->
0x215a7cb8`, including queue removal, callback return and W0C acknowledgement.
No guest register, stack, queue or RTOS state is written by completion HLE.
Its 4096-instruction latency is provisional; actual RD16024 execution remains
absent. The default non-HLE path does not invent DSP runtime responses.

ENV initialization reaches its real return at `0x20d901fa`, instruction
231,263,605. Fresh `dsp-irq9-clean-300m` reaches 300,000,000 instructions with
no memory fault, 21 LCD frames, one HKBD initialization, 28 standby dispatches,
3,924 IRQs and `TMD_System_Clock=988`. The last frame is the genuine Samsung
Fun Club logo and `www.samsungmobile.com`, SHA-256
`2aa46f28260edbe9a20c6a3d47933b8817e956d3c997f7e03b2e1cb0f82cfbb7`.
This differs from the earlier one-frame Samsung splash. Native tests pass
**66/66**, including status/IRQ/W0C lifecycle and a CPU-level test proving
completion preserves task registers/stack and waits for architectural IRQ entry.

`dsp-irq9-clean-600m` reaches 600,000,000 instructions without a memory fault
but has exactly the same 21 frames, clock and IRQ totals. PC is `0x2091425e`,
CPSR `0x800000ff`, inside timer delay helper `0x20914214`. The original loop
reads `0x50501000`, shifts right by two and waits for R1=1958. The current
modulo-5000 raw-counter model can never produce more than 1249 after that
shift. Thus waiting longer is insufficient: counter/compare units or timer
mode semantics need correction from original driver evidence, not a PC bypass.
The prior raw 4700/5000 cadence checkpoint below is historical and is not
physical-hardware validation. Nonfatal reports such as `0xaabbccdd` come from
ordinary standby timer logging and must not be treated as fatal stops.

Evidence remains local under `proof/native-windows`: `irq20-unbalanced-lock-230m`,
`intc-service-latch-300m`, `intc-accepted-fatal-origin`, `dsp-irq9-word-late-300m`,
`dsp-irq9-clean-300m`, `dsp-irq9-clean-600m` and `funclub-timer-wait`.

### Checkpoint — 2026-10-30: real state-table dispatch

The earlier keyboard/standby counters were attached to the wrong addresses.
DEF symbols `a_hkbd_Init` at `0x20e82eac` and `a_lk_StandbyLook` at
`0x21717f98` name **state/action tables, not executable functions**. Their old
zero entry counts cannot establish that either task was never scheduled.

The genuine dispatcher executes `blx r1` at `0x208478f2`. R5 points to a
state-list node, and its current table is **`[R5+4]`**, not R5 itself. Entries
are 12 bytes: callback at +0, next table at +4, message ID at +8. The dispatcher
loads the table at `0x2084783e` and stores the next state at `0x20847910`.
`BootReport` now counts `keyboard_init_dispatches` and
`standby_look_dispatches` at that call site. A breakpoint before the `blx`
counts the impending dispatch; separate callback-entry watches verify execution.

A fresh full-TFS run to **300,000,000 instructions**, with the image-specific
DSP HLE enabled, records **one HKBD-init dispatch and two standby dispatches**,
3,941 IRQs, `TMD_System_Clock=1,035`, no memory fault and one LCD frame.
The final splash PPM is byte-identical to the earlier corrected-timer 300M run
(SHA-256 `9140970baf8c6ef39893e9ac6469a27b76ce64c9cfaaace084ed10e41638f4f5`).
The focused callback watches establish this firmware-owned sequence:

| Instruction | Genuine firmware event |
| --- | --- |
| 60,975,655 | HKBD state node `0x3197ee88` is allocated and recorded at `0x3002a1b4`. |
| 168,266,886 | Standby callback `0x20156b34` handles `0x5d43` (`MAPI_MEA_AVAILABLE_DIR_IND`, sender `0x5d`, recipient `0x37`). |
| 221,844,072 | Handler `0x2118a010` receives `0x1f89` (`CYC_READ_PA_IDENT_CNF`) and sends HKBD message `0x2202`. |
| 229,191,262 | Real keyboard initialization callback `0x215a5f60` executes. |
| 229,207,297 | Standby handles `0x5b3b` (`MOBI_HECI_PHONE_OPEN_DETECTION_IND`, sender `0x5b`). |
| 229,218,051 | Standby enters UI-queue scan `0x20157978` with R0=`0x3f`. |
| 229,219,293 | Standby reaches its normal return at `0x20156c40`, without a new LCD frame. |

The 300M RAM snapshot confirms the HKBD transition: node `0x3197ee88`
contains `0, 0x20a8e874, 0x20e82eac, 0, 0`; its current state is Idle
(`0x20a8e874`). Globals at `0x301ce2bc` contain `0x110f0b00, 0x72, 0x16`.
The first two words of the process record at `0x3002a1ac` retain the last
dispatched tables; they must not be mistaken for the node's current state.
The standby node at `0x3197f3bc` still selects `0x21717f98`.

The next lead is the **empty UI callback queue**, not unscheduled HKBD/standby.
It has 20 entries of 0x1c bytes at `0x302d3b80..0x302d3daf`, with callback
pointer at entry+0x10 and suspended marker at +0x18. Both the pre-scan RAM
snapshot and the 300M snapshot are all zero. Flags at `0x3036c10c` are
`0x20000000`, so bit `0x100` is clear and standby takes the scan path. The
focused scan trace skips all 20 entries through `0x20157a84`; none can call
a UI callback. This does **not yet prove** why registrations are absent or
whether this queue must be populated to draw the main menu.

Candidate producer `0x215b8c50` inserts a queue record;
`0x215b8bd0` compacts the queue; `0x20587db0` releases/clears nonzero callback
fields. The initial `ui-registration-300m` write trace recorded only zero
writes, but exhausted its 240-write budget during the second standby scan.
It is not evidence that no later write or earlier transient registration occurred.

The rebuilt, RAM-read-guarded `ui-queue-guarded-300m` probe repeats the clean
300M result with a 6,000-write budget starting at instruction zero: **342 queue
writes, all zero**, with the budget not exhausted. The queue-insertion entry
`0x215b8c50` is never hit, nor is callback-release entry `0x20587db0` (seven
watch hits out of a budget of 60 cover the actual callbacks and returns).
At `0x20157d4a`, after the result load, both scans return R0=0:
instructions 168,274,308 and 229,219,162. Watching `0x20157d48` instead would
sample R0 before that load and misleadingly show 1. The guarded run preserves
the same instruction count, IRQ/clock totals, final registers and splash hash
as the previous 300M run. This establishes no observed registration in this
interval, not the ultimate reason the menu is absent.

Static decoding narrows the producer chain: the ARM veneer at `0x208b0330`
loads `0x215b8c51`, and its direct caller is `0x209a44fa`. This is case **7**
of the UI command interpreter at `0x209a4434` (R0 selects cases 0..8; R1 is
the argument). Veneers `0x2007e26c`, `0x20cb8b08` and `0x214dbd8c` lead to
that interpreter; their callers load the command from entry+6 and the argument
from `[0x30014df0+4]`. A separate `ui-registration-caller-300m` probe watches
the interpreter entry, case 7 and the insertion entry from instruction zero:
**zero hits**, with the 100-hit budget untouched and the same clean 300M
IRQ/clock/callback results. The next investigation must follow the upstream
UI action-table execution, not assume a failed call inside queue insertion.

Bounded, read-only task diagnostics are enabled by `E830_TRACE_TASKS`, with
`_AFTER` (default 0), `_LIMIT` (default 1000) and optional `_PCS` filtering.
Allowed PCs are `0x20847660,0x20847686,0x208476c4,0x20847724,0x20847d14,
0x204b01d8,0x208478f2,0x20847826`. The trace includes the current task,
wait object, state node, argument descriptor and registers. All pointer reads
are limited to ordinary guest RAM; diagnostics must not consume MMIO/FIFO data.
Two regression tests cover table-vs-code classification, node dereferencing,
trace limits, RAM bounds and unchanged guest memory. Native tests now pass
**63/63**; the release GUI and CLI are rebuilt. No board behavior or desktop
layout was changed by this diagnostic correction.

Evidence is local under `proof/native-windows`: `task-dispatch-180m`,
`standby-entry`, `hkbd-registration-180m`, `standby-dispatch-late-300m`,
`ui-init-dispatch-230m`, `standby-ui-gate`, `ui-registration-300m` and
`ui-queue-guarded-300m` and `ui-registration-caller-300m`.
Firmware, full RAM/NAND dumps and probe outputs remain excluded from public Git.

Current reproduction from the buildable Downloads tree:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\build-windows.ps1
& 'C:\Users\Administrator\AppData\Local\Programs\Python\Python312\python.exe' .\tools\run-probe.py `
  --env 'E830_DSP_BOOT_HLE=1' `
  --env 'E830_WATCH_PC=0x215a5f60,0x20156b34,0x20156c40' `
  --env 'E830_WATCH_LIMIT=20' verified-state-dispatch-300m -- `
  --onenand-image .\proof\native-windows\direct-call-side\tfs-v4\full-tfs-2k.onenand.bin `
  --max-instructions 300000000 --peek 0x309011f0 `
  --dump-lcd-frame .\proof\native-windows\verified-state-dispatch-300m.ppm
```

Use the installed 34,603,008-byte physical image with SHA-256
`d305c8e0b27e86ecec70534be1ee2c917c1556e675d5868573e600b7cf341a56`.
`firmware/E830XEGG1.onenand.bin` is erased/unformatted and is not a substitute.
Older `D:\CodexBuild\...` paths below identify historical evidence locations;
they do not exist in this Downloads workspace.

### Checkpoint — 2026-10-30: timer/IRQ lifecycle (earlier)

The measured periodic/W0C timer model has now been compiled and exercised with
the installed full-TFS OneNAND image. Native validation passes **61 tests**.
The first runtime attempt exposed an interrupt-controller lifecycle bug rather
than a timer-register error: after 463 delivered IRQs, IRQ20 had executed EOI
but the controller still considered it in service. `TMD_System_Clock` stopped
at 166 with IRQ20, IRQ21 and IRQ28 pending.

Focused lifecycle tracing shows that the affected IRQ20 entered with the RTOS
nesting byte at zero, executed the normal `0x208a1108` return path, and resumed
through `0x204b010c` after the scheduler switched to a task whose baseline
nesting byte is one. A normal IRQ21 uses the same return path but resumes at
`0x204b00e0` with nesting zero. Consequently, comparing the current RTOS
nesting byte with its value at interrupt entry was invalid: task switching may
legitimately change that byte before architectural IRQ return completes.

Controller service state now releases only after EOI has been observed and the
CPU is in a non-IRQ mode with CPSR.I clear. EOI alone still cannot open an early
nested-IRQ window. The emulator does not alter the RTOS nesting byte. A bounded
read-only diagnostic is available through `E830_TRACE_IRQ_SERVICE` and its
`_AFTER`/`_LIMIT` options.

The corrected 300-million-instruction run delivers 3,941 IRQs and advances
`TMD_System_Clock` to 1,035. The 1.2-billion-instruction run delivers **48,941
IRQs**, advances the clock to **12,285**, ends with no enabled pending IRQ and
has no memory fault. The main event pointer is `0x3049e8ec`, still below its
normal wrap threshold `0x304a0324`; `0x304a52e4` and `0x30033cfc` remain zero.
Thus the measured IRQ20/IRQ21 timer cadence drains the event stream without the
earlier buffer corruption or fatal reporter path.

This is substantial scheduler progress but not yet the menu transition. The
1.2-billion run still records one LCD frame. Its old keyboard/standby entry
counters were zero, but watched DEF data-table addresses rather than callback
code; that scheduling inference is superseded by the checkpoint above.
Its sampled PC is inside the genuine
RAM sleep stub at `0x31f0019c`. A separate final-5,000-instruction trace at
300 million contains 1,257 distinct PCs across RTOS work, timer callbacks and
the power/idle path, proving active scheduling rather than a single-instruction
hang. Genuine keyboard initialization and standby callback execution are now
confirmed; the next investigation follows their UI event/registration path.

### Checkpoint — 2026-10-29

The current source tree contains the DSP runtime queue lifecycle, internal-flash
command handling, serial identification controller and the new I2C/PMIC work.
The firmware and NAND images remain unmodified. Native validation passes **57
tests**, including a check that runtime writes to the CLA working image do not
alter its read-only mirror.

`src/i2c.rs` now models the PNX master controller instead of treating every
transaction as an absent slave:

- separate TX and RX FIFOs, FIFO levels and transmitted/received counters;
- START/STOP, write-one-to-clear status and the firmware-used interrupt bits;
- a PCF5060X-compatible register slave at seven-bit address `0x09`;
- sequential register-pointer reads and writes; all other addresses still NACK;
- an 8:1 ARM/peripheral-clock ratio. With firmware dividers 52/103, one byte
  takes about 11,300 ARM instruction slots instead of the earlier 1,400.

This progresses through the former `0x220604f1` failure. The previously failing
exchange is a read of PCF5060X register `0x05` after a successful read of
register `0x24`; it now completes through the normal IRQ28 driver path. PMIC
register `OOCS` defaults to the documented `BATOK|BACKOK|TEMPOK` bits, and the
RTC starts from the CLA build timestamp in BCD rather than an invented current
date. Useful evidence directories are `pcf5060x-v1`, `pcf-irq-lifecycle`,
`pcf5060x-timing8` and `pcf-valid-rtc` under
`D:\CodexBuild\SamsungE380-Emu`.

The INTC re-entry failure has also been resolved without changing guest RTOS
state. A per-line EOI now marks acknowledgement, but the controller remains in
service until the CPU has actually returned to a non-IRQ context with CPSR.I
clear. This prevents the generic dispatcher's early EOI from opening a nested
IRQ window, while still allowing normal delivery when the scheduler's own
nesting byte is one. No RTOS byte is seeded or cleared by the emulator. The
system timer uses the same 8:1 ARM/peripheral ratio as I2C and retains its
fractional remainder between updates.

The CLA execution window at `0x20000000` also contains firmware-owned writable
tables. A normal `str` at `0x21188d1a` updates the second word of the table at
`0x2023e284`; treating the whole window as read-only caused a false WRITE_PROT.
The working image is now writable, while the separate copy at `0xea000000`
remains read-only. At runtime `0x2023e288` becomes 5 and `0xea23e288` remains
zero, so the supplied CLA bytes are not modified.

DSP runtime command `0x2001` is now included in the opt-in E830 HLE. The original
routine clears bit 0 at `0x50505018`, sends `0x2001`, and waits for exactly one
bit-0 event; omitting that event produced report `0xee07018a`. The modeled event
removes that timeout without changing the mailbox payload or guest queues.

The latest full-TFS run reaches **600,000,000 instructions** with no memory
fault and no fatal loop. The former reports `0x220604f1`, `0x22030bf1`,
`0x220332f1` and `0xee07018a` are absent after their resolved call sites. The
ordinary instruction budget stops at PC `0x20848112`, inside active Nucleus
memory-management work. The final 5,001-instruction trace contains 644 distinct
PCs, so this is not a one-instruction trap. The run delivers 1,264 IRQs,
advances `TMD_System_Clock` at `0x309011f0` to 503 and produces four complete
LCD frames. The last frame is unchanged between 300M and 600M. The old
keyboard/standby counters remain zero, but their watched addresses were data
tables, not executable routines (corrected above). The main menu is unverified
from the LCD result, not from those invalid entry counters.
`0x30033cfc` also remains zero.

Evidence is in `working-image-300m`, `dsp-2001-300m`, `dsp-2001-600m` and
`late-active-600m` under `D:\CodexBuild\SamsungE380-Emu`.

Reproduction uses the installed NAND image
`D:\CodexBuild\SamsungE380-Emu\firmware-work\E830XEGG1.full-tfs-2k.onenand.bin`
with `E830_DSP_BOOT_HLE=1`. The erased Downloads copy
`E830XEGG1.onenand.bin` is not a valid replacement.

### Trace-buffer and IRQ21 investigation — 2026-10-30

The next clean-run failure occurs at instruction **855,125,069**: a
`READ_UNMAPPED` at `0xe24ee600`, PC `0x20847af4`, after report `0x133a0500`.
This is secondary damage, not a missing memory map. The firmware-owned event
stream pointer at `0x3001a684` has advanced beyond its intended buffer and the
writer at `0x2118f164` overwrites MP/radio state. The decisive writes are at
instructions 855,115,720/724: `0x000c` and `0x5051` are stored at
`0x304a52e4/0x304a52e6`. The later MP handler then sees a false ready message
whose body at `0x304a6da4` is zero; its reporter runs with task-ID sentinel
`0xff` and indexes a table outside its valid range.

The firmware's normal drain path is now identified. IRQ21 dispatches through
`0x20011b84`, clears timer status bit `0x100` at `0x50501008`, and calls the
runtime callback stored at `0x309a1150`. Initialization at `0x2098d03e`
registers callback `0x20143185`, so the actual drain starts at `0x20143184`.
Adjacent callbacks are `0x201430ed` at `0x309a114c` for IRQ20 and
`0x215a7d11` at `0x309a1154` for IRQ19.

The drain confirms these buffer layouts without patching guest state:

- main event stream: base `0x30492fd0`, normal wrap threshold `0x304a0324`,
  hard guard `0x304a1fd0`, write pointer at `0x3001a684`;
- auxiliary stream at `0x30491eda`, pointer `0x3001a688`, warning at 120
  halfwords;
- ring at `0x30491fca`, pointer `0x3001a68c`, maximum `0x7d0` halfwords;
- auxiliary stream at `0x30492f6a`, pointer `0x3001a690`, warning at 50
  halfwords;
- backend callback pointer `0x3001a26c` resolves at runtime to `0x20584c95`.

Timer writes around the first observed drain are also pinned down. Firmware
writes control `0x07e0` at `0x2098cf7c`; IRQ20 acknowledges it as `0x0760` at
`0x200117e6`; then `0x2098d594/0x2098d59a` program `+0x0c=0x1388` and
`+0x14=0x125c`; IRQ21 acknowledges with `0x06e0` at `0x20011b98`. This proves
that bits `0x80` and `0x100` have separate interrupt lifecycles. Their handler
write sequences also prove write-zero-to-clear behavior: IRQ20 writes bit 7 as
zero while preserving bit 8 as one, and IRQ21 does the inverse.

The timer cadence is now constrained directly from the trace. When
period `+0x0c=5000`, match0 `+0x10=0` and match1 `+0x14=4700` are programmed,
the counter is 586. The next
genuine IRQ21 appears 32,913 guest instructions later, matching
`(4700-586)*8 = 32,912` within the one-instruction hook boundary. IRQ20 follows
2,400 instructions later, exactly `(5000-4700)*8`. Thus the observed `0x600`
mode is a 5000-tick period with an early compare at 4700 and a period/reset
event at 5000. Before `0x2098cf7c` adds `0x600`, the control value is `0x01e0`;
bit `0x40` therefore predates this mode. Static driver decoding resolves the
fields: `0x20/0x40` enable IRQ20/IRQ21, `0x80/0x100` are their W0C statuses,
and `0x200/0x400` enable match0/match1. Firmware explicitly waits until the
counter is at most 4500 before setting `0x600`, so match1=4700 remains ahead
and match0=0 occurs at the next 5000-tick boundary. The previously observed
immediate IRQ20/IRQ21 entries were artifacts of treating status bits as normal
writable storage.

`src/machine.rs` now contains a candidate implementation limited to this exact
observed combination: periodic modulo-5000 counting, IRQ21/status `0x100` at
match1 `+0x14`, IRQ20/status `0x80` at match0 `+0x10=0`, and W0C status writes.
Static tests encode the measured 586→4700→0 sequence. The candidate is now
compiled and runtime-tested; the validated results and IRQ-service correction
are recorded in the 2026-10-30 checkpoint above.

Two diagnostic IRQ21 models were tested and then removed from the default
emulator. They reached the genuine drain and prevented the original overwrite,
but their cadence was wrong. One reached 1.2 billion instructions without a
memory fault and kept `0x304a52e4` zero, yet had already entered fatal loop
`0x20014b4c` after report `0x22060ff1`. The compare-channel variant preserved
four LCD frames but eventually let the first auxiliary stream reach 120 words,
reported `0xe70204f0`, and its secondary reporter again accessed
`0xe24ee600`. Therefore neither experiment is retained as hardware behavior.

Evidence is under `D:\CodexBuild\SamsungE380-Emu` in `mp-buffer-writes`,
`invalid-index-entry`, `timer-match-irq21-v2`, `timer-match-irq21-1200m`,
`irq21-post-drain-report`, `error-22060ff1-source`,
`timer-compare-dual-300m` and `timer-control-before-irq21`. The external
`research/seftool` checkout contains genuine PNX5230 loader binaries and names
`pd_timer.c`, but no source-level timer register definitions were found.

### Timer and DSP progress — 2026-10-27

The 16-bit counter at `0x50501000` now advances with guest instruction progress,
not counter reads. On rollover, control bit `0x20` enables status bit `0x80` and
IRQ20; peripheral acknowledgement clears the pending level, not INTC EOI alone.
The older conclusion that bit `0x40` copied `+0x14` into `+0x0c` was wrong.
Firmware helpers independently use `0x20/0x40` to enable IRQ20/IRQ21, while
`0x80/0x100` are their W0C status bits. The observed `0x600` match mode and
measured 4700/5000 cadence are described above. This remains a provisional
instruction-based model; physical clock frequency and unobserved modes remain
unverified.

The old `timer-period-latch` result, where `+0x0c` became 4700, came from the
emulator's removed copy-on-rollover behavior and is superseded. Its IRQ count
and `TMD_System_Clock=605` are retained only as historical evidence, not as
validation of the corrected match-channel model.

The DSP boot interface at `0x60100000` now provides TX/RX FIFO status, the boot
signature response, uploads and readback in two independent memory spaces.
The ARM loader uploads `0x6000` halfwords to space 0 at address zero and `0x1000`
halfwords to space 1 at `0x7000`, then sends command `3` and start address `0`.
The parser leaves the boot protocol after this pair; later runtime words are
not interpreted as boot-ROM readback commands. No runtime response is fabricated.

The focused local `bp-dsp-start-signal` probe stops before `0x2098cddc` at
instruction **141,217,692**, with `memory_fault: None` and the genuine splash.
The ARM then polls bit 0 of the halfword at `0x50505010`. Its following code
acknowledges that bit, sends runtime command `0x4001`, and polls `0x50505018`.
RD16024 execution and the DSP-side signaling operation are not implemented, so
the signal bit must not simply be forced to bypass this gate.

Native validation on 2026-10-27: `build-windows.ps1 -Test` passes **48 tests**,
including timer rollover/acknowledgement, DSP upload/readback and the transition
from boot-ROM commands to runtime mode. Firmware, probes, full dumps and vendor
tools remain local and are excluded from the public repository.

### Later DSP runtime progress — 2026-10-28

The opt-in `E830_DSP_BOOT_HLE` path now recognizes the exact uploaded E830 DSP
image before providing any runtime behavior. It preserves runtime readback and
models the observed polling completions for commands `0x4001`, `0x1001` and
`0x1002`. Command `0x1001` has two observed bit-0 events on `0x50505018`; the
second follows the ARM acknowledgement of the first. Command `0x1002` has one
observed bit-0 completion. The default path still fabricates no runtime DSP
response.

With `0x1002` acknowledged, execution advances past the previous
`0xe903008a` timeout and repeatedly submits asynchronous DSP work. Four 64-byte
entries at `0x304917ec`, `0x3049182c`, `0x3049186c` and `0x304918ac` fill with
callback `0x215a7cb9`. The next submission reports `0xeb020002` at
`0x2118d47a`, meaning no free entry. Callback `0x215a7cb8` normally clears
`entry+0x10`, but no genuine DSP completion dispatch reaches it in the current
model. Clearing those words directly would seed firmware state and is not an
acceptable fix.

The report is not an immediate stop. A clean run without a breakpoint continues
to **1,200,000,000** instructions without a memory fault, but reaches the same
fatal firmware trap at `0x20014b4c` by instruction **147,668,760** and remains
there. Dynamic PC tracing gives the exact path
`0x2001459c -> 0x20014686 -> 0x200149a2 -> 0x20014b3e -> 0x20014b4c`.
The reporter passes `0xeb020002`; its later lookup of ID `0x0f72` resolves to
RAM word `0x30033cfc`, which remains zero, and the firmware selects the fatal
path. A focused write trace shows this word is only initialized to erased
`0xffffffff` and later cleared with its table; it receives no successful
runtime registration. Thus the missing asynchronous completion remains the
causal blocker even though the initial report itself is non-fatal.

At the `0x1002` transmit point, the relevant shared mailbox halfwords are at
`0x600006bc` (`0xc8d4`), `0x600006be` (request-dependent values including
`0x1800`, `0x2000`, `0x2800`, `0x4000` and `0x4800`) and `0x600006c0`
(request-dependent payload). This confirms that repeated `0x1002` words are
distinct requests, not a single polling command to acknowledge indefinitely.

The physical interrupt table at `0x309a117c` confirms IRQ20 is the system timer
(`0x200117d5`). IRQ19 dispatches through `0x20011b75` to a dynamic callback,
and IRQ21 through `0x20011b85`; both are masked at the slot-exhaustion point.
IRQ22 is enabled but belongs to a generic UART/DMA-style device context at
`0x309a2d0c`, not to DSP completion. A diagnostic IRQ22 experiment invoked
`0x20521265`, caused a level interrupt storm and eventually reported
`0x22018052`; it was removed from the model. No forced interrupt remains.

Native validation on 2026-10-28: `build-windows.ps1 -Test` passes **52 tests**.
The focused evidence is stored only in excluded local run directories including
`post-1002-tx`, `slot-exhaustion`, `dsp-mailbox-1002`, `late-failure-path`,
`trace-0f72-writes` and `post-1002-long-clean`.

### Complete TFS image and current retail mount proof

The examined Clifford VE / XSR2 loader created physical metadata and installed
2099 files, 127 directories and 26,268,942 payload bytes. Each file was read back
in a fresh loader process, compared with the source and checked for exact EOF.
The physical image is `proof/native-windows/direct-call-side/tfs-v4/full-tfs-2k.onenand.bin`,
34,603,008 bytes, SHA-256
`d305c8e0b27e86ecec70534be1ee2c917c1556e675d5868573e600b7cf341a56`.
See that folder's README and verification JSON for complete reproduction.

The original FAT formatter's fourth argument selects sectors per cluster.
Four sectors (2 KiB) fit the complete package in the 32 MiB device; the default
eight-sector clusters exceed available logical space. No physical XSR metadata
or file data was manufactured by the host. The factory cluster size is unverified.

`full-tfs-v4-retail-mount.mmio` confirms `r0=0` at `0x20d16b9a`,
`0x21134ea8`, `0x20a00744` and `0x2112a362`; `0x2112a36a` has `r4=0`.
The retail firmware therefore completes this filesystem initialization path.
At 120,000,000 instructions it is still executing the power/idle path, without
a memory fault and with one unchanged Samsung splash. Menu entry is unverified.

`full-tfs-v4-wake-after-mount` stops before `0x2012179c` at 71,267,120
instructions, after the successful mount. INTC mask `0x50302010=0x7fffffff`
leaves only IRQ31 enabled. `TMD_System_Clock` remains zero and power register
`0x50500004=0x02041099`. The guest flags at `0x304a20f4` are `0x4fe9fffe`,
including bit 10. The instruction at `0x201217bc` therefore skips the diagnostic
report at `0x201217be`; `0x201217b0` was only a sampling point, not a wait.

Focused `wake-flow-1` tracing shows that `0x2012179c..0x201217d6` completes and
the firmware repeats the surrounding sleep/idle cycle. Consecutive entries are
about 441 guest instructions apart. The cycle masks all but IRQ31, calls the RAM
sleep stub at `0x31f00140`, restores the previous mask and returns to scheduler
work. No IRQ31 or wake-status event occurs in the model; the 120 interrupts in
the report belong to the earlier I2C phase. This is an active repeated idle path,
not a firmware hang at one instruction.

The RAM block `0x31f00000..0x31f001ac` is an exact copy of CLA bytes
`0x2189a4f4..0x2189a6a0`. It contains ARM sleep stubs at `0x31f00090` and
`0x31f00140`; both set bit `0x400` in the halfword at `0x50502000`, and the
second first performs cache/prefetch drain operations. Veneers at `0x200827a8`
and `0x204bd074` target `0x31f00140`; `0x204bd060` targets `0x31f00090`.
Read-only trace `wake-stub-copy-3` observes the complete copy at instructions
53,652,746..53,652,831: the bulk stores execute at `0x204af708/0x204af710`,
with final stores at `0x204af72c/0x204af73c`. Thus the RAM code is installed by
the retail firmware during initialization; it is not seeded by the emulator.

The IRQ31 dispatch data is also now resolved without raising the interrupt.
The vector `0x20cb0cd0` selects generic handler `0x208a0fd8` with context
`0x309a2d9c` (`line=31`, controller base `0x50502000`). Its Thumb dispatcher
`0x20010a40` indexes the 20-byte entry at `0x309a13e8`, calls `0x200140cc`,
then the ARM veneer at `0x200107e8` reaches callback `0x20d729c0`. The word at
`0x200107f0` is that veneer's literal, not another veneer. Runtime table words
match the static table at `0x218c3508`. This establishes the callback chain but
not the missing wake source, status lifecycle, or semantics of `0x50502010`,
`0x50502014`, `0x50501000` and `0x50501008`; forcing IRQ31 is not justified.

Cold STL recovery previously rejected the headers because main-area ECC was
absent. `0x3000d5cc` examines OOB bytes 8..10; programmed headers deliberately
have odd parity and require paired Hamming bits. The device now computes main
ECC from the selected 512-byte BufferRAM sector during command 0x80 unless
System Configuration bit 8 disables ECC. Spare-only programming preserves that
code. The independent parity implementation uses SMC byte order; the exact
chip's byte ordering, spare ECC and read-time correction still need validation.
The Linux OneNAND 1 KiB OOB layout confirms the main ECC area within bytes 8..12;
reference copies stay under excluded proof.

The UART protocol was also corrected by examining ARM dispatch and its original
filesystem calls: DB creates a directory, EE unlinks a file, E5 unmounts and
returns E4. E4 itself consumes a 16-byte version-file payload; it is not unmount.
DD starts a file and FE delivers its contents; successful completion returns DF.

The older failure probes below describe erased media and superseded experiments.

Reproduce without debug bridges:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\build-windows.ps1
& 'C:\Users\Administrator\AppData\Local\Programs\Python\Python312\python.exe' .\tools\run-probe.py --env 'E830_WATCH_PC=0x205280a0,0x20d16b9a,0x21134ea8' nand32-boot -- --max-instructions 80000000 --break-at 0x200141c0 --break-after 60000000 --peek 0x4801e002 --peek 0x301ad1b0 --peek 0x309011f0 --dump-lcd-frame .\proof\native-windows\nand32-boot.ppm
```

`--break-after 60000000` matters: the same error-reporting entry is called earlier
with a different severity. Do not confuse that earlier call with the filesystem
failure. Instruction counts include fast-forwarded pure software loops.

## Filesystem failure: confirmed call chain

| Address | Observation |
| --- | --- |
| `0x2112a338` | Calls `0x21134e88`; a negative result becomes `0x18000002`. |
| `0x21134e88` | Builds the filesystem callbacks, then calls the veneer `0x210be5f0`. |
| `0x20dfec78` | Target of that veneer; stores the lower-layer result at `0x3017c118`. |
| `0x212152f4` | Initialization using callbacks `0x20d30f61` and `0x20d30f91`. |
| `0x20d30f60` | Initializes BML successfully. |
| `0x20d30f90` | Calls `0x20d16ad4` to open the BML volume. |
| `0x20d16b9a` | Observed return `r0=0x800b0000` from `0x214f2590`. |
| `0x214f25f0` | Calls `0x21550bc0` to mount the reserved area; return is zero. |
| `0x214f2616` | Logs `[BIF:ERR] nVol:%d is BML unformatted`. |
| `0x21134ea8` | Wrapper returns `0xffffffff`; stored lower-layer result is `0xfffffffb`. |
| `0x200141c0` | Final report: `r0=1`, `r1=0x18000002`, `lr=0x2112a37b`. |

After correcting NAND to the user's specified **32 MiB**, the final stop is at
61,362,832 instructions: PC `0x200141c0`, CPSR `0x3f`, r0=1, r1=`0x18000002`,
LR `0x2112a37b`. There are 120 I2C interrupts, one real Samsung splash frame and
no memory fault. Proof: `nand32-boot.log`, `.mmio`, `.ppm` and `.png`.
The earlier `fs-final.*` and `nor-nsf-boot.*` probes used the superseded 64 MiB
profile. Correct capacity does not resolve the missing filesystem metadata.

The OneNAND driver starts reading reserved blocks `0x1f2` (498) onward. The
backing begins erased unless an explicit physical dump is supplied. Correct
read completion alone cannot provide the absent reserved-area metadata.

## NOR, NSF and the retail TFS package

The earlier mapping of the entire TFS package at `0x21f40000` was unjustified.
Reading those bytes did not demonstrate a filesystem mount. It also put resource
bytes into a NOR region used by another partition.

The supplied NSF file has a 20-byte NF4 wrapper and a `0x3e0000`-byte payload.
Its payload block 13, offset `0x1a0000`, starts with `SMPB`. The firmware's NOR
partition setup at `0x205e19be` supplies base `0x21dc0000`, and its nearby table
also supplies `0x21e00000` and `0x21f40000`. Together these observations support
placing the NSF payload at `0x21c20000`: the existing `SMPB` bytes then appear at
`0x21dc0000` without fabricating a signature or modifying the input.

Current mappings:

| Input/window | Meaning |
| --- | --- |
| CLA at `0x20000000` | Firmware code; boot aliases remain inherited. |
| NOR at `0x21c00000..0x22000000` | Four MiB data area, initialized erased. |
| NSF payload at `0x21c20000` | Original supplied NOR bytes, without NF4 wrapper. |
| TFS at `0x1c000000` | Provisional diagnostic installer source, not a mounted filesystem. |
| OneNAND at `0x48000000..0x48020000` | Device register and BufferRAM window. |
| `0x60000000..0x60010000` | Zero-filled shared DSP data storage. |
| `0x60100000..0x60500000` | Provisional auxiliary DSP RAM, no longer an NSF alias. |

The retail TFS file is a concatenation of file contents in the external CFG's
order; the exact byte/count checks below establish this for this supplied pair.
It is not a complete physical NAND dump including BML reserved blocks and spare
data. It must not be copied blindly into physical NAND or NOR.
Installing TFS using an understood original loader/formatting path remains work
in progress. The source window and auxiliary DSP RAM sizes still need hardware
verification; mapping storage does not implement DSP execution.

### Retail package investigation with CLA/NSF/TFS only

The user has no full NAND capture. Continue investigating installation from the
supplied package rather than making a capture a prerequisite.

The external `E830XEGG1.cfg` declares `TFS4.0_CORONA_01`, 127 directories and
2099 files. Parsing each `FILE_NAME`/`FILE_SIZE` pair and accumulating sizes gives
exactly **26,268,942 bytes (`0x190d50e`)**, equal to the complete TFS file size.
There is no trailing metadata after these contents. The first files verify the
concatenation: JPEG headers appear at offsets `0`, `0x28f`, `0x685`, `0x9e0`,
`0xddf` and `0x1130`, matching the first six CFG sizes. The last file,
`/vobject/vnote_mng_backup.bak`, starts at `0x190b8e2` and occupies 7212 bytes.

The text previously suspected to be filesystem metadata near `0x1853a08` is
actually the resource **`/test/cfg.txt`**, starting at `0x1853a04`, size 150639.
That embedded configuration is an earlier version: it lists 2098 files, omits
its own `/test/cfg.txt` entry and sums to 26,118,303 bytes. The external CFG has
the inserted entry and agrees exactly with the actual package. Do not use the
embedded list unchanged: it shifts all subsequent file offsets.

The public `bhmsgame06/sflasher` `src/tfs.c` reference's `patch_start` also walks
the CFG and advances the package pointer by each file size. Its serialized
TFS4 sector/control format targets a different Samsung filesystem and is not
evidence for the E830's NAND format. E830 initialization names `KFATFS` and
passes through XSR2 BML/STL; resource extraction is not filesystem installation.

Confirmed E830 reservoir facts from the untouched CLA and a native probe:

- The pool signatures at `0x206ad654` and `0x206ad660` are eight-byte
  **`ULOCKPCH`** and **`LOCKPCHD`**, respectively. Modern Samsung FSR headers use
  different four-byte signatures; their layouts must not be transplanted.
- `0x205280a0` searches pool blocks, tries sector offsets 14, 10 and 6, then
  reads the first sector and compares its eight-byte signature. It requires
  words at offsets `0x0c` and `0x10` to be zero and checks 16-bit ages at `+8`.
- `0x20528728` is an original pool-header writer: it fills a 512-byte buffer
  with erased bytes, copies the appropriate signature, increments the age,
  copies the other pool block at `+0x0a`, and clears `+0x0c/+0x10`. This is a
  low-level helper, not yet an identified complete formatting entry.
- Probe `tfs-reservoir-entry.*` stops naturally at `0x205280a0`, instruction
  61,326,249, with `r0=0x30aea678` (device context), `r1=0x301ac120` (volume),
  `r2=0`, 120 I2C IRQs and one LCD frame. The initialized geometry gives 512
  physical blocks and reservoir start 496; the scan begins at 498.

That earlier entry probe used the old 64 MiB profile. In the corrected 32 MiB
profile, `nand32-geometry.*` verifies the same 512 physical blocks and reservoir
start 496, but **128 sectors/block** rather than 256. The firmware derives 64
pages/block and two sectors/page from the `ec/14` device profile itself.

At the time of the reservoir-entry probes no formatter or installer had been
executed. Subsequent original-loader formatting and full TFS installation are
documented above; the earlier BML error is no longer the current boot blocker.
Reference copies remain under excluded `proof/native-windows` and are not part
of the distributed implementation.

### Created NAND image and extracted installer resources

At the user's request, `tools/prepare_nand.py` now creates erased physical media
for the current profile. The local `firmware/E830XEGG1.onenand.bin` has 34,603,008
bytes: 33,554,432 main bytes plus 1,048,576 spare bytes, interleaved 1024+32 per
page. Every byte is `0xff`. SHA-256:
`f89b94be7663558d06728e10f6488a145fad48c46fff36fd36c4924ea2e8dbcd`.
The old GUI already discovers this name beside the CLA; no interface code was
changed. The image is erased/unformatted, not a reconstructed phone dump.

The same tool validates the external CFG's sections, file/directory counts,
paths and exact payload length before writing output. It never overwrites an
existing NAND image or staging directory. It extracts the 2099 original files
and 127 directories into `firmware/E830XEGG1.tfs-staging/files`, with a JSON
manifest recording source hashes, offsets and per-file SHA-256 values. Independent
verification of all files, directories and hashes, and concatenation back to
the complete original TFS, passed. Five Python tests cover extraction boundaries,
empty entries, malformed paths/counts, truncated/extra payloads and preservation
of existing outputs. Run `python -B tools/test_prepare_nand.py`.

An explicit native CLI boot with `--onenand-image` accepts all 34,603,008 bytes
and reproduces the existing stop at 61,362,832 instructions, PC `0x200141c0`,
r1 `0x18000002`, lower BML error `0x800b0000`, 120 I2C interrupts and one Samsung
splash frame, without a memory fault. Evidence: `nand32-created-boot.log`,
`.mmio`, `.ppm`, `.png` and `nand32-created-verification.json` under excluded
`proof/native-windows`. Creating erased media and extracting resources does
not format BML/STL/KFATFS or install TFS; the main menu remains unreached.

`src/nor.rs` now handles the observed x16 AA/55/25/count/data/29 write-buffer
sequence, A0 word program and sector erase. In the real trace, the first data
word can be written at the command address itself. The old implementation
incorrectly started at command address+2. Unlock/reset commands no longer change
array cells, and ordinary writes cannot program cells without a command.
Erase geometry (128 KiB), chip identification and immediate completion timing
remain provisional.

## OneNAND model and physical dumps

The user specifies **32 MiB NAND**. `src/onenand.rs` now uses Samsung ID `ec/14`,
version zero, present in the firmware's supported-device table. This selects
512 blocks, 64 pages/block, **1024 main bytes and 32 spare bytes/page**. The
exact fitted chip model is still unverified; `ec/24`/64 MiB was an earlier
provisional selection and has been replaced.

This must not be implemented merely by halving the old block count. Original
driver `0x211301b0` handles device density 1 by setting two sectors/page, sector
address shift 1, and seven sector-address bits per block. `0x20510754` reports
512 blocks and 64 pages/block to BML. `nand32-geometry.*` confirms the firmware's
own volume context: blocks=512, reserved blocks=16, sectors/block=128,
pages/block=64, reservoir start=496, driver sectors/page=2. The 32 MiB correction
is in the device model; the CLA and driver table are untouched.

Important registers: manufacturer/device at `+0x1e000/+0x1e002`, block at
`+0x1e200`, page+sector at `+0x1e20e`, BufferRAM selector/count at `+0x1e400`,
command at `+0x1e440`, status at `+0x1e480`, completion at `+0x1e482`.
Data buffers begin at `+0x400/+0x800`; spare buffers at `+0x10020/+0x10040`.

Read/program respect selected sectors and count (encoded zero means two).
In this density profile SA8 bit 0 selects sector 1; page remains in bits 7:2.
Retail `0x20d27350..64` encodes `((sector << 1) & 0xfc) | (sector & 1)`;
the driver's shift value applies to the page field, not the sector selector.
SB sector selection stays in bits 9:8 and bank selection in bit 10. The earlier
four-sector interpretation belongs to the retired 64 MiB profile. Spare-only
reads preserve main BufferRAM data. Transfers across a
page boundary, spare ECC, read-time correction, accurate timings and detailed
locks are not implemented. Main ECC generation is described above. ECC status
registers reset to zero; no bit errors are injected.

CLI `--onenand-image` accepts:

- 33,554,432 bytes: physical main-only array, spare stays erased.
- 34,603,008 bytes: 1024 main +32 spare bytes interleaved per physical page.

Guest writes and erase use an in-memory overlay and never overwrite the source
file. Main-only data may be insufficient when BML requires spare metadata. The
GUI uses the same backend and optionally finds `CLA-stem.onenand.bin` beside the
selected CLA; the existing file-picker layout stays unchanged. Actual boot with
a real full dump is still unverified. Other dump geometries/OOB formats must be
identified before loading, not padded or shifted until a signature appears.
The previous 64 MiB input formats are now rejected. All 39 native Windows tests
pass, including last-sector addressing, page-boundary rejection and physical
main/spare image loading. Release binaries have been rebuilt with this profile.

## Earlier boot blockers now resolved

- ARM946 TCMTR `0x00140140`: 16 KiB ITCM at zero and 16 KiB DTCM at
  `0x50600000`. CP15 TCM accesses are modeled without firmware patches.
- UART0/1 at `0x50403000/0x50404000`: ready transmitter, LSR `0x60`, DLAB
  divisor support and bounded serial output. Empty input is not invented data.
- `0x50405000`, IRQ 28 is I2C, not ADC. The controller follows the documented
  PNX I2C register/FIFO shape. Reference:
  https://github.com/torvalds/linux/blob/master/drivers/i2c/busses/i2c-pnx.c
- Firmware sends `0x112`: START, slave `0x09`, write. The current model attaches
  a PCF5060X-compatible register device at that address, including RX FIFO
  delivery and the measured peripheral-clock scaling. Unknown addresses NACK.
- Adding shared DSP storage resolved the access at `0x60000008` from
  `0x20d72b88`; adding OneNAND resolved the ID read at `0x4801e000` from
  `0x2112fada`. Neither memory fault is the present filesystem failure.

The GPL reference copy under `proof/native-windows` is research-only and is not
part of source/binary distribution. Current device implementation is separate.

## Interrupts and clocks: corrected findings

IRQ 29 is DMA: its callback at `0x208c8244` reads status at `0x50100000`.
`0x30900168` is not the system clock: byte `0x30900169` tracks nesting depth.
The actual RTOS clock is at `0x309011f0`. It was zero in the earlier mount/wake
probes; the later timer-enabled probe observes 605 as described above.

Normal tick path: `0x200117d4` -> `0x20848732`. IRQ20 has callback
`0x200117d5`, but its generic handler is still zero and line20 is masked at the
filesystem stop. Sleep elapsed-time path reaches `0x21490438`. IRQ31 callback
`0x20d729c0` sets byte `0x304a20d5`. Low FIQ vector at `0x100` currently calls
`0x20011b71`, a `bx lr` stub. None proves a running frame/tick timer.

INTC: `0x50302000` is service line*4, `+0x1c` is the accepted-source latch
until EOI (zero outside accepted service), `+0x14` is pending and not masked,
`+0x10` uses 1=masked. EOI plus architectural return releases service state;
device status persists until the device is acknowledged. Concurrent sources
currently choose the highest bit; actual priority register semantics are TODO.
IRQ entry respects CPSR.I. Force IRQ, SEED_NUCLEUS and CLOCK_ACCEL bridges have
been removed. The opt-in E830_TIMER_PERIOD/E830_TIMER_IRQ source is diagnostic,
not a validated hardware timer. The counter at `0x50501000` now advances with
guest instruction progress. The candidate model now covers the observed W0C
status and 4700/5000 match cycle; accurate physical timing and other modes still
need verification.

## Next work

1. Follow the original **Insert SIM / SOS** state and its genuine input/SIM
   transition toward the menu. LCD partial-window addressing is now runtime
   verified; UART0 TX DMA releases SL3 through real IRQ29. No queue bypass or
   synthesized SIM-card event is justified.
2. Establish the real SIM/UI transition and earliest missing event, and trace
   the original insert_sim IFG load/decode path for the colour anomaly already
   present in guest graphics RAM. A sampled
   idle PC is not a hang: the bounded trace shows repeated returns to RTOS work.
   Raw clock rates, additional DMA/LCD modes and wake behavior remain unverified.
3. Keep PCF5060X status/RTC and DSP completions evidence-based; do not invent
   power events or mailbox results merely to select a successful branch.
4. Preserve the real firmware, NAND and RTOS state; no forced IRQ, direct state
   seeding, fake menu or firmware patch is permitted.

Keep the real firmware untouched, preserve the desktop interface, and record
new evidence here. A missing filesystem must not be bypassed with RTOS state,
manual clock changes, a fake menu or fabricated success returns.
