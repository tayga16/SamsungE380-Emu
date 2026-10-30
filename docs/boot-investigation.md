# E830: current native Windows boot investigation

Updated 2026-10-30. This file describes the standalone Rust/Unicorn model.
The old QEMU, forced-IRQ and clock-acceleration notes are archived in
[boot-investigation-legacy.md](boot-investigation-legacy.md); they contain
superseded conclusions and must not guide the current board implementation.
The desktop layout in `src/app.rs` has not been changed in this continuation.

## Current outcome

The original E830XEGG1 firmware draws its SGH-E830/Samsung splash, creates the
RTOS system stack and unmasks IRQs itself. No main menu is reached. A complete
TFS image is now installed and verified through a fresh original loader process.
Retail E830 BML initialization and FAT mounting also succeed on that image.
With the timer and DSP boot interface modeled, execution reaches the DSP-start
handshake described below. The splash remains; RD16024 execution is still absent.
Neither mounting nor a splash proves a phone boot.

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
LCD frames. The last frame is unchanged between 300M and 600M. Keyboard-init
and standby-look counters remain zero, so the main menu is still unverified.
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
Static tests encode the measured 586→4700→0 sequence. Per the user's request,
this candidate has not yet been compiled or runtime-tested.

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

INTC: `0x50302000` is service line*4, `+0x1c` is pending, `+0x14` is pending
and not masked, `+0x10` uses 1=masked. EOI releases controller service state;
device status persists until the device is acknowledged. Concurrent sources
currently choose the highest bit; actual priority register semantics are TODO.
IRQ entry respects CPSR.I. Force IRQ, SEED_NUCLEUS and CLOCK_ACCEL bridges have
been removed. The opt-in E830_TIMER_PERIOD/E830_TIMER_IRQ source is diagnostic,
not a validated hardware timer. The counter at `0x50501000` now advances with
guest instruction progress. The candidate model now covers the observed W0C
status and 4700/5000 match cycle; accurate physical timing and other modes still
need verification.

## Next work

1. Compile and run the candidate periodic/W0C timer model when builds are next
   allowed; first run its focused unit tests, then stop at `0x20143184`.
2. Keep IRQ21 only if its 4700/5000 cadence reproduces the `0x20143184` drain
   without `0xe70204f0`, `0x22060ff1` or a secondary reporter fault.
3. Repeat the 1.2-billion-instruction run and verify that `0x3001a684` remains
   below `0x304a0324`, `0x304a52e4` stays untouched, and four LCD frames remain.
4. Keep PCF5060X status/RTC and DSP completions evidence-based; do not invent
   power events or mailbox results merely to select a successful branch.
5. Preserve the real firmware, NAND and RTOS state; no forced IRQ, direct state
   seeding, fake menu or firmware patch is permitted.

Keep the real firmware untouched, preserve the desktop interface, and record
new evidence here. A missing filesystem must not be bypassed with RTOS state,
manual clock changes, a fake menu or fabricated success returns.
