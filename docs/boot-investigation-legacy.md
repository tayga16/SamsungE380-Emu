# Historical E830 investigation

Imported notes from the previous project. Some early hypotheses are disproved by later findings; the latest QEMU IRQ observations at the end take precedence. Paths and old frontend descriptions may describe the original tree. The standalone desktop frontend uses the Rust model in src/.

## 15. Experimental Samsung SGH-E830 target

The E830 path is a separate device model, not a WinCE application: `.cla`
files must never go through `pocket-pe`, `pocket-kernel`, or the WinCE API
HLE. `src` owns firmware validation, the bare-metal machine
and its provisional memory/peripheral model; the `run-e830` CLI command is
only a headless boot probe.

The desktop launcher and Android app also offer a GUI path: select `.cla` + `.nsf` and optionally `.tfs`; both frontends call the same `pocket-e830` machine and display its 176×220 LCD snapshots. Do not route these files through `pocket-core` or WinCE HLE. The E830 screen deliberately has no virtual WinCE gamepad: phone keypad registers are not yet emulated.

Reuse `cpu::Arm946Cpu` for instruction execution. The board
model adds its own device state and uses the exposed raw engine only for
memory maps, MMIO callbacks, hooks, and register setup. Do not implement a
second CPU engine. The SoC is labeled PCF5230 in the E830 service manual;
ARM946E-S is the current Unicorn execution profile, not a verified claim about
the silicon core.

The `.cla` reset vector is at file offset `0x15c`; its image is mapped at
`0x20000000`. `.nsf` files with the observed `NF4` container header have a
20-byte wrapper removed before mapping EEPROM at `0x60100000`. A matching
`.tfs` package is staged read/write at `0x1c000000`; firmware flash commands
write to a provisional erased flash backing range beginning at `0x21f40000`
(minimum 32 MiB). This is not an accurate OneNAND implementation. Keep
supplied firmware out of the repository and never commit it.

The initial model asserts bit 9 of `0x50303050` as a virtual boot-ready GPIO
because the reset path polls that line before continuing. The `0x44000000`
external-control window has a minimal register/data-bus interpretation that
captures 176×220 LCD writes; its controller registers and timing are not yet
verified against hardware. Keep both explicitly provisional until matched to
a working device trace.

The current model is explicitly incomplete: CPU execution, basic RAM,
EEPROM, selected registers, a provisional flash window, and a minimal LCD
write-capture path are present; the baseband DSP, accurate OneNAND
timing/controller, radio, audio, keypad, interrupt sources, and most
peripherals are not. The supplied E830 image has only been verified to draw
its Samsung boot splash; reaching the phone's main menu is still unverified.
An instruction-budget stop proves only that guest code executed that far.
Keep the bounded budget, MMIO trace, and `--peek` inspection for reproducible
probes.

### Boot progress and what currently blocks the main menu

The `E830XEGG1` image draws its full Samsung SGH-E830 colour splash
(`SGH-E830 / WWW.SAMSUNGMOBILE.COM / SAMSUNG`) — proof in
`proof/samsung-e830-boot-splash.png`. After the splash the firmware settles
into its LCD refresh loop (the routine at `0x2009f05c` writes the
`0x44000002` LCD data register) and keeps redrawing the same splash while
busy-waiting in the counting-delay helper at `0x20cd7a74` / `0x20cd7a88`.
The delays are huge (≈6M iterations each) and are what the fast-forwarder
skips; across a 1.5B instruction budget the firmware stays in this loop and
never swaps the displayed screen to the menu. The splash-to-menu transition
is **not** interrupt-driven at this stage: the core runs with the CPSR
I-bit set (IRQs masked) throughout, so the gate is some boot-completion /
polled condition the model does not yet satisfy, not a missing tick. The
1024× flash-command bursts to the `0x21c00000` external-flash window
(unlock writes at `0x2115a0da/e0/e4`, zero reads) are a firmware-side flash
format pass, not a poll we are failing to answer.

### Interrupt controller map (from the IRQ vector at 0x20cb0cd0)

The ARM IRQ vector (`0x20000018` → literal at `0x20000164` = `0x20cb0cd0`)
reads the pending line number as a byte at controller base `0x50302000`
(already pre-multiplied by 4), filters it against the pending bitmask at
`+0x1c`, dispatches through a RAM table at `0x30a2c854`
(`[entry]=handler fn`, `[entry+4]=arg`), then acknowledges by writing `0x80`
to the EOI register at `+0x08`. A periodic timer-tick IRQ model built on
this is present but **off by default** (`DEFAULT_TIMER_IRQ_PERIOD` = 0,
line 29). When it fires it performs a proper ARM IRQ entry (SPSR_irq ← CPSR,
LR_irq ← addr+4, mode → IRQ with I set and T cleared, PC → `0x20cb0cd0`);
`BootReport.irqs_taken` counts deliveries.

The enable gate (`machine.rs`, in the code hook) is: deliver only when the
line is unmasked in the controller's enable-mask register `0x50302010`
(firmware clears bit 29 during the splash — verified), the core is not
already in IRQ mode, no tick is in service, **and** the core either has the
CPSR I-bit clear (a true hardware edge) or — in the pre-NU_Start splash
phase where the I-bit never clears — the CPU is parked in a recognized
busy-wait (`is_delay_loop || is_nested_delay_loop`, with
`E830_TIMER_FORCE`). A busy-wait holds no half-updated state and
the vector handler saves/restores SPSR, so interrupting it and returning is
transparent; forcing at an arbitrary PC instead corrupts the run, which is
why `force` is scoped to that safe point. The tick cadence is driven off
*total* instructions (real + fast-forwarded) so the clock advances through
the long fast-forwarded delays rather than stalling on them.

**Verified this session:** with `E830_TIMER_PERIOD=50000`,
`E830_TIMER_FORCE=1`, `E830_SEED_NUCLEUS=1` the full
vector → line-number read (0x74 = line 29×4) → mask read (bit 29) →
Nucleus clock ISR → EOI (`0x80` → `0x50302008`) path runs end to end, and
the splash *animates* (its sparkle background advances one frame per tick,
35 frames over a 2B-instruction run). Delivery saturates at 256 ticks and
the firmware never swaps to the menu: the `SEED_NUCLEUS` bridge (priming the
system stack at `[0x309011f8]` and the ready magic `[0x300295d8]=0x55`) lets
the ISR run but does **not** drive the real Nucleus scheduler, so the
splash-to-menu task transition does not occur. Reaching the menu needs the
real `NU_Start` / scheduler handoff modeled, not a deeper poke at the raw
timer IRQ.

### Clock-acceleration bridge and the splash fade loop

The tick-per-delay-loop-entry rate above is far too slow: the firmware's
calibrated millisecond delay (`0x20cd7a7e`, the recognized counting loop)
burns ~24M fast-forwarded instructions per call but, with one IRQ per entry,
advances the Nucleus clock by a single tick, so a wait the firmware thinks is
tens of ms needs billions of host instructions. `E830_CLOCK_ACCEL=1`
bridges this: whenever a delay loop is fast-forwarded, the Nucleus system-clock
counter in memory (`clock_counter_addr`, default `0x30900168`, observed to step
by `0x100` per real tick) is advanced by `skipped_instructions / period` ticks
worth directly (`advance_system_clock`). `E830_CLOCK_ADDR` /
`_INCREMENT` override the address and step for sweeping.

With accel on, the firmware leaves the first Nucleus delay and runs the splash
LCD-init table interpreter around `0x20155490`. That interpreter walks a
command/data table (writing register index/data to the LCD bus at
`0x44000000`/`0x44000002`) and, between entries, runs a *different* software
delay — `mov r8,r8; subs r0,#1; bne .-2` with the inner count reloaded from r2
each outer pass (bytes `c0 46 01 38 fc d1`, `FADE_DELAY_LOOP`). It is not one of
the earlier delay shapes, so it executed in real time and spent billions of
instructions holding the logo. `find_fade_delay_loop_pcs` + the
`is_fade_delay_loop` branch now fast-forward its inner countdown to zero and run
clock-accel across the skipped time, exactly like the other loops.

**Verified (this session):** with `E830_TIMER_FORCE=1`,
`E830_SEED_NUCLEUS=1`, `E830_CLOCK_ACCEL=1`,
`E830_TIMER_PERIOD=300000`, `E830_TIMER_IRQ=29`, the
firmware boots, brings up the LCD, and **renders the real SGH-E830 boot logo**
(“SGH-E830 / WWW.SAMSUNGMOBILE.COM” over the sparkle background with the
“SAMSUNG” wordmark) within ~800M instructions, then animates it (47–53 complete
frames). Proof image: `proof/e830-boot-logo.png`. The splash is an *infinite*
animation loop whose fade table repeats (reg `0x11` ramps `0x0712`/`0x0f12`/
`0x0f3f`); the transition out is driven externally, not by the table ending.
Even with the clock advanced aggressively (256 ticks, counter `0x38400`) the
firmware does not swap to the menu, which supports the conclusion below: the
splash-to-menu handoff is gated on a boot-completion event (scheduler task /
modem / SIM / filesystem), not purely on the system clock. The clock bridge is
a debugging crutch (opt-in, off by default) and does not model the real timer.

### Debugging the gate

`E830_PC_LOG=<n>` logs the next `n` executed guest PCs to stderr
as `[e830-pc] 0x…`, optionally starting only after
`E830_PC_LOG_AFTER=<instrs>` — use it to find the outer loop and
the branch that decides to leave the splash. Reaching the main menu almost
certainly requires identifying that boot-completion condition (modem / SIM /
filesystem handshake or a boot timer) and satisfying it, or finding where
the firmware finally unmasks IRQs and driving the Nucleus tick from there.

### Nucleus PLUS semantics (clean-room reference for the menu gate)

The E830 firmware's RTOS is **Nucleus PLUS 1.14** (Mentor Graphics). The
proprietary source is not in this repo, but full copies are public and were
used as a behavioural reference only (MediaTek/Siemens baseband trees:
`github.com/MobiFun/citrine` → `nucleus/`, `hyperion70/HSPA_MOLY…`,
`siemens-mobile-hacks/sdk`). No public emulator exists for the SGH-E830 /
Sysol chipset — a `gh search code` sweep for `SGH-E830`, `Sysol`, `mf04`,
`0x50302000`, and `Samsung SWIFT baseband` returns only user-agent/EXIF
datasets, so the Nucleus source is the only transferable prior art.

What that source establishes about the splash→menu gate:

* **The visible clock counter is the wrong lever.** Nucleus keeps two
  separate quantities: `TMD_System_Clock`, a free-running counter the HISR
  bumps every tick (this is what the model's `CLOCK_ACCEL` advances at
  `0x30900168`), and `TMD_Timer` / `TMD_Active_Timers_List`, the count-down
  that actually expires sleeps and application timers. Advancing the former
  does **not** wake a sleeping task — nothing reads it to decide a timeout.
* **What wakes a sleeping boot task** is: hardware timer IRQ → LISR activates
  the `"SYSTEM H"` HISR (`TMD_HISR`) → `TMC_Timer_HISR` sees
  `TMD_Timer_State == TM_EXPIRED` → `TMC_Timer_Expiration` decrements the
  head of `TMD_Active_Timers_List` by `TMD_Timer_Start` and, at zero, runs
  the app-timer expiration routine or `TCC_Task_Timeout` (the `NU_Sleep`
  path). The model already vectors to the real LISR, which is correct — but
  the `SEED_NUCLEUS` crutch fakes the system stack / ready magic without a
  running scheduler, so `TMD_Active_Timers_List` is empty and no timeout
  ever fires. That is exactly why 256 forced ticks never reach the menu.
* **If the splash instead waits on an event**, the control block is `EV_GCB`
  (`ev_current_events` = flags, `ev_suspension_list` = waiters). Only a real
  producer task (`NU_Set_Events` from the modem/SIM/filesystem handshake)
  clears the wait — same conclusion as the timer case: it needs a running
  scheduler, not a poke at a counter.

**Concrete next step this implies:** stop forcing/seeding; let the image run
`NU_Start` → `INC_Initialize` → `Application_Initialize` → `TCT_Schedule`
naturally, find the PC where it unmasks IRQs (CPSR I-bit clears) and installs
the periodic timer, and deliver genuine periodic ticks from there. Once the
boot task's `NU_Sleep`/timer is actually on `TMD_Active_Timers_List`, the
already-correct LISR→HISR→`TMC_Timer_Expiration` chain will expire it and the
menu handoff follows. `E830_PC_LOG` is the tool to locate that
I-bit-clear / timer-install site.

### Boot progresses into Nucleus OS init, then formats blank flash (clock-accel)

With `E830_CLOCK_ACCEL=1` and a large budget (300M+), the image
runs well past the splash into real Nucleus OS initialization. Three behaviours
are observable in the MMIO trace and confirm where boot actually stalls:

* **Interrupt-controller wiring (INC_Initialize).** The firmware installs IRQ
  handler vectors into the INTC priority registers (`0x503020c4`/`c8`/`f4`
  hold vector numbers `0x46`/`0x45`/`0x42`) and progressively unmasks lines in
  the enable-mask `0x50302010`: `0xdfffffff` → `0xdffdffff` → `0xdff9ffff`
  (clearing bits 29, then 17, then 18). The executing PCs are high in the
  image (`0x210f8f1a`, `0x215162f6`, `0x21509410`, `0x200b55xx`) — OS/app
  code, not the bootloader. **But the CPSR I-bit still never clears globally**
  (final `CPSR=0x200000ff`): the model is inside `INC_Initialize` wiring the
  INTC, not yet past `TCT_Schedule`’s first IRQ-enabled task switch.
* **Keypad matrix scan.** The routine at `0x208daa1c` reads the keypad row
  registers `SOC_KEYPAD_INPUT_REGISTERS` (`0x50504008`–`0x50504028`), decoding
  2 bits per key and comparing each field against a reference at `[r5,#8]`.
  The model returns `u32::MAX` (all keys released); this is a polled boot-time
  key-combo check, not the menu gate.
* **Blank-flash format pass.** The firmware issues 25k+ AMD/CFI unlock writes
  to `0x21c00aaa` / `0x21c00554`, programs the flash backing at `0x21f4xxxx`,
  and reads flash status at `0x21f4fffe` (`0xffff`). Flash reads come back
  `0xffffffff` (erased), so the firmware **formats the flash because it finds
  it empty** instead of loading menu resources from it.

**Implication for the menu gate.** Two concrete blockers remain, and the
instruction-budget crutches cannot clear either: (a) the flash filesystem is
blank — without a populated TFS (`--tfs` / `--tfs-flash-image`) the firmware
formats flash rather than reading the main-menu UI resources (fonts, icons,
menu strings) that live there; (b) the Nucleus scheduler handoff that globally
enables IRQs has not completed. Supplying the phone’s TFS filesystem image is
the single most likely unblock and is a missing *input*, not a code defect —
the `.cla` + `.nsf` pair alone does not contain the menu resource filesystem.

### Real TFS filesystem wired in — firmware reads it, menu gate unchanged

The full retail firmware package (firmware.center `E830/`) ships five files:
`E830XEGG1.cla` (code), `E830 eeprom.nsf` (EEPROM), **`E830XEGG1.tfs`**
(26 268 942-byte filesystem), plus `E830XEGG1.cfg` / `.def` (the TFS build
manifest and process table — human-readable, useful as a map). The `.tfs` is
the menu-resource filesystem whose absence §15 flagged as the likely gate. Its
`.cfg` header identifies it as `Samsung TFS Builder 1.1`, `TFSVERSION
TFS4.0_CORONA_01`, `NUM_DIR 127`, `MAXFILES 2099`, with dirs `/font`,
`/im/images/maintitle`, `/images/idle/menu`, etc. The TFS4.0 directory/FAT
metadata sits **near the end** of the image (~offset `0x1853a08`); the start is
raw file data (the first entry is a JPEG). The CLA references `0x21f40000`
directly, confirming that is the partition base.

`--tfs-flash-image E830XEGG1.tfs` loads it into the flash model at
`0x21f40000` (byte 0 of the file → `0x21f40000`). **Verified this session:** the
firmware now *reads* the TFS region at `0x21f40000` during boot (36+ reads in a
natural run) instead of finding erased `0xff`, so the blank-flash format pass is
addressed and the filesystem mount is attempted. **But the splash→menu gate is
unchanged** — with the real TFS present, both the forced-timer+seed recipe and a
natural boot still converge on the same steady state: the boot logo on the LCD,
the pre-scheduler polling superloop (keypad scan `0x208daa1c`, `SOC_TIMER`
`0x50501000`, INTC wiring), and **CPSR I-bit never clearing** (`0x…000000ff`).
This confirms the §15 conclusion empirically: the filesystem was a missing
*input* worth supplying, but it is **not** the menu gate. The gate is the
Nucleus scheduler bring-up.

### Decisive evidence: forcing IRQs before the scheduler is the wrong lever

Running the forced timer **without** `SEED_NUCLEUS` (i.e. injecting a real
periodic IRQ into the un-seeded image) faults deterministically:
`WRITE_UNMAPPED at 0xfffffff8, PC=0x208a1028`, immediately after the IRQ vector
(`0x20cb0cd0`) reads INTC pending (`0x50302000` → `0x74`) and the HISR tries to
save context to the current task's stack. `0xfffffff8` is an uninitialised
Nucleus TCB / system-stack pointer. `SEED_NUCLEUS` exists precisely to paper
over this fault by faking the stack/ready magic — but then the scheduler state
is synthetic, `TMD_Active_Timers_List` is empty, and no task timeout ever fires
(why 256 forced ticks never reach the menu). **The two crutches are mutually
exclusive dead-ends for principled reasons:** no-seed → fault on uninitialised
TCB; seed → fake scheduler with no real timer expiry. Neither can produce the
menu, because both bypass the one thing that must happen: the firmware's own
`NU_Start → INC_Initialize → Application_Initialize → TCT_Schedule` must run to
completion, initialise the real TCBs/system stack, clear the CPSR I-bit itself,
and install the periodic timer — after which genuine ticks delivered through the
already-correct LISR→HISR→`TMC_Timer_Expiration` chain expire the boot task's
`NU_Sleep`/app-timer and the menu handoff follows. The open problem is why the
natural boot never leaves the pre-scheduler polling superloop to reach that
handoff; that polling loop (not the filesystem, not the clock counter) is the
next thing to crack. `E830_PC_LOG`/`_AFTER` + `TRACE_IRQEN` remain the
tools. A global `gh search code` sweep (`SGH-E830`, `Sysol`, `TFS4.0_CORONA`,
`0x50302000`, `Samsung SWIFT baseband`) still returns no transferable prior
art — the Nucleus PLUS source remains the only reference.

### The pre-scheduler polling superloop, disassembled (PC trace)

`E830_PC_LOG` on a natural boot (no force/seed, TFS flash image
present) shows the steady state is a three-part superloop, all Thumb, all in
the pre-scheduler phase with the CPSR I-bit set:

* **Calibrated delay** at `0x20cd7a82` (the recognized counting loop) — the
  fast-forwarder skips its body.
* **Keypad matrix scan** at `0x208daa1c`. It calls an ARM long-branch veneer
  at `0x208a6d84` (`ldr ip,[pc]; bx ip`, far target in the literal at
  `0x208a6d8c`) to fetch a per-column input pointer, then runs an inner
  column-decode loop `0x208daa38`–`0x208daa4e`: `ldrb r7,[r5,#8]` (expected
  key code), `lsls/lsrs r2,r1,#0x1e` (extract the low 2 bits of the column
  word), `cmp r2,r7`; on match it sets a bit in `r4`, shifts the column word
  right by 2 (`lsrs r1,r1,#2`), and repeats for `[r5,#9]` columns. With the
  model returning all-released the scan always yields "no key", which is the
  correct steady state — this loop is a symptom of the superloop, not its gate.
* **Poll helper** at `0x200e1cb0` — a tiny table lookup (`[r3,r0]` vs
  `[r3+r0,#4]` selected by `r1`) returning a status word; its result feeds the
  branch that keeps the loop spinning.

The loop never raises the CPSR I-bit, so `NU_Start`/`TCT_Schedule` has not run.
The decode shapes are recorded here so a future session can re-locate them
without re-deriving the trace; the open question is still *what upstream
condition gates leaving this loop for the scheduler bring-up*, not any single
MMIO value inside it.

### New prior art: `bhmsgame06/sflasher` (Samsung Swift / PNX49xx)

A global `gh search code` sweep for `TFS4.0_CORONA` finally returned one real
hit: `github.com/bhmsgame06/sflasher`, a "Samsung Swift (PNX49xx)
Flasher/Dumper". It is the first public bare-metal code for the Samsung Swift
platform family found so far and is worth reading, but it is a **later chip**
than the E830 and its register map does **not** transfer: flash base
`0x90000000` (vs the E830's `0x21f40000`), UARTs at `0x70007000`/`0x70006000`,
power-lock `0x7001b030`, RAM `0xa0000000`, ITCM `0x1f400000`. What *is*
transferable: (1) confirmation the platform is Samsung Swift with a NXP/Philips
PNX49xx-class SoC; (2) the TFS4 on-flash layout — 512-byte pages carrying 511
data bytes + a 1-byte sector mark at offset `TFS_SECT_SIZE`=511, sectors based
at `(sect+0x8000)*512` from the flash base, marks `0xff`/`0xf0`/`0x00`; (3) the
TFS4 control-entry structs (`tfs4_ctrl_ent_dir/comm/file`) in `src/tfs.c`,
useful if we ever parse the mounted TFS directly. The E830's own
`0x50302000` INTC, `0x44000000` LCD, `0x50504008` keypad, and `0x30900168`
Nucleus clock are from an earlier Swift variant and remain undocumented in any
public source. Cloned to the conversation workspace for reference; not added to
the repo.

### QEMU backend for the E830 target (new this session)

The experimental E830 model now has a **second CPU/system backend built on
upstream QEMU** (`arm-softmmu`, v8.2.0), alongside the Unicorn path. It lives in
`research/qemu/`:

* `hw/e830.c` — a custom QEMU machine (`-M e830`, default CPU `arm946`) that maps
  the same address space the Rust model uses: flash/`.cla` at `0x20000000` (plus
  the low-vector, boot-ROM-mirror and `0x00010000` alias loads), EEPROM `.nsf` at
  `0x60100000`, optional TFS at `0x1c000000`, the SOC register banks
  (`0x50500000` + boot/config/power/memctl banks), and the 176×220 LCD behind the
  `0x44…`/`EXT_CTRL` GRAM-cursor protocol. Config is entirely via env vars
  (`E830_CLA`, `E830_EEPROM`, `E830_TFS`, `E830_LCD_OUT`, `E830_RUN_NS`,
  `E830_TIMER_PERIOD_NS`, `E830_TIMER_IRQ`) so no QEMU CLI plumbing is needed.
* `build.sh` — copies `e830.c` into a QEMU v8.2.0 checkout, patches
  `hw/arm/meson.build` + `Kconfig` (`CONFIG_E830`), configures `arm-softmmu` and
  builds `qemu-system-arm`. **QEMU is intentionally NOT vendored** — point
  `QEMU_SRC` at a `git clone --depth 1 --branch v8.2.0` checkout.
* `run.sh` — headless boot writing the latest LCD frame to a PPM.

**Crash fixed to get it booting:** `add_ram`/`add_io` passed `OBJECT(machine)` as
the MemoryRegion owner; `MachineState` is not a `TYPE_DEVICE`, so
`memory_region_init_ram`'s internal `DEVICE(owner)` aborted
(`qdev-core.h:77 … is not an instance of type device`). The owner must be a
`DeviceState*` or `NULL` — both calls now pass `NULL`.

**Verified this session:** QEMU boots the real `E830XEGG1.cla` and **renders the
genuine SGH-E830 boot logo** ("SGH-E830 / WWW.SAMSUNGMOBILE.COM / SAMSUNG" over
the bokeh background) to the 176×220 LCD — `proof/e830-qemu-boot-logo.png`.

**The splash→menu gate is unchanged by the backend switch.** A QEMU PC sample in
the steady state lands ~50% of the time in a `SOC_TIMER` (`0x50501000`)
calibrated-delay loop at `0x20010108`–`0x20010148` (read timer → wait for the
counter to pass `now+0xc8`), with the rest spread across high OS regions
(`0x208e…`, `0x214f…`, `0x2151…`). The LCD never advances past the logo across
15 s/45 s/90 s virtual-time runs. This empirically confirms §15's conclusion
from the opposite direction: a fully upstream, high-fidelity ARM core + MMU +
exception model reaches **exactly the same pre-scheduler steady state** as
Unicorn, so the gate is **not** CPU-backend fidelity — it is still the Nucleus
scheduler bring-up (`NU_Start → … → TCT_Schedule` clearing the CPSR I-bit and
installing the periodic timer). QEMU is nonetheless the better platform to crack
it on: real banked SP/exception handling means forcing a timer IRQ no longer
instantly faults on an uninitialised handler stack the way the Unicorn
`SEED_NUCLEUS`/no-seed dead-ends did.

**Update — real IRQ delivery sharpens the gate (QEMU, this session).** With the
interrupt-controller tick left enabled (`E830_TIMER_PERIOD_NS=1000000`, line 29)
and `E830_DEBUG=1`, the QEMU run shows the firmware does **not** stay I-masked:
after the splash it runs with **CPSR.I clear (I=0)**, unmasks INTC line 29 in the
enable-mask (`0x50302010 <= 0xdfffffff`), and **takes a genuine ARM timer IRQ**
(the core vectors through `0x18` on its own — "tick #13 RAISING IRQ line 29"),
runs the ISR, and acknowledges with the real EOI (`0x80 → 0x50302008`). So the
prior note's framing — that the gate is "clearing the CPSR I-bit and installing
the periodic timer" — is now disproven on the QEMU backend: **both already
happen.** Immediately after that first serviced IRQ the firmware re-masks every
INTC line (`0x50302010 <= 0xffffffff`, from the critical-section helpers at
`0x200b55e2`/`0x200b5606`) and settles into its splash superloop. A statistical
PC profiler (opt-in `E830_PROFILE=1`: a 5 µs virtual-time sampler that dumps a
top-PC histogram at shutdown) puts ~51 % of steady-state samples in the Thumb
dispatch/animation loop at `0x2001012c`/`0x2001013c` and the rest in an ARM
32-byte-copy loop around `0x204af704` (LCD-refresh / splash blit) — i.e. the hot
loop is the splash animation, not a stuck init. Conclusion, now from a core that
demonstrably delivers real periodic ticks: the splash→menu handoff is gated on a
downstream **boot-completion event** (modem / SIM / filesystem / a producer task
calling `NU_Set_Events`), consumed by a task the model has no producer for — not
on the I-bit, not on the timer install, and not on CPU-backend fidelity. The
next lever is identifying that producer/event, not any further poke at the raw
timer path. `E830_PROFILE=1` and the `E830_DEBUG=1` enable-mask/EOI trace in
`hw/e830.c` are the tools for that.
