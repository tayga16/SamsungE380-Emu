# E830: current native Windows boot investigation

Updated 2026-10-27. This file describes the standalone Rust/Unicorn model.
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

### Timer and DSP progress — 2026-10-27

The 16-bit counter at `0x50501000` now advances with guest instruction progress,
not counter reads. On rollover, control bit `0x20` enables status bit `0x80` and
IRQ20; peripheral acknowledgement clears the pending level, not INTC EOI alone.
With control bit `0x40`, rollover copies the programmed period at `+0x14` to
the active period at `+0x0c`. The original firmware's delay bookkeeping requires
this transition. This is a provisional instruction-based time model; physical
clock frequency and complete timer semantics remain unverified.

Local `timer-period-latch` and `long-boot-after-timer` probes observe active and
next periods of 4700, 1336 interrupts taken, and `TMD_System_Clock=605` in the
latter probe. These results supersede the earlier zero-clock observation; they
do not establish correct real-time behavior or a working menu.

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
- Firmware sends `0x112`: START, slave `0x09`, write. With no slave attached the
  model reports NACK after transfer time; firmware ISR `0x20cec930` clears its
  own wait flag at `0x31f48ef0`. This is not a successful PMIC emulation.
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
guest instruction progress, with provisional rollover and period-latch behavior
described above; accurate physical timing still needs verification.

## Next work

1. Identify documented RD16024 instruction encodings and DSP-side startup signaling.
2. Implement only confirmed DSP behavior, then probe beyond `0x2098cddc`.
3. Validate retail resource reads and later menu gates without firmware patches.
4. Complete physical timer timing, interrupt priorities and I2C slave `0x09`.

Keep the real firmware untouched, preserve the desktop interface, and record
new evidence here. A missing filesystem must not be bypassed with RTOS state,
manual clock changes, a fake menu or fabricated success returns.
