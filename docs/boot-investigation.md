# E830: current native Windows boot investigation

Updated 2026-11-04. This file describes the standalone Rust/Unicorn model.
The old QEMU, forced-IRQ and clock-acceleration notes are archived in
[boot-investigation-legacy.md](boot-investigation-legacy.md); they contain
superseded conclusions and must not guide the current board implementation.
The existing desktop layout, LCD scaling and firmware picker are preserved;
the Library now also offers an explicit physical NAND image selector.

## Current outcome

The original E830XEGG1 firmware draws its SGH-E830/Samsung splash, creates the
RTOS system stack and unmasks IRQs itself. Application-menu navigation is not
verified unless noted; with the menu-key HLE fix below, SoftLeft opens the
application list and Up reaches UI dispatch (`0x68`). A complete
TFS image is now installed and verified through a fresh original loader process.
Retail E830 BML initialization and FAT mounting also succeed on that image.
The default DSP boot interface reaches the DSP-start handshake described below;
RD16024 execution is still absent. With the opt-in, image-specific
`E830_DSP_BOOT_HLE=1`, genuine HKBD initialization and standby callbacks now
advance the LCD to the original Samsung Fun Club startup animation (21 frames).
Application-menu navigation is still not verified. The timer's fractional counter units,
packed one-word serial IRQ9 mode and observed UART0 TX DMA are corrected from
the original drivers. DMA completion now releases the SL3 transmit queue through
the genuine IRQ29 callback. Original LCD window/cursor addressing now preserves
partial updates. The 400M run retains the complete Fun Club image; the 600M run
displays the original readable **Insert SIM / SOS** screen (26 full-screen-
equivalent writes), retained through 900M. The SIM illustration still has
visibly unusual colours, already present in the guest's graphics buffer rather
than introduced by LCD addressing. The user identifies this genuine standby
screen as the reached "menu"; opening the application list is not runtime
verified. Phone-controller input now reaches the original HKBD path and a
digit opens the genuine dialer. Neither mounting nor animation frames alone prove boot.

The additional opt-in `E830_SIM_PRESENT=1` development path now answers the
observed SELECT, GET RESPONSE, STATUS and READ BINARY commands and transfers
their data through the original SIM RX DMA driver. The firmware reads several
files of a fictional test card, but this candidate subsequently enters its
fatal reporter with `EA020101`; it is not a verified SIM/menu fix and is not
deployed as a SIM fix. The updated `dist/e830-emulator.exe` contains current
SIM/RF/analog HLE and keypad source. The old separate keypad binary is retained,
but its launcher now targets the updated main binary in explicit no-SIM mode.
The separate experimental `E830_DSP_RF_HLE=empty` model now handles the
observed RF requests and advances beyond `EA020101`, but reaches a different
original fatal `70012904` before a SIM-present menu. This is a synthetic
empty-air HLE, not recovered RD16024 execution or a SIM fix.
The newer opt-in `E830_ANALOG_HLE=battery` supplies only the observed serial
address3b measurement and passes that power-range check through the original
ARM conversion. RF timing now accounts for the guest timer-frame phase and
the next-frame descriptor pipeline under an explicit HLE assumption. Clearing
the producer mask releases that generation but does not cancel accepted work;
an idle zero-mask control does not replay stale commands. Calibration0333 is
supported, and the original ARM packer's 8-byte command / 40-byte scratch
copy contract is verified. Strict accepted-command history now recognizes
the observed scratch residue at scan restarts. The previous277.9M,
280.3M,288.4M,302.0M,304.8M and321.2M barriers are passed. The verified
033d/ff61 (-159) profile, same033d restart and captured885/033d to0..4/033f
transition now follow the original ARM producer and accepted-command history.
Same033f scan restarts, ff5d (-163) ARM arithmetic and the accepted885/033f
to0..4/033f scan-range transition are now verified. They pass the329.4M
and345.9M boundaries. A further optimization bug at356,038,371 collapsed
a large zero-fill across RF deadlines and timer matches with IRQs enabled.
Loop collapsing is now disabled while opt-in RF work is pending; original
instructions and normal IRQ delivery remain responsible for event ordering.
The previous356.4M and394.5M fatals are passed in the new tagged0333
candidate. Original ARM gain0333, correctionff75 (-139), band8 packer
inputs and the exact clear885/033f-to-tagged512..516/0333 scratch transition
are verified. Accepted tagged commands keep ten-bit channel decoding,
same-tag restart history, individual deadlines and synthetic raw -400.
The tagged033f-wrap candidate now passes the410,844,338,427,324,372 and
443,804,337 boundaries and completes900M without a memory fault.
It recognizes only the captured tagged807..810/0333-or-033d-or-033f four-request
tails for history updates, each requiring the exact accepted806 of its
own profile, records, extensions and timing. Exact accepted810/0333 residue
qualifies the transition to tagged512..516/033d; accepted810/033d qualifies
tagged512..516/033f. Tagged033f packing/correction and same-profile restarts
retain strict decoding. Exact accepted810/033f now also qualifies the
captured same-profile wrap to tagged512..516/033f; other same-profile wraps
remain unsupported. The previous missing-result check443,675,858,
EA020101443,804,337 and READ_UNMAPPED443,812,212 are passed without suppressing
errors or mapping the bad pointer. See the new checkpoint for reproduction.
The first393.915M
producer snapshot has mask0 and cannot establish an unsupported active
request from retained command bytes. See [memory debugging](rf-memory-debugging.md).
The previous SIM-present900M LCD had32 full-screen-equivalent writes and
showed a startup-animation fragment with lower-screen artifacts. The new
900M candidate completes with60 equivalent writes and a changed full LCD.
Local OCR reads Menu/Contacts, suggesting SIM-present standby; direct visual
inspection and application-list navigation are not yet verified. Its LCD
is saved, but image inspection is unavailable in this agent session.
Default no-card behavior is preserved; see the new checkpoint
below. Older checkpoints describe the implementation at their recorded stage.

### Checkpoint — 2026-11-01: host-side boot-loop optimization

The Unicorn engine already translates guest ARM code, but the emulator's Rust
code hook still runs once per guest instruction. The hot path now keeps the
recognized delay-loop PCs in one sorted compact table, skips an empty PC-watch
lookup, advances modeled devices only at their earliest exact deadline, and
caches the enabled pending-IRQ projection until MMIO, keypad, timer or device
state invalidates it. Firmware, NAND, RTOS state, guest timing and RF result
deadlines are unchanged.

A fresh 100,000,000-instruction SIM/RF/analog-HLE probe takes 4.710s on this
host, compared with about 8.02s before this optimization (host-load-sensitive;
roughly 41% faster). Two repeated 100M reports are byte-identical. The complete
900M regression takes 77.559s and exactly matches the previous tagged033f-wrap
candidate's PC `0x2107f49e`, 54,404,304 fast-forwarded instructions, 60 LCD
equivalents, 134 standby callbacks and 15,764 IRQs. It has no memory fault, and
the final PPM remains byte-identical with SHA-256
`5af4c4451d0ab59fc6938506ccda40a2c821c534a9297075afab00eb621fa490`.
`build-windows.ps1 -Test` passes all 176 Rust tests and Python unittest
discovery passes all 66 tests.

### Checkpoint — 2026-11-01: exact tagged033f tail and same-profile scan wrap

The user's screenshot reported READ_UNMAPPED at20847af4, matching the previous
443,812,212 stop. Continuation rechecks all264active bytes of the independently
captured443,366,255 group and implements the already captured438,569,060
mask00ff/slots7..4/tagged807..810/033f history update after exact806/033f.
The separate wrap rule requires tagged810/033f, tagged512..516/033f, exact
accepted eight-byte residue and24trailing zeros. Unknown profiles, arbitrary
wraps, partial/noncontiguous history and altered tail timing still reject.
Raw -400, next-frame deadlines, producer clear/idle, IRQ and ACK are unchanged;
firmware/NAND/RTOS are not patched or seeded.

build-windows.ps1 -Test passes176Rust tests (165library,1CLI,10GUI), including
four added tests for overlapping19deadlines, duplicate/idle/reset,15untrusted
tail mutations, all160extension-byte mutations, tag/profile/channel negatives
and rejection of same-profile wraps for other profiles.66Python tests pass.
Original ARM contracts pass41,135packer/458,797arithmetic/520collector cases.
The replay utility now has an explicit033f-wrap boundary; both old and new
replays accept5/4/5/5 with exact history and19deadlines for inactive fillersa5/5a:

```powershell
python -B tools/replay_tagged_rf_boundary.py `
  proof/native-windows/rf-tagged033f-next-tail-controls.mmio `
  proof/native-windows/rf-tagged033f-next-controls.mmio `
  proof/native-windows/rf-tagged033f-next-active-peeks.log `
  --boundary 033f-wrap `
  --rustc .tools/rustup/toolchains/stable-x86_64-pc-windows-gnu/bin/rustc.exe
```

rf-tagged033f-wrap-candidate-900m completes900,000,000 instructions,
InstructionBudget, PC2107f49e, memory_faultNone. The live accepted controls
443,366,255/443,526,019 confirm phases12399/12163; first-group raw -400
results publish443,531,008..443,647,488 before the old missing-result fatal.
The last recorded accepted scan tail is471,529,160; its results finish
471,778,368. No missing RF request is established from subsequent silence.
60LCD equivalents/134standby callbacks/15764IRQs/54,404,304fast-forwarded;
elapsed134.188s includes concurrent no-SIM work and is NOT a speed comparison.
Full PPM SHA256:5af4c4451d0ab59fc6938506ccda40a2c821c534a9297075afab00eb621fa490.
tools/ocr-lcd-frame.ps1 validates the176x220 P6 frame, creates a new4xPNG and
uses local Windows OCR. English reads `Menu IN01Sgryisæ Contacts`; Russian
reads `мепи lN03qgig Contacts`. Only Menu/Contacts are reliable text evidence;
the middle text is NOT established as No service or another status. No direct
visual inspection is available, so this suggests standby rather than proving
application-menu navigation. Saved image:rf-tagged033f-wrap-candidate-900m-4x.png.

rf-tagged033f-wrap-postscan-500m reaches500M/InstructionBudget/memory_faultNone.
Reporter watch470M..500M records190entries of300available, no severity1;
mostly severity2/70012c01 plus6680d673 at471,963,212 and70020f00 at472,797,044.
These are unsuppressed diagnostics, not proof of a halt. The watch does NOT
cover500M..900M. Its LCD still has43equivalent writes at500M; later graphics
progress must not be inferred from this early frame.

rf-tagged033f-wrap-candidate-nosim-600m exactly matches the previous BootReport
(exclude only peek/output-path trailers) and complete LCD SHA256
a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07.
26LCD equivalents/48standby callbacks/11792IRQs; elapsed68.281s.

New separate GUI:dist/e830-emulator-rf-tagged033f-wrap.exe,
dist/Run-E830-RF-Tagged033f-Wrap.cmd, SHA256
14012ccfebdfe0d33a054c1b45f89e11a63b7c0d7bce68f5e7bccf29204a8ef0.
Own process480 responds and closes normally in a brief no-run smoke;
config SHA256 remainsdb6a068fd9102f85d7f9442afe16f50f1d88dc9128aadf8f196bc8f2f45ee0b7.
The separate own-process3776 startup/capture/close smoke also succeeds;
gui-tagged033f-wrap-startup-smoke.png is a startup capture, not a boot/menu test.
User's tagged033f process14708 is preserved; mainGUI/distCLI/old033f hashes
are unchanged. No firmware/NAND/full-RAM duplicate or public upload.

Input regression probe rf-tagged033f-wrap-softleft-650m applies genuine
SoftLeft down600M/up601M, then stops606,692,352 with READ_UNMAPPED,
PC20847af4/address e24ee600,size1.44LCD equivalents/114standby callbacks;
this is the SAME handler/address as the old fatal, not a new address.
The ordinary no-input900M run does not reproduce it. Early input cannot be
claimed to open the menu safely; do not map this pointer or suppress errors.
Late input rf-tagged033f-wrap-softleft-after900m applies SoftLeft down900M/
up901M and stops908,612,174 with the same READ_UNMAPPED. Before it, the
reporter908,604,350 has severity1/EA020101,R7=1f,LR2098ddf3: missing RF work,
not a proven keypad failure. Only the later zero-extension group is accepted
at908,326,099,phase12243.62LCD equivalents/136standby callbacks/15907IRQs.
Local English OCR of the resulting frame reads `Messages Select 10:16 Back`.
This suggests the original application-menu screen was drawn after SoftLeft,
but there is no direct visual verification or stable navigation; the CPU
subsequently stops. Saved4x image:rf-tagged033f-wrap-softleft-after900m-4x.png.
Next task is the preceding unsupported RF generation near908.16M after input,
requiring bounded producer capture and independent active-byte peeks. The
early and late key runs differ in RF history; do not assume arbitrary input
timing or all keypad semantics are correct. No fault/result suppression.

The bounded follow-up rf-tagged033f-wrap-menu-next-controls starts908,145,000,
RAM limit1800/DSP limit1800/watch limit100 and stops before reporter908,604,349,
memory_faultNone. Its451,806byte trace records unsupported control908,166,322:
mask03ff,slots7..3,tag-clear channels0..4/profile0335, standard37/536+i910
records. Each40byte command retains tagged810/033f0e550800/067f0000 at+8,
then24zeros. The original gain0335/correctionff74 (-140) are watched;
packer2098d854 at908,156,512 has R0scratch3002995c,R1request304a86d0,
R2gain0335,R3band0, emitting00010800/066b0000. Collector205769f8 reads zero
at908,304,797 and908,464,746; original missing check908,475,864 has R2=0/R7=1f.
The later908,326,099 group is tag-clear5..9/0335 with zero extensions,
accepted5,phase12243 and first result908,491,008. It cannot replace the earlier
missing generation before that deadline.

rf-tagged033f-wrap-menu-next-peeks independently stops post-control205747f4
at908,166,322 with105aligned words. All264active bytes match;156inactive
remain unknown. Reproduce:

```powershell
python -B tools/inspect_rf_memory_trace.py `
  proof/native-windows/rf-tagged033f-wrap-menu-next-controls.mmio `
  --after 908160000 --before 908170000 --active-only `
  --compare-peeks proof/native-windows/rf-tagged033f-wrap-menu-next-peeks.log `
  --compare-at 908166322
```

This identifies a new tagged810/033f-to-clear0..4/0335 transition, not a new
unknown gain profile. Before implementing, replay accepted prior tagged033f
tail/history through this first post-input group with all32extension bytes and
tag/range/timing negatives. No generic tag-clear or channel-wrap exception.
The bad-pointer function's original Thumb20847af4 instruction is ldrb[r0,#8]
in an error-table lookup; its occurrence after EA020101 is not established
as RF-memory corruption and no invalid pointer is mapped.

### Previous checkpoint — 2026-11-01: exact tagged033d tail and tagged033f fix

Continuation implements only the independently captured422,088,363
mask00ff/slots7..4/tagged807..810/033d tail after exact accepted806/033d.
The two observed tail profiles0333/033d share explicit record timing and
zero-extension checks; tagged033f four-request tails still cannot update
history. Tagged033f format is admitted after original band8/gain033f,
correctionff5d (-163), packed0c010800/067f0000 and active-byte verification.
The new cross-profile rule requires tagged810/033d, tagged512..516/033f,
exact eight-byte residue and24trailing zeros. No general profile transition,
guest memory patch, result-mask seeding or RF timing change is added.

build-windows.ps1 -Test passes172Rust tests (161library,1CLI,10GUI);66Python
tests pass. Four new Rust tests cover19independent deadlines, overlapping
generations, duplicate/idle/reset, thirteen untrusted tail mutations,
future033f-tail rejection, all160extension-byte mutations, profile/channel/
tag and reserved/alignment negatives. Four new Python tests verify that
offline replay never invents active bytes. Original ARM harnesses remain
41,135packer/458,797arithmetic/520collector cases passing.

tools/replay_tagged_rf_boundary.py compiles the actual src/dsp_rf.rs in a
temporary isolated harness and replays the two private traces. Independent
peeks match264active bytes at426,886,248;156inactive bytes stay unknown.
Both inactive-fill fixturesa5/5a accept5/4/5/5 and yield the exact accepted
history and19deadlines. Captured033d phases and the unchanged-origin033f
phases12392/12158 are used; the subsequent live run confirms both phases.
No live firmware/NAND/RAM is modified by this harness. Reproduce:

```powershell
python -B tools/replay_tagged_rf_boundary.py `
  proof/native-windows/rf-tagged033d-candidate-next-memory.mmio `
  proof/native-windows/rf-tagged033f-accepted-transition-controls.mmio `
  proof/native-windows/rf-tagged033f-accepted-transition-peeks.log `
  --rustc .tools/rustup/toolchains/stable-x86_64-pc-windows-gnu/bin/rustc.exe
```

rf-tagged033f-candidate-900m uses the existing installed NAND with explicit
SIM/boot/RF/battery options. Controls426,886,248 and427,046,014 both accept5;
individual raw -400 results publish at427,051,008..427,167,488 and
427,211,008..427,327,488. The old427.3M fatal is passed. Budget900M is NOT
reached: READ_UNMAPPED stops443,812,212 at20847af4/address e24ee600, size1.
It has43LCD equivalent writes,109standby callbacks,4337IRQs and53,986,296
fast-forwarded instructions. Elapsed61.623s is one loaded-host measurement,
not a speed guarantee. Full PPM SHA256 matches the previous033d candidate:
2aa46f28260edbe9a20c6a3d47933b8817e956d3c997f7e03b2e1cb0f82cfbb7.
Saved PNG is not visually inspected; no SIM/menu claim. The early reporter
watch exhausted100entries on severity2/70012c01; absence of EA020101 in that
watch is NOT evidence of no fatal. The focused trace below captures it.

rf-tagged033f-candidate-next-memory starts bounded RAM/DSP tracing443.5M.
At443,526,019 it captures complete tagged517..521/033f, mask03ff, all32
extension bytes zero, accepted5. Results publish443,691,008..443,807,488.
The preceding collector reads zero443,664,746; original2098dde6 at443,675,858
has R2=0/R7=1f, then reporter443,804,337 has severity1/EA020101, LR2098ddf3.
A later alternate collector read at204af70c/443,804,971 observesf0 while
slot3is still pending. These distinct reads must not be correlated as one
completed generation. No pointer mapping or fatal suppression is added.

Next boundary is independently captured: rf-tagged033f-next-tail-controls
records tagged802..806/033f mask03ff at438,410,791, accepted5, followed by
807..810/033f mask00ff at438,569,060, accepted4. Their extensions are zero;
collector flagsf8/f8/f0 at438,544,879/438,704,946/438,864,796. Current history
still ends806/033f because only0333/033d four-request histories are supported.
rf-tagged033f-next-controls captures tagged512..516/033f at443,366,255,
retaining exact810/033f pair0e550800/067f0000 plus24zeros. No submit is logged;
the unchanged decoder does not recognize this channel-wrap residue. All264
active bytes match rf-tagged033f-next-active-peeks at post-control205747f4,
before instruction443,366,255;156inactive bytes stay unknown. Original gain
033f/correctionff5d and band8/channel512 packer call443,356,466 are watched.
Reporter breakpoint443,804,336 has memory_faultNone. This supports investigating
the exact033f tail AND tagged810-to512..516 same-profile wrap next, not a
blanket tail exception. The tail trace's2500DSP-entry budget exhausts before
443.36M controls, so the separate443.35M capture is required.

Capture bookkeeping correction: the old private rf-tagged033f-next-controls
and rf-tagged033f-next-active-peeks output names were inadvertently reused
for this new boundary; the old two files were overwritten and no backup is
known. Historical observations above remain stage-specific, but those old
filenames now refer to443.36M, not426.88M. Replacement current accepted-stage
captures use rf-tagged033f-accepted-transition-controls/peeks and the replay
command above uses them. tools/run-probe.py now refuses any existing .log or
.mmio output name before launching the guest; fresh capture names are required.

rf-tagged033f-candidate-nosim-600m completes600M with an exact unchanged
BootReport (exclude only diagnostic peek/output-path trailers). Full LCD
SHA256 remainsa7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07;
26equivalent writes,48standby callbacks,11792IRQs. Elapsed67.583s is not a
performance comparison.

Separate GUI: dist/e830-emulator-rf-tagged033f.exe and
Run-E830-RF-Tagged033f.cmd. SHA256:
6c824125dce72bd0193d2214eb2bc50a87f9938fb1699054900354c282f06a60.
Brief GUI startup/close smoke succeeds for only its own process14692,
gui-tagged033f-candidate-smoke.png; config SHA256 remains
db6a068fd9102f85d7f9442afe16f50f1d88dc9128aadf8f196bc8f2f45ee0b7.
User's running tagged033d process4080 remains untouched. MainGUI, oldRFDebug,
tagged033d, distCLI and their hashes are unchanged. No firmware/full-memory/
NAND duplicate and no public upload.

### Checkpoint — 2026-11-01: exact tagged0333 tail and tagged033d fix

The user requests continuation of RF/SIM HLE implementation. Original
firmware, source NAND, RTOS state, result arithmetic and IRQ behavior are
unchanged. src/dsp_rf.rs now admits the original packed tagged033d profile,
including zero-extension and same-tag/same-profile restart commands. For the
captured transition it requires accepted tagged810/0333, channels512..516,
profile033d, exact eight-byte residue and24zero extension bytes. Unknown
tagged profiles0335/033f, reserved/alignment bits and unrelated transitions
still reject.

Only the independently captured mask00ff, slots7..4/channels807..810/0333
tail following accepted806/0333 updates four-request history. It requires
all eight active records, packed tag/profile, zero extension and captured
start/finish times37/536 plus910 per producer index. This is not a generic
count4 exception; partial/malformed/noncontiguous groups cannot change
history. Existing count3/5 history rules and per-slot deadlines are retained.
Synthetic raw -400, payload-before-flag, ACK and pending-work preservation
across producer clear remain unchanged.

build-windows.ps1 -Test passes168Rust tests (157library,1CLI,10GUI);
Python unittest discovery passes62. Four added Rust tests cover the captured
overlapping batch sequence, independent deadlines, duplicate/idle/reset,
12untrusted tail mutations, exact history/transition/extension negatives,
tagged033d ten-bit channels and reserved/alignment rejection. Original ARM
harnesses pass41,135packer,458,797correction/clamp/RXLEV and520collector cases.
These verify contracts, not a physical DSP or successful phone boot.

rf-tagged033d-candidate-900m uses the existing installed NAND and the four
explicit SIM/boot/RF/battery HLE options. Both formerly rejected controls
410,405,639 and410,565,420 accept5 and publish individual raw -400 results;
the old410,844,338 fatal is passed. The900M instruction budget is NOT reached:
execution stops at427,332,247 with READ_UNMAPPED, address e24ee600, size1,
PC20847af4. Before that, original reporter200141c0 at427,324,372 reports
severity1/EA020101, LR2098ddf3. There are43LCD equivalent writes,109standby
callbacks and3819IRQs; elapsed100.210s is a single host measurement, not a
speed guarantee. PPM SHA256:
2aa46f28260edbe9a20c6a3d47933b8817e956d3c997f7e03b2e1cb0f82cfbb7.
The saved PPM/PNG has not been visually inspected here; no menu claim.

rf-tagged033d-candidate-next-memory records the accepted802..806/033d group
at421,930,768 and807..810/033d mask00ff tail at422,088,363. Results are
published and original collector readsf0 at422,384,626, but history stays
at806/033d because only the verified0333 tail exception is implemented.
The original producer later retains810/033d in the first tagged033f group.
This is the next scoped investigation, not permission for blanket profiles.
rf-tagged033f-next-controls captures mask03ff/tagged512..516/033f at
426,886,248 with residue0e550800/067b0000 plus24zero bytes, and517..521/033f
at427,046,014 with32zero extension bytes. Neither is accepted by the current
tagged-profile guard. Original collector reads zero at427,024,746 and
427,184,776. All264captured active bytes in the first group match independent
rf-tagged033f-next-active-peeks at the post-control breakpoint205747f4;
156inactive bytes are unknown. Breakpoints report the boundary before its
instruction executes, while watch/control logs can count that instruction;
compare-at uses the actual control timestamp426,886,248, not minus one.
rf-tagged033f-next-first-input stops before packer2098d854 at426,876,458:
channel512, band8, gain033f, correctionff5d (-163) from the matching watch,
scratch0e550800/067b0000 at30029964/68. Request words02000000/0008ff24 and
300333fc low halfwordff29 (-215) are retained as independent inputs, not
inferred RF behavior. Original packed pair is0c010800/067f0000.
Bounded trace sizes are374,858bytes (controls) and386,815bytes (tail/fatal),
not full memory dumps. An earlier450-entry DSP I/O trace exhausted its
budget on polling before the active controls; it is not used to infer them.
The original missing-result check2098dde6 at427,195,893 has R2=0/R7=1f;
the reporter breakpoint stops at427,324,371 with memory_faultNone. The later
unmapped read occurs in the original20847ad8 routine after the fatal;
disassembly shows ldrb r1,[r0,#8] at20847af4. Root cause of that pointer is
not established by the bounded RF trace.

rf-tagged033d-candidate-nosim-600m passes600M with a BootReport exactly equal
to rf-bandtag0333-nosim-default-600m after removing only the output filename.
Full LCD bytes/hash match: a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07;
26equivalent writes,48standby callbacks,11792IRQs. Elapsed110.136s while
another probe was running; this is not a performance comparison.

Native release build succeeds. A separate GUI candidate is provided as
dist/e830-emulator-rf-tagged033d.exe with Run-E830-RF-Tagged033d.cmd; it is
experimental, not a verified SIM fix. SHA256:
092246922fa626a0efe0a58aa32df5f3ec006035fefb1730a82d7a217ee9f850.
The existing mainGUI, CLI and old RF Debug candidate are not overwritten;
old RF Debug SHA256 remains223aeec5b891ddd90c8f61e592a5d9aa0e6fa20a33ca75241a7172e5409cf9b5.
The brief capture-window smoke starts/closes only its own GUI process19736,
saves gui-tagged033d-candidate-smoke.png and leaves config SHA256 unchanged:
db6a068fd9102f85d7f9442afe16f50f1d88dc9128aadf8f196bc8f2f45ee0b7.
This verifies startup/window lifecycle, not long GUI boot or visual menu.
No full RAM/NAND dump or firmware duplicate is created. No public upload.

### Checkpoint — 2026-10-31: independent current-Rust replay confirms both gates

The user's follow-up requests verification, not deployment. The existing
264active-byte comparison at410,405,639 passes again. An isolated executable
compiles the unchanged src/dsp_rf.rs directly from stdin and replays the
captured405,610,184 five-request batch,405,768,363 four-request tail, and
both tagged033d generations. All active mask/record/40byte-command bytes are
required to be captured; inactive bytes use alternating synthetica5/5a fillers
in separate fixtures. Decoding matches for both fillers; unknown/inactive
guest memory is not inferred as zero. No live emulator memory is modified.

Assertions on actual RfHle state confirm submit5 caches806/0333; submit4
executes all four requests but leaves previous_command at806/0333. The
preceding five requests remain in flight across the mask clear and finish
405,771,008..405,887,488; tail results finish405,931,008..406,018,368, with
raw -400 and slot flag unionf0. The captured next residue is810/0333, not806.
Both the old806 and independently verified810 histories still reject the
first tagged033d generation. Its following zero-extension generation also
rejects. A clear-tag mutation of only that zero-extension fixture accepts5
in the isolated harness, confirming the tag/profile guard; this mutation is
not performed on firmware/NAND/live memory and does not authorize tagged033d
or establish physical RF behavior. A scoped format/transition AND history
fix is still needed, not just one of them.

Repeated original ARM harnesses pass:41,135packer cases preserve+8..+39,
458,797correction/clamp/RXLEV cases and520collector cases. Synthetic -400
withff61 still yields -259half-dB, clamp -220 and RXLEV0. This is consumer
contract verification, not successful phone boot. Diagnostics create only
small temporary standalone executables; no new deployed build or behavior
changes. User process32600, RF Debug executable and config remain unchanged.

### Checkpoint — 2026-10-31: user's candidate freeze; four-request tail leaves stale history

The user reports the RF Debug candidate freezing again. Only process32600,
dist/e830-emulator-rf-debug.exe, is open; its SHA256 remains
223aeec5b891ddd90c8f61e592a5d9aa0e6fa20a33ca75241a7172e5409cf9b5.
The passive gui-user-tagged033d-freeze.png shows the same pink animation
fragment/lower artifacts as the previous CLI LCD, not a menu. The GUI counter
shows48Frames, not directly equivalent to the CLI's37full-screen-equivalent
writes. No live guest-memory inspection/attachment is claimed: the following
probes reproduce using the matching build, four HLE options and selected NAND.
The user's GUI is neither stopped nor replaced.

rf-tagged033d-memory-tail-fatal expands bounded RF-window tracing to
405.58M, capturing636RAM writes,105control-window words and DSP I/O. At
405,610,184, mask03ff/tagged802..806/0333 is fully accepted5; slot3 pair is
0e4d0800/06670000. At405,768,363, mask00ff/tagged807..810/0333 is fully
accepted4, all32extension bytes zero. Final slot4 pair is
0e550800/06670000. Its four results publish individually at405,931,008,
405,960,128,405,989,248 and406,018,368. Original collector reads flagsf0
at406,064,626. This establishes accepted tagged810/0333 provenance; later
idle zero-mask controls do not replay it.

The unchanged RfHle::submit history rule accepts only fully recognized
three/five-request batches for caching (matches!(count, 3 | 5)). Thus the
four-request tail executes but does NOT update previous_command: it still
contains tagged806/0333. This is a separate history gap, not proof that
results were lost. Current tagged033d is rejected earlier by the profile/tag
guard; merely admitting that profile would still not authorize the actual
810/0333 residue. A future fix must constrain the observed four-request tail
and exact tagged810/0333-to-tagged512..516/033d transition separately.

The next controls are unchanged:410,405,639 tagged512..516/033d with the
810/0333 pair at+8 plus24zero bytes, then410,565,420 tagged517..521/033d
with32zero extension bytes. rf-tagged033d-active-submit-peeks breaks just
after the first control, PC205747f4/instructions410,405,639, and reads all
105aligned RF-window words. inspect_rf_memory_trace.py comparison confirms
all264captured active bytes exactly;156inactive bytes remain uninferred.
The trace is359,950bytes (about352KiB), not a full RAM dump.

rf-tagged033d-first-input breaks before original2098d854 at410,396,437:
R0=3002995c scratch,R1=304a86d0 request,R2=033d gain. Peeks:
304a86d0=02000000 (channel512),304a86d4=0008ff2d (band8, requested -211),
304a86d8=0,300333fc=ff29 (-215),30031d34=28,30033418=00020100
(signed byte at3003341a is+2). The original watch at205760de confirms
R0=30033412/R3=8 for that load. Original correction205760f6 returnsff61
(-159). Scratch+8 at30029964/68 is exactly0e550800/06670000 before packing;
copied active words are0c010800/067b0000, matching the original band8/gain033d
packer contract. Other scratch words before the packer are not commands.
Synthetic raw -400 would still give -259half-dB before the original clamp
to -220/RXLEV0; no result values are changed by this diagnostic.

The original collector reads zero flags at410,544,590 and410,704,590;
2098dde6 at410,715,852 has R2=0/R7=1f,EA020101. The reporter breakpoint
is200141c0 at410,844,337 (PC-watch convention410,844,338),LR2098ddf3.
BootReport: memory_fault None,37LCD writes,104standby callbacks,3,520IRQs,
53,983,388fast-forwarded instructions. There is no captured invalid memory
access or active-buffer mismatch; this does not prove every RAM byte or
graphics-buffer operation correct. This freeze is reproduced as a missing
RF result leading to original fatal sleep, not a demonstrated memory overrun.

62Python tests pass. No source/HLE behavior changes, new binaries, firmware
patches, IRQ changes, guest state seeding, NAND writes or full dumps. Existing
GUI/config hashes remain unchanged. The request was diagnostic only; implement
the scoped tail/tagged033d fix as the next explicitly requested change.

### Checkpoint — 2026-10-31: verified tagged0333 passes394.5M; tagged033d is next

rf-bandtag0333-gain watches the original20575fe8 producer,205760b8 gain,
205760f6 correction and2098d854 packer. Channels512..516 all select gain0333
and return correctionffffff75 (-139). The band8 table base is30032f68,
R5=ffffffd5 (-43), calibration pointer30029948, correction pointer30029a94.
rf-bandtag0333-input breaks before the first original packer: request at
304a86d0 is02000000 (channel512),304a86d4=0008ff40 (band8, requested -192),
304a86d8=0,300333fc=ff29 (-215),30031d34=28. The signed-byte correction
load is3003341a; its byte was not separately peeked. The peek at3001a5c4
is033f, not the selected0333 gain entry, and must not be cited as such.
The original correction output and packed tagged words are independently
verified. Synthetic raw -400 stays unchanged: correction -139 gives -239
half-dB before the original clamp to -220 / RXLEV0.

src/dsp_rf.rs now permits only packed bit16 plus the ten-bit channel for
tagged0333. Tagged0335/033d/033f and reserved bits remain rejected. Previous
channel extraction masks off the tag. Same-profile restarts require the same
tag and accepted-command history. A separate exact transition admits only
clear885/033f to tagged512..516/0333 with the previous eight-byte pair at+8
and24zero bytes; ordinary zero extensions retain their strict format checks.
Clear-tag range transitions remain clear-only. No firmware patches, seeded
guest state, passing flags, IRQ/timer changes or blanket tag acceptance.

rf-bandtag0333-final-900m accepts five slots at394,085,629 and394,245,410.
The first group completes individually at394,251,008..394,367,488;
the original collector readsf8 at394,384,590 and ACKsff07 at394,384,615.
Further tagged0333 measurements complete through406,018,368. The900M run
takes90.030s, with53,983,396 fast-forwarded instructions, PC20014b4c,
memory_fault None,37LCD writes,104standby callbacks,3,520IRQs, and mask
3049198c=07ff0000. PPM SHA256:
d81923898676bb00d96b8592ad54d872d883835aa09dcd72b690836a8bd3b851.
The rendered PNG still shows a startup-animation fragment and lower-row
artifacts, NOT a SIM-present menu. More frames are progress, not a menu proof.

rf-bandtag0333-next-memory captures the next active generation410,405,639:
mask03ff, tagged512..516/profile033d, ordered05/00 and02/0a records,
extension0e550800/06670000 (tagged810/0333) plus24zero bytes. At410,565,420,
tagged517..521/033d have32zero extension bytes. The original gain is033d
and correctionff61 (-159). Both remain unsupported in this candidate;
collector reads zero flags at410,544,590 and410,704,590. Original missing
result2098dde6 at410,715,852 has R2=0/R7=1f, then severity1 reporter at
410,844,338 has EA020101/LR2098ddf3. The bounded trace starts after the
preceding accepted tail: a matching residue alone does not establish its
accepted provenance. Check that history and exact inputs before extending.

164Rust tests pass (153library,1CLI,10GUI/runner), plus62Python tests.
Isolated unchanged original ARM packer41,135, correction/math458,797 and
collector520 checks pass. rf-bandtag0333-nosim-default-600m reproduces the
previous no-SIM BootReport exactly (excluding output filename/peek lines),
including26LCD writes,48standby callbacks,11,792IRQs and PPM SHA256
a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07.

Deployment is separate: dist/Run-E830-RF-Debug.cmd launches the new
dist/e830-emulator-rf-debug.exe with all four explicit HLE options, using
the existing firmware/NAND selection. Candidate SHA256:
223aeec5b891ddd90c8f61e592a5d9aa0e6fa20a33ca75241a7172e5409cf9b5.
The user's running process29644 and main dist GUI/CLI were not replaced.
tools/capture-window.ps1 now accepts an optional ExecutablePath; its smoke
test started and normally closed only candidate process30040. Screenshot:
gui-rf-bandtag0333-smoke.png (startup/UI smoke, not the final boot screen).
dist/e830-config.json SHA256 remains
db6a068fd9102f85d7f9442afe16f50f1d88dc9128aadf8f196bc8f2f45ee0b7.
No full RAM/NAND dumps or duplicated firmware were created.

### Checkpoint — 2026-10-31: memory tracing identifies the next unsupported band tag

The user's open GUI remains on the orange animation fragment with lower-screen
artifacts (gui-user-rf-memory-freeze.png). This is not a menu. Its process29644
and executable were preserved; the following probes use the existing CLI.

rf-memory-last-generation enables the existing bounded MEM_WRITE trace for
60000658..600007fb after391.8M, plus DSP I/O and original ARM PC watches.
tools/inspect_rf_memory_trace.py reconstructs byte writes offline and takes
isolated snapshots at actual50505000=40 controls. Missing bytes remain
unknown. It reports active records, packed channels/tag/profile, raw words,
extension coverage, last captured writers and chronological result-flag reads.
An absent HLE submit log is labelled as such, not alone proof of rejection.

The last accepted group is mask3f at392,007,511: channels883..885/033f,
tag clear. Its final slot5 cache is06eb0800/067f0000; completions at
392,171,008,392,200,128,392,229,248 remain individually timed. Subsequent
zero-mask controls do not replay retained commands.

At394,085,629 the original producer has mask03ff, five slots7..3,
channels512..516, tag SET, calibration0333 and the same ordered05/00,
02/0a records/times as earlier scans. Each40byte copied slot retains
06eb0800/067f0000 at+8 followed by24zero bytes. This matches the last
accepted tail's scratch pair, but is NOT an already supported transition.
The decoder rejects the tag even before inspecting that residue.

At394,245,410 another nonempty03ff generation has tagged channels517..521,
profile0333, all32extension bytes zero. Slot3 raw words are0c130800/06670000.
All active bytes are captured:4mask +60records +200commands =264bytes.
The other156window bytes are inactive and not inferred. The independent
rf-memory-active-submit probe breaks at205747f4 immediately after control
and peeks all105aligned window words. All264captured bytes match exactly.
Both independent probes have memory_fault: None. No RAM/NAND dumps were made.

The original collector reads5050501c=0 at394,224,609 and394,384,590,
with no HLE submit/completion for either tagged generation. The original
guest clears the final mask at394,394,124/126, then reaches2098dde6 at
394,395,852 with R0=1, R1=ea020101, R2=0, R7=1f and LR2098dddf.
The breakpoint report stops at reporter200141c0 at394,524,337 (the PC hook
in the previous run logs394,524,338). The existing900M probe then reaches
fatal sleep. This explains the reproduced freeze as an unsupported RF
request/result contract, not an observed invalid memory access.

Only offline tooling/tests/documentation change at this checkpoint:61Python
tests pass, including10new memory-trace regressions. HLE acceptance, result
values, timing, firmware, NAND, RTOS and GUI remain unchanged. Original
gain/correction, tagged packer inputs and exact transition checks are next;
do not blanket-accept tagged requests or suppress EA020101.

### Checkpoint — 2026-10-31: pending RF work must not be fast-forwarded

The two033f decoder changes below first advanced the final900M run to
EA020101 at356,444,374, LR2098ddf3. That intermediate run produced24LCD
writes,81standby callbacks,2,607IRQs and54,398,528 fast-forwarded instructions.
Its PPM SHA256312dc817810412e30b19644c99ca7238d4737c036048ca282b0be755d66a7dd7
differs from the old frozen fragment but still shows startup animation.

rf-profile033f-range-delay-watch proves the temporal error: original ARM
zero-fill204af5fc enters at356,038,371 with R0=30773614, R1=001887e0,
R2=R3=R12=LR=0 and CPSR2000001f (IRQs enabled). The optimizer clears
1,607,680bytes at once and advances200,960 loop instructions, reaching
356,239,331. Several accepted RF deadlines and timer matches are crossed;
seven RF publications occur at that one time and repeated slot flags coalesce.
Original collector failure2098dde6 at356,315,852 has R2=18/R7=1f,
before the severity1 reporter at356,444,374. Register equivalence alone
does not preserve asynchronous hardware ordering.

All four loop-collapse paths now return without accelerating while dsp_rf
has pending accepted work. Normal instruction execution, device payload
publication, W0C flags and IRQ21 collection are unchanged. Default RF-off
loop optimization remains available. No timer/deadline retuning, forced IRQ,
firmware/NAND/RTOS/keypad changes or passing ARM masks were introduced.
The new actual-Unicorn regression runs counting, nested, fade and ARM
zero-fill entries with pending RF work and verifies one original instruction,
zero skipped instructions, preserved CPSR, no premature flags and only the
first16bytes cleared by the actual ARM store. 160Rust tests pass
(149library,1CLI,10GUI/runner), and51Python tests pass.

rf-profile033f-events-final-900m completes in87.119s: instructions900M,
fast-forward53,979,000, PC20014b4c, no memory fault,32LCD writes,
99standby callbacks,3,211IRQs and ARM result mask07ff0000. The previous
356.4M boundary is passed and individual RF completion times are restored.
Its PPM SHA256c936261c5fffc7c09ba4d48780a06dbcf209333099099ab1c015465a6ef04925
visually shows a later orange animation fragment with colored artifacts in
the lower rows. It is NOT a SIM menu or a resolved freeze.

rf-profile033f-events-next-halt starts its reporter watch only after392M:
severity2 analog70012c01 diagnostics return normally. The new severity1
EA020101 is at394,524,338, LR2098ddf3; the original fatal sleep loop
20014b4c is reached at400,876,817. The earlier900M watch exhausted its
four records on severity2 diagnostics, so absence of EA020101 there was
NOT evidence that the fatal was gone. The next command producer must be
captured rather than suppressing this assertion.

rf-profile033f-events-absent-600m retains the readable no-card screen:
PPM SHA256a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07,
26LCD writes and48standby callbacks. Its sampled CPU/IRQ counters differ
from the preceding RF/battery-enabled baseline; only visual equivalence
is claimed for that comparison. The separate real no-card launcher mode,
boot HLE only, is rf-profile033f-events-nosim-default-600m (68.188s).
Its complete BootReport is identical to sim-files-absent-regression-600m
after excluding output filenames and extra diagnostic peeks:54,168,644
fast-forwarded, PC204b00d0,26LCD writes,48standby callbacks and11,792IRQs.
Its PPM also retains SHA256a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07.

rf-profile033f-events-next-boundary stops at original control393,915,273.
The active mask is0; retained command/scratch bytes must NOT be treated as
an unsupported submitted group. Scratch30029964/68 is06eb0800/067f0000,
input304a86d0/4 is03750000/0004ff24. Continue with the last NONEMPTY
control and original producer provenance before394,524,338.

Verified release GUI and CLI were copied to dist only after checking that
the user's GUI was no longer running. GUI SHA256
bcc1ef096665894e18035d3b43b08e4e27d7919e3c81c57f66c43d6a3e4d3e60;
CLI de4bd420fc5c8fafc936e6a296d76a994f752b63d87b85df0faa339c772f3bd4.
gui-profile033f-events-smoke.png confirms the real GUI starts with explicit
SIM/boot/RF/battery labels and preserved keypad/layout. This brief smoke
is not a long-run SIM-menu test. Only the smoke's own process30012 was
closed normally. Config SHA256 remained
db6a068fd9102f85d7f9442afe16f50f1d88dc9128aadf8f196bc8f2f45ee0b7;
selected NAND, firmware and launchers are unchanged. No GitHub push.

### Checkpoint — 2026-10-31: verified033f same-profile scan restart

The user again reports the same frozen screen. A passive capture of the user's
running GUI15156, gui-user-profile033d-freeze.png, confirms the startup-animation
fragment and explicit SIM/boot/RF/battery HLE labels. Responding=True confirms
only Windows responsiveness, not guest progress. The user's window was left
open; it runs the preceding dist build, not this new candidate.

rf-profile033f-restart-prior captures original control326,570,078,
mask03ff, five complete ordered operation pairs in slots7..3,
channels518..522/033f and zero extensions. All five are accepted. Original
gain/correction stores326,560,749..326,568,263 select033f/ff5d (-163)
with R5=-43 and table30032968; the packer uses scratch30029964.
The last accepted channel522/033f command is04150800/067f0000. The
preceding checkpoint's328,965,634 restart contains those exact eight
bytes at+8 in all five slots523..527, followed by24zeros.

rf-profile033f-restart-packer-input breaks before2098d854 at328,956,442.
R0=3002995c, R1=304a86d0, R2=033f; input channel523, band4, requested
level-220 (304a86d4=0004ff24), lower bound30032dfc=-215,
calibration30031d34=40. Original gain arithmetic still yields-43,
clamps the requested level to-215, and selects offset172. The original
gain store328,956,359 selects033f, correction328,956,431 writesff5d.
Scratch30029964/68 contains04150800/067f0000, next word0. A calibration
profile is neither an RF-band identifier nor a unique signed correction.

The existing same-profile restart rule now also permits033f under exactly
the prior checks: channel advances1..5, same accepted profile, matching
eight-byte cache,24zero extension bytes. No other cross-profile transition
was added. Decoder history still changes only from fully recognized contiguous
five- or three-request groups. Timers, pending work, idle masks and raw-400
remain unchanged; no firmware/NAND/RTOS/keypad changes or forced IRQs.

verify_rf_result_math.py now coversff5d over every signed16-bit input:
458,797 original ARM slice cases pass. Raw-400 becomes-263 half-dB,
clamps to-220, RXLEV0. The unchanged packer passes41,135 cases, the
collector520. 158Rust tests pass (147library,1CLI,10GUI/runner), including
the new033f restart/history test with real submission phase/deadlines,
idle retention, pending batches, reset, byte mutations, channel bounds,
cross-profile rejection and partial-history rejection. 51Python tests pass.
These verify ARM/HLE contracts, not recovered RD16024 or physical RF behavior.

The intermediate rf-profile033f-restart-600m passed329,404,337 but reached
EA020101 at345,884,338, LR2098ddf3. Final intermediate counters were
54,150,176 fast-forwarded, PC20014b4c,23LCD writes,78standby callbacks,
2,475IRQs, no memory fault. LCD remained byte-identical to the fragment.
Its no-SIM BootReport and LCD match the preceding baseline exactly.

rf-profile033f-restart-next-boundary captures345,445,804, mask03ff,
channels0..4/033f, with06eb0800/067f0000 plus24zeros in every extension.
Original gain/correction stores345,436,363..345,444,004 use band2's
table30031d68, R5=-40,033f/ff60; scratch3002995c+8 retains885/033f.
rf-profile033f-tail-provenance captures the fully accepted preceding tail
at342,726,638: mask3f, complete slots7..5/channels883..885/033f,
zero extensions, original033f/ff5d packer calls342,720,978..342,724,818.
rf-profile033f-restart-tail instead broke at342,880,884 with mask0;
its retained bytes are NOT active tail provenance.

The bounded885-to0..4 transition rule now also permits a cached033f
previous command, retaining exact-byte/trailing-zero/current033f checks.
The new033f tail/range test covers actual phase and publication times,
reset, every extension-byte mutation, channel bounds, wrong current
profiles, and unsupported/noncontiguous preceding tails. 159Rust tests
pass (148library,1CLI,10GUI/runner);51Python tests remain passing.

### Checkpoint — 2026-10-31: verified033d producer, scan restart and second band transition

This checkpoint advances internal RF contracts, NOT the visible SIM-present
freeze. The final LCD is still byte-identical to the startup-animation fragment.
No firmware, NAND, ARM task state, keypad handling or timer units were changed.
Temporary passive DSP-control logging used to find the missing batch was removed
before the final release build. All HLE remains opt-in and image-gated.

rf-profile033d-packer-input stops before2098d854 at301,608,525. The original
input channel is512, band4, requested level-211, calibration30031d34=40,
and lower bound30032dfc=-215. Original20575fe8 selects table30032968
(band4 table30031d08 plus0x63*32). The gain arithmetic is-215+2*86=-43,
selecting033d at offset172; original205760f6 writesff61 (-159).
rf-profile033d-active-snapshot has mask03ff, channels513..517/033d and
zero extensions at301,770,244. The mixed preceding group301,610,286 has
four033f requests and channel512/033d. Both now fully decode.

verify_rf_command.py verifies41,135 unchanged ARM packer cases, adding033d
over all ten-bit channels/bands. Its fingerprint remains
07e523d3b240bb9a300316b6599d9ced0d65a867cff30ae9b748799f5eb4afb6
for2098d854..2098d8a7. verify_rf_result_math.py verifies393,261 unchanged
ARM cases, including all signed16-bit inputs withff61: synthetic raw-400
becomes-259 half-dB, clamps to-220, RXLEV0. verify_rf_collector.py passes
520 original ARM cases. These verify ARM contracts, NOT real DSP output.

The intermediate zero-extension033d candidate passed302,048,154 but reached
EA020101 at304,764,325. rf-profile033d-first-missing-result stops before the
original assertion2098dde6 at304,635,845. This was NOT a new timing failure:
rf-profile033d-controls shows the missing channels523..527 group at304,325,606
was rejected, whereas the later528..532 group304,485,398 was accepted.
All five missing-group extensions contain00 08 15 04 00 00 7b 06 and24zero
bytes: the preceding accepted channel522/033d command. The original packer
scratch3002995c retains this at+8 (rf-profile033d-restart-residue,
304,316,423). Same-profile033d restarts now use the same strict history
checks as0333: advance1..5 channels, exact previous eight bytes,24zeros.
rf-profile033d-history-600m passes that boundary, then reports at321,244,338.

Descriptor IDs must NOT be mistaken for RF channels. In
rf-profile033d-restart-descriptors, original2118e508 increments3049198a
and stores the measurement ID at304a86c8; actual RF channel is at304a86d2.
Descriptor banks304a567c/304a56d4 contain IDs734..738/739..743, not channels.
rf-profile033d-restart-channel directly captures actual channel528 with
preceding ID738. No descriptor state was written by a diagnostic.

rf-profile033d-tail-provenance captures original control at318,086,635,
mask3f, exactly three complete05/00 +02/0a pairs in slots7..5,
channels883..885/033d, zero extensions. Original packer calls318,080,977,
318,082,893 and318,084,815 use scratch30029964. This fully accepted group
provides trusted last-command history; unsupported partial tails do not.
rf-profile033d-next-boundary captures control320,805,689 with mask03ff,
channels0..4/033f and all five extensions06eb0800/067b0000 plus24zeros,
exactly the previous885/033d command retained at scratch3002995c+8.
Original gain/correction stores320,796,363..320,803,833 select033f/ff60.
The existing885/0333 transition exception now also recognizes this captured
885/033d case; no other previous profiles or channel jumps are enabled.

157 Rust tests pass (146 library,1 CLI,10 GUI/runner), including new mixed033d,
strict-extension, same-profile restart and three-request033d transition tests.
The transition test checks exact history, every extension byte, reset,
unsupported profiles, channel bounds and malformed/noncontiguous prior tails.
50 Python tests pass. RF frame timing, pending-work lifetime, zero-mask
handling and synthetic empty-air raw-400 remain unchanged.

Final rf-profile033d-transition-600m:600,000,000 instructions,
54,108,180 fast-forwarded, PC20014b4c,17LCD writes,1HKBD initialization,
61standby callbacks,2,269IRQs, no memory fault; ARM mask3049198c=07ff0000,
shared mask0, battery301ce478=3,704,836. These increased counters do NOT
establish visible progress. LCD SHA256 remains
dfb3bc42c0923246e723a60a1c41ee2444c25202e7f4f44fe40c0c9cf4e8859c;
the rendered PNG was visually inspected and is NOT a SIM standby/menu.
rf-profile033d-transition-next-reporter stops at329,404,337 with
R1=EA020101, LR2098ddf3. rf-profile033d-transition-absent-600m exactly
matches the previous no-SIM BootReport excluding the LCD filename; its PPM
SHA256 remainsa7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07.
No additional dependencies or full RAM/NAND dumps were created.

Next boundary: rf-profile033d-transition-next-boundary stops at328,965,634,
mask03ff, channels523..527/033f, with all five extensions04150800/067f0000
plus24zeros. Scratch30029964/68 has the same previous522/033f bytes.
Original205760b8 gain stores328,956,359..328,963,785 select033f with
table30032968, R5=-43, while205760f6 writesff5d (-163), not earlierff60.
The current decoder rejects same033f nonzero-extension restarts by design.
Before extending it, establish the previous fully accepted522/033f group's
provenance and original packer input/gain selection, then verify ff5d over
the full signed16-bit ARM correction contract. A recognized profile does
not imply a single correction or RF band. Do not broadly accept scratch
residue or seed the missing result. This is the next bounded HLE task.

Verified release GUI/CLI and root README were copied to dist after a read-only
process check found no running user GUI. Launchers and the separate old keypad
binary were retained. GUI SHA256:
daf7d2ec6d7c5eb140de9b18b6942dca9739cb69635a68db4210b83710007335;
CLI SHA256:
2589781ef2c695929aca526290f7466b66dde24e74cb9b113988de9413f78397.
Root/dist README hashes match. gui-profile033d-transition-hle-smoke.png
visually verifies startup, all four explicit experimental mode labels,
the unchanged layout/selectors/keypad and the SIM-menu warning, not a full
GUI boot. The task's own test window was closed after capture; no user
window was closed. Persisted config SHA256 before/after remains
db6a068fd9102f85d7f9442afe16f50f1d88dc9128aadf8f196bc8f2f45ee0b7.
GitHub was not updated. The deferred star-key error was not investigated.

### Checkpoint — 2026-10-31: observed033f band transition and three-request history

The user reports the GUI simply freezes. At inspection, the existing GUI
process7212 was Responding=True; this does not prove guest progress. Prior
native traces establish the original EA020101 fatal reporter, not a Windows
UI deadlock. The final probe below still stops progressing toward standby:
this checkpoint is NOT a fix for the visible freeze or a SIM-present menu.

rf-partial-group-provenance breaks after the original control at293,606,428.
Mask3f has six records and three COMPLETE05/00 +02/0a pairs in slots7..5,
channels883..885, profile0333, zero command extension. The original packer
calls at293,600,867,293,602,759 and293,604,653 use scratch30029964;
the last accepted command is06eb0800/06670000 (885/0333). This is not an
unsupported partial decode of a larger group. At the next band transition,
rf-profile033f-original-input stops before2098d854 with scratch3002995c;
its +8 bytes are30029964, exactly the previously packed885 command. The
8-byte change in stack/scratch origin explains the retained extension.
The old cache updated only five-request groups and consequently lost the
genuine last command of the three-request tail.

Original20575fe8 reads the input band byte2 at304a86d6, channel0 at+2,
and signed requested level-212 at+4. It selects table30031d68 and
channel index31 hex; the original branch for bands1/2 adds49 to channels
<=124. The captured gain store205760b8 at296,156,344 writes033f;
R5=-40, calibration bytes30031d34=40/00, lower bound300321fc=-212.
The original correction store205760f6 at296,156,423 writesff60 (-160).
Final control296,165,776 retains the same ten-record, clear-tag, timer
contract and channels0..4. rf-profile033f-next-group also traces033f/ff60
at channels10..14 with zero extension and the same ARM producer, ruling out
an initial-only channel0..4 profile. Its control snapshot follows the
omitted channels5..9 frame; the latter is observed in final HLE execution.

src/dsp_rf.rs now recognizes033f under the existing strict request decoder.
Zero extensions use the verified original ten-bit channel packing. Nonzero
extensions at this transition require exact cached885/0333 bytes, current
033f, channel0..4, and24 trailing zeros. This is a deliberately bounded,
observed transition exception, NOT generic cross-band residue recognition.
Trusted history now updates from fully recognized contiguous five-request
mask03ff slots7..3 OR three-request mask3f slots7..5 groups. Every active
record must be accounted for; unsupported partial decodes and noncontiguous
masks do not replace history. Clear/idle still retain accepted work/history;
reset removes them. Same0333 restart checks and unknown-profile rejection
are preserved. No firmware patch, ARM RAM seeding or forced IRQ was added.

verify_rf_command.py now includes033f:30,895 unchanged ARM packer cases pass.
verify_rf_result_math.py now includesff60:327,725 unchanged ARM slice cases
pass, covering all signed16-bit inputs. Synthetic raw-400 /ff60 becomes
-260 half-dB, clamps to-220 and encodes RXLEV0. This verifies the ARM
consumer contract only; the empty-air value and DSP timing remain declared
HLE assumptions. 153Rust tests pass (142library,1CLI,10GUI/runner), including
new three-request history, exact band-transition and unsupported-tail tests.
49Python tests pass, including a new033f/band2 command fixture.

An intermediate first-channel-only candidate passed296,604,325 but failed
at296,764,338 on the subsequent channels5..9. That artificial restriction
was removed after tracing the same original producer at10..14 and verifying
the full original ten-bit packer. Final rf-profile033f-final-600m passes
both former barriers and delivers the initial five results296,331,008..
296,447,488. The next fatal is EA020101 at302,048,154, LR2098ddf3,
requested slot7. rf-profile033f-next-boundary establishes a NEW profile,
not another stale-command clear: original301,608,442 gain store writes033d
for channel512, correctionff61 at301,608,514, table30032968/R5=-43.
The mixed preceding group has four recognized033f requests and unsupported
slot3/channel512/033d. The following group uses channels513..517/033d,
zero extension and the same ordered records; original gain/correction/
packer calls are captured at301,760,858..301,764,712.
rf-profile033d-active-snapshot breaks at301,770,244 with mask03ff,
channels513..517, profile033d and zero extensions. The earlier
rf-profile033f-next-boundary watch identifies the producer but its final
303M snapshot has mask0; do not treat those retained bytes as an active
request. These commands are
correctly unanswered until the new profile's producer/math are checked.

Final600M:54,010,016 fast-forwarded instructions, PC20014b4c,6LCD writes,
1HKBD initialization,43standby callbacks,1,927IRQs, no memory fault;
ARM mask3049198c=07ff0000, shared mask0, battery301ce478=3,704,836.
The LCD is BYTE-IDENTICAL to the previous animation fragment, SHA256
dfb3bc42c0923246e723a60a1c41ee2444c25202e7f4f44fe40c0c9cf4e8859c.
More callbacks/accepted RF frames do not establish visible UI progress.
rf-profile033f-absent-600m uses only boot HLE and exactly matches the prior
no-SIM BootReport apart from the LCD filename. No-SIM LCD SHA256 remains
a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07.
No full RAM/NAND dumps or additional dependencies were created.

Verified release GUI and CLI were copied to dist only after inspection found
no user GUI process remaining. No user window was closed by this task.
Source/dist GUI SHA256:
0c63bb6c88c381edcf829ab81946fb41f5e20457d224894299872e5c50177506;
CLI SHA256:
d494aec05a043f01af3a96498a1b4fc8203b7b7115bf21f4bfde0003907497df.
Root/dist README match. gui-profile033f-hle-smoke.png visually verifies
startup, experimental warning and unchanged layout, not full GUI boot.
That separate test window was closed via WM_CLOSE and persisted config hash
was unchanged. Firmware/NAND, keypad code and launcher flags were untouched;
GitHub was not updated. No-SIM report comparison and LCD hash match exactly.

### Checkpoint — 2026-10-31: next-frame RF pipeline and verified scan-restart scratch history

This checkpoint supersedes the older0333 checkpoint's interpretation of the
277.58M consumers and producer-mask clear. Bounded native probes are
rf-empty-cycle-caller, rf-empty-cycle-queue and rf-empty-cycle-first-pipeline.
Original20584d3e..56 packs an 8-byte descriptor (opcode, channel, correction,
slot);20584d64..98 rotates four banks. Original20999530 queues the descriptor
at304a5900 and calls209995fc. Consumer2099a9e0 loads a descriptor pointer
from304a58d4+2c, copies its payload to304a8f44 and calls2098dd98.
The277,595,784..277,599,571 consumers use bank304a572c from the previous
frame, NOT the last channels518..522 group. That last group was packed at
277,442,472..277,450,018 into304a5784; its first actual consumer is at
277,755,784. First RF submission271,685,791 is similarly followed by a
second submission271,845,535 before the first consumers271,995,790.

src/dsp_rf.rs now schedules completion in the next timer frame:
now + saturating_sub((5000 + finish) * units, phase_clocks). This is a
declared HLE timing assumption constrained by the ARM pipeline, NOT recovered
RD16024 timing. Producer clear frees the generation but preserves accepted
pending requests; another submission extends rather than replaces pending
work. Runtime reset removes both pending work and history. A control with
mask0 returns without latching a generation or decoding stale slot bytes.
Result payloads, flags and original IRQ21/collector/ACK delivery are retained;
there is no forced callback, ARM-state write or firmware patch.

Final channels518..522 submit at277,450,301, producer clear at
277,594,124/126 and idle control at277,600,987. Accepted results now arrive
at277,611,008 (slot7),277,640,128 (6),277,669,248 (5),277,698,368 (4),
277,727,488 (3). Original205769f4 collects flagsf8 at277,744,626 and
ACKsff07 at277,744,651. All consumers277,755,784..277,759,567 pass;
the last descriptor opcode is28, LR2099aac3. Earlier current-frame timing
donated later results one frame early and canceled the genuinely pending
last group. This fixes that lifecycle, not an assertion by replaying idle data.

rf-pipeline-restart-control / -full-snapshot / -packer capture the first
restart control at279,845,683: mask03ff, profile0333, channels523..527,
timer0660/5000/0/4700. Its commands have nonzero extension bytes: the first
8 extension bytes are the previous channel522/profile0333 pair
04150800/06670000, followed by24 zero bytes. Original2098d854 writes only
8 bytes of scratch3002995c; callback20d94ddc is followed by original
20143dea copying all20 halfwords into each 40-byte shared slot. This is
scratch residue, not a second newly packed measurement. The next restart
at288,005,638 uses channels708..712 and previous channel707/profile0333
residue, with the same original gain/correction producer.

The decoder accepts that extension ONLY when it exactly matches a cached
previously accepted slot3 command, current channel increases by1..5, current
profile is0333, both second words match and the remaining24 bytes are zero.
History is updated only from a completely recognized five-request slots3..7
group. Idle and producer clear retain it; runtime reset removes it. Partial
or unsupported groups cannot replace the cache. Unknown residue/profiles,
band tags, record shapes and timer modes remain rejected. No hard-coded
channel522/707 exception or unvalidated ignored extension is retained.

tools/verify_rf_command.py runs the unchanged ARM packer in an isolated
synthetic harness:20,655 cases pass across observed bands, channels and
calibration edges, including preserved extension and caller R4/SP. Code
2098d854, size0x54 including literal2098d8a4, SHA256
07e523d3b240bb9a300316b6599d9ced0d65a867cff30ae9b748799f5eb4afb6.
This verifies the ARM packing/copy contract, not DSP interpretation or timing.
verify_rf_collector.py passes520 original ARM cases; verify_rf_result_math.py
passes262,189. The Python suite passes48 tests, including four new harness
tests. Rust passes149 tests (138library,1CLI,10GUI/runner) covering accepted
work lifetime, frame overlap, idle, reset, exact residue and history provenance.

Final rf-pipeline-history-600m uses all four explicit HLE flags. Restart
results arrive at280,011,008..280,127,488 and288,171,008..288,287,488,
passing the former280.284M/288.444M fatal boundaries. A later partial group
of three requests is accepted at293,606,428. The next fatal EA020101 is at
296,604,325, LR2098ddf3, requested slot7. It is NOT established that the
partial group caused the fatal. rf-pipeline-third-restart-snapshot captures
the new control at296,165,776 with mask03ff, channels0..4, calibration033f
and correctionff60 (-160). Gain table base30031d68, producer R5=-40;
slot7 command00010800/067f0000. Each extension retains channel885/profile0333
(06eb0800/06670000) plus24 zero bytes. This new profile/channel relation is
unsupported and correctly unanswered; trace its original math and residue
history before extending the model, especially after the partial final group.

The600M BootReport has53,986,824 fast-forwarded instructions, PC20014b4c,
4LCD writes,1HKBD initialization,21standby callbacks,1,859IRQs and no memory
fault. Battery301ce478=3,704,836; ARM mask3049198c=07ff0000, shared mask0.
LCD SHA256 dfb3bc42c0923246e723a60a1c41ee2444c25202e7f4f44fe40c0c9cf4e8859c.
rf-pipeline-history-600m.png was visually checked: yellow/orange spotlights
on black, the Samsung word has cleared. This is an original animation
fragment, NOT a SIM-present menu. No-SIM regression rf-pipeline-final-absent-600m
uses ONLY E830_DSP_BOOT_HLE=1 and exactly matches the previous no-card
BootReport except LCD artifact name:54,168,644 fast-forwarded,26LCD,
48standby callbacks,11,792IRQs; stderr is empty. LCD SHA256 remains
a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07.
RF/analog-off baseline also matches its prior full report. No firmware/NAND,
persisted paths, launcher flags, dependencies or full RAM dumps were changed.
The GUI notice now describes incomplete experimental SIM boot without the
outdated claim that every HLE candidate stays on the Samsung word.

Verified release GUI/CLI are deployed to dist; dist/README.md matches the
root README. GUI source/dist SHA256:
f35c3608bc4728eb0e65749e458fd02e4037f08e3f5c2f5b99e20c99b7934b6b;
CLI source/dist SHA256:
757a7760fb519868dfb4057e0201b9b1c661a55a3c683b72908cf2c34ff5ac19.
gui-pipeline-history-hle-smoke.png visually verifies startup, corrected
experimental-mode warning and unchanged LCD/keypad layout. This is a short
startup check, not a full GUI boot. The test window was closed through
WM_CLOSE; no emulator processes remain and persisted config SHA256 is
unchanged. The old separate keypad binary and all launchers remain untouched.
GitHub was not updated. Star-key INSN_INVALID remains explicitly deferred.

### Checkpoint — 2026-10-31: RF HLE supports the observed0333 gain profile

Historical checkpoint: its final-group consumer identification and clear-mask
lifetime interpretation are superseded by the next-frame checkpoint above.

rf-profile0333-gain watches the original20575fe8 gain producer,
calibration store205760b8, correction store205760f6 and2098d5c8 schedule
packer after277M. Slots7..4 select channels1020..1023 with0335/ff74;
the final call at277,128,372 selects channel512. At277,128,415 the producer
writes0333 to30029950, using table base30032968 and channel index0200.
At277,128,487 it writesff75 (-139) to30029a9c. The unchanged ARM packer
then stores the same ordered05/00,02/0a request in slot3. Active mask03ff
is captured at277,130,287. Slot3 words are04010800 and06670000: the
previous checkpoint's06668000 was a transcription mistake; both original
and new CLI reports contain decimal107413504, i.e.06670000.

src/dsp_rf.rs accepts only0335 and0333 with the existing strict command,
clear-band-tag, zero-extension, ordered-record and timer-mode checks.
Calibration is not interpreted as a band ID. Synthetic signed raw-400
remains an explicit empty-air assumption, not recovered RD16024 output.
The original correction consumer gives-239half-dB withff75 (-240 withff74),
then the original numeric clamp gives-220 and RXLEV0. The diagnostic
verify_rf_result_math.py executes262,189 cases against fingerprinted,
unchanged ARM slices, covering all65,536 raw values with each correction
and the clamp/encoding contracts. verify_rf_collector.py still passes520
cases. No firmware patches, ARM masks/results, forced IRQs or callbacks
are added. Payload/flag ordering, ACK retention, mask-clear cancellation
and once-per-generation behavior remain unchanged.

rf-profile0333-600m proves all five flags in the previously failing group:
slot3 publishes at277,247,488; original collector205769f4 readsf8 at
277,264,629 and ACKsff07 at277,264,654. The guest passes277,408,137 and
packs further0333 channels; the next observed group uses channels518..522
with the same0333/ff75 producer contract. All five results in that group
are collected at277,584,626 and ACKed at277,584,651.

The new boundary is NOT another unsupported calibration. Bounded
rf-profile0333-schedule-lifetime records the final ten-record mask03ff at
277,449,203, then its original clear205759dc/e0 at277,594,124/126. Five
original2098dd98 consumers read that completed group at277,595,784 through
277,599,571. No gain/command/record producer writes a new group afterwards.
The periodic50505000=40 store at277,600,987 therefore sees mask0,
confirmed by rf-profile0333-next-active. Original IRQ21 collector reads
flags0 at277,744,626; the mask is cleared again at277,754,124/126 and
the next2098dd98 call at277,755,784 requests slot7 without a result.
rf-profile0333-next-fatal stops at277,884,307, LR2098ddf3, R1EA020101,
requested byte304a8f48=7, ARM mask3049198c=07ff0000. The reporter watch
is277,884,308. Retained command bytes are stale, not a new request.
Do not replay them, ignore a clear mask or seed a result to pass this check.
Next work is the original request/callback/result-consumer lifecycle across
the end of that scan batch; full RF/demodulation behavior remains absent.

The600M BootReport has53,946,420fast-forwarded instructions,1LCD,
1HKBD initialization,5standby callbacks,1,605IRQs and no memory fault.
301ce478 remains3,704,836. LCD SHA256 remains
9140970baf8c6ef39893e9ac6469a27b76ce64c9cfaaace084ed10e41638f4f5;
visual inspection confirms the initial Samsung splash, NOT a SIM menu.
rf-profile0333-absent-600m exactly matches the earlier no-card BootReport
and LCD bytes (apart from the artifact filename), SHA256
a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07.
rf-profile0333-off-baseline exactly matches the full prior RF/analog-off
report:272,124,313instructions,53,837,316fast-forwarded,1,576IRQs.
143Rust tests (132library,1CLI,10GUI/runner) and44Python tests pass.
No full RAM/NAND dumps or additional dependencies were created.
Current release GUI/CLI are copied to dist, and dist/README.md matches the
root README. The source/dist GUI SHA256 matches
5fed04b5fc8d94e67145eccd43b875dae918d886a76682466e24745c1d39e327;
CLI matches8aac5a7be25f484e660d0b9a6f64ada83c5c32a11dede6e7c8a9ca4b8690725c.
gui-profile0333-hle-smoke.png verifies startup, mode warning and unchanged
LCD/keypad layout; this short GUI check is not a full boot test. Its test
window was closed through WM_CLOSE. No existing user window was replaced,
no firmware/config/NAND or launch flags were changed, and GitHub was not
updated. The reported star-key INSN_INVALID is recorded under Next work
and deliberately left uninvestigated at the user's request.

### Checkpoint — 2026-10-31: GUI refreshed with explicit no-SIM / experimental HLE launches

At the user's request, the current GUI is deployed to dist/e830-emulator.exe
along with the current CLI. The source adds a read-only environment-mode
label and warning in Library and Run output, outside the guest LCD. No
firmware/board behavior is changed by this GUI patch; the desktop controls,
NAND/firmware selection, F11, keypad and persisted paths remain unchanged.
The displayed modes use exactly the same environment tests as the board.

Run-E830-NoSIM.cmd delegates to Run-E830-Keypad.cmd: DSP boot HLE on, SIM,
RF and battery HLE cleared. The keypad launcher now uses the updated main
binary, leaving the already open old keypad process and binary untouched.
Run-E830-HLE.cmd enables the four experimental flags; its GUI warning states
that the SIM candidate still remains on the Samsung splash. Run-E830.cmd
preserves its previous SIM-launch choice by delegating to this explicit HLE
launcher. No persistent machine/global environment setting is changed.
The established no-card Insert SIM / SOS path remains verified; this update
does not establish a SIM-present standby or application-list navigation.
140Rust tests (129library,1CLI,10GUI/runner) pass, including mode-label tests
and existing settings/NAND/input/worker-lifecycle coverage. The selected
NAND/config and private firmware are not replaced; GitHub is not updated.
42Python diagnostic tests also pass. Native GUI smoke captures
gui-refreshed-nosim-smoke.png and gui-refreshed-hle-smoke.png verify both
environment labels, warning, LCD/keypad layout and successful startup.
These short captures show the initial splash, not completed GUI boot. Both
test windows were closed via WM_CLOSE; the user's old keypad window remains
open. The main dist/source GUI hashes match SHA256
0e70c9eb1e69ee624a9dae521c2335e548a530af48747083ff66aca7d862a7ee.
tools/capture-window.ps1 accepts an optional output path so new verification
does not overwrite the existing gui-current.png artifact.

### Checkpoint — 2026-10-31: RF HLE frame phase passes the late-result boundary

The previous late-slot failure was a control-relative synthetic timing error.
rf-frame-phase-baseline records submission274,731,848 with timer raw2249,
remainder0 and last-update274,731,848. The guest timer is already562.25
integer units into its frame; writing50505000=0040 does not reset it.
At8 elapsed guest instructions per raw fractional tick and4 raw ticks per
integer unit, the current frame origin is274,713,856. Slot3's finish4176
therefore corresponds to274,847,488, not274,865,480. These time origins and
schedule-unit correspondences remain **declared HLE assumptions**, not a
recovered DSP/hardware timing contract.

The opt-in board now projects the current timer phase non-mutatingly from
timer_counter, timer_last_update and timer_clock_remainder, including elapsed
instructions and periodic wrapping. Only the observed mode0660 (ignoring
latched status bits), period5000, match0=0 and match1=4700 is supported;
other timer modes do not submit synthetic work. Compare-register flag bits
are masked as in the existing timer. The model deadline is submission time
plus max(0, finish*32 - current-frame-phase-in-instructions). Already elapsed
events become due at the next normal board advancement, with no next-frame
replay, forced interrupt or ARM result-mask write. Publication still writes
the device payload before its flag, and original IRQ21 performs collection.
Guest schedule-mask clear still cancels pending work and opens a new generation;
ACK retains payload and does not resubmit. Default/image gates are unchanged.

rf-frame-aligned-600m proves the former failing frame now succeeds:

- Submit274,731,848 projects phase17,992 elapsed instructions. Slot7's
  deadline was already elapsed, so publication occurs at274,731,849.
- Slot3 is published at274,847,574 (first normal advancement past its
  computed deadline, after a fast-forward interval).
- Original collector205769f4 enters274,864,828 and reads5050501c=f8 at
  274,864,830, acknowledgingff07 at274,864,855. All five results are present.
- The guest continues beyond the old275,008,137 EA020101 boundary, with
  repeated successful frames. Other-channel70012c01 and70020f00 diagnostic
  reports remain; their presence alone is not proof of a halt.

The next failure is now a different, **active unsupported calibration**.
rf-frame-next-active stops immediately after the control store205747f2,
at277,130,287. Mask60000658=03ff selects ten records with the same ordered
05/00 then02/0a pairs, zero arguments and finish times536..4176. Slots7..4
have channels1020..1023 with calibration0335; slot3 has channel512, clear
band tag, calibration0333, and zero command extensions. The exact slot3
command words at60000734/738 are04010800 and06670000. HLE intentionally
accepts only four slots here; do not interpret this as another late-event
failure or accept arbitrary calibration values to silence the assertion.
inspect_dsp_schedule.py decodes this active snapshot without a RAM dump.

The long run reports EA020101 at277,408,137 (watch entry), LR2098ddf3, and
retains the original splash through600M. Its BootReport has600,000,000
instructions,53,943,192 fast-forwarded,1LCD equivalent,1HKBD init,5standby
callbacks and1,599IRQs, no memory fault;301ce478 remains3,704,836. LCD SHA256
9140970baf8c6ef39893e9ac6469a27b76ce64c9cfaaace084ed10e41638f4f5 equals
the previous analog-HLE splash. Passing this timing boundary is **not** a
SIM-present menu fix or full RD16024/RF emulation.
rf-frame-next-fatal confirms the breakpoint itself at277,408,136,
53,943,184fast-forwarded, requested slot byte304a8f48=3 and ARM mask
3049198c=07ff0000 (low halfword0), with60000658 already cleared. Its retained
0333 command is interpreted using the earlier active snapshot, not replayed.

rf-frame-aligned-absent-600m exactly matches the complete earlier
analog-hle-final-absent-600m BootReport and LCD bytes (SHA256
a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07).
rf-frame-off-baseline, with RF and analog HLE disabled, exactly reproduces
the complete sim-rf-hle-final-off report (272,124,313instructions,
53,837,316fast-forwarded,EA020101,1,576IRQs). rf-frame-off-exact enables
analog HLE while leaving RF off: the same first RF assertion is reached at
272,124,313, but fast-forward/IRQ counts differ (53,837,908/1,524). This is
not an identical configuration and must not be labeled an exact regression.
139Rust tests (129library,1CLI,9GUI/runner) and42Python tests pass. New tests
cover the observed deadline, already elapsed events, fractional remainder,
single/multiple frame wraps, unsupported mode rejection, guest control hooks,
cancellation and preservation of IRQ/ARM/CPU state. Release source is rebuilt
locally; dist GUI binaries/launchers and the publishing mirror remain unchanged.

### Checkpoint — 2026-10-31: serial analog HLE passes the power-range check

The producer of the previous70012904 parameter is now established. The
original215a265c ADC-message handler reads a raw halfword through its incoming
descriptor, finds the original request, calls2118715c on channel1 and sends
the converted word at215a26ce. The channel1 calibration is signed12(raw)*4126
-421164, using30030df0/30030df4. Raw0 therefore produced fff992d4 exactly.
The recipient215a3c4c stores that value at301ce478 and215a3b14 requires
2,500,000..5,000,000. Neither the message nor the assertion is patched.

The raw producer2118cde0 loads an intermediate packed word from30491b4c,
shifts it right6 and stores a halfword at2118cdee (3197cee0 in the probe).
Original serial helper200980b0 builds that packed word from
LDRH[50301018]<<6 | ([50301004]&3f). The zero is a missing peripheral
measurement, not an unknown DSP mailbox value. Proof is in the bounded
analog-raw-origin, analog-wire-result and analog-serial-contract probes.
The observed transaction at260,158,388..260,162,527 is:

- 50301004=3b,50301008=0,FIFO50301100=ec0.
- Halfword5030100c=4 starts the observed packed one-word transfer.
- The existing delayed serial completion sets50301014 bit4; the original
 IRQ9 ISR writes control0 and W0C ACK3f8, then reads50301018.
- Another observed address3d transaction uses FIFO0f40 and must not receive
 the battery sample.

`src/analog_hle.rs` is separately default-off, enabled only by
E830_ANALOG_HLE=battery and the recognized-image DSP boot runtime. It answers
only the exact halfword-control4,address3b,extension0,FIFOec0 contract.
Its fixed raw1000 is a **declared synthetic environment assumption**, not a
recovered ADC hardware value. The observed ARM calibration yields3,704,836,
consistent with about3.7V if expressed in microvolts; physical units and the
fitted analog chip remain unverified. No temperature/other-channel response,
charging state, RF command or synthetic guest-message injection is added.

The sample is published as a transaction-local device read overlay before
the existing serial completion, not by an extra/forced IRQ or callback.
Control0 and W0C ACK preserve the result for the actual ARM read. A new
control/submission or rewritten address/extension/FIFO cancels old work and
invalidates the old result. Unsupported/malformed/unaligned requests retain
the original register behavior. Tests cover delayed completion, INTC masks,
stop/ACK, one-shot consumption, cancellation, default/image gates, address3d
isolation and real guest control stores without modifying RAM/CPU context.

The first bounded candidate analog-hle-battery-first reaches reporter200141c0
at274,644,238instructions,53,939,772fast-forwarded, with R1=70012c01,
LR215a3bf1,1LCD,1HKBD,5standby callbacks and1,556IRQs. The watch proves
215a3b14 receives00388804 at274,643,508 and peek301ce478 is3,704,836.
Thus70012904 is passed under the synthetic battery assumption. Reporter
200141c0 alone is **not evidence of a fatal halt**:70012c01 is also an earlier
no-card diagnostic. Here215a3bd8 sees04c5201e, outside its signed range
[-40,000,000,75,000,000]. The long run continues past that report and70020f00,
then reports EA020101 at275,008,137 with LR2098ddf3. The600M run remains on
the original splash, with1LCD,1HKBD,5standby callbacks and1,567IRQs, no memory
fault. It does not reach a SIM-present standby/menu. The next RF request must
be inspected before changing the synthetic timing contract.

analog-hle-off-regression exactly reproduces the entire prior
sim-rf-hle-generation-final BootReport (including274,645,712instructions and
70012904). analog-hle-absent-600m matches the complete prior no-card report
and LCD bytes:26LCD,48standby callbacks,11,792IRQs; LCD SHA256
a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07.
The release source is rebuilt locally; dist GUI binaries/launchers and the
publishing mirror are not deployed or pushed as part of this experiment.
The final setup-cancellation build passes136Rust tests (126library,1CLI,
9GUI/runner) and42Python diagnostic tests. analog-hle-battery-final-600m
reproduces the entire prior candidate BootReport and LCD bytes exactly.
analog-hle-wire-final records the real driver reading50301018=03e8 for
address3b at260,002,527, and0 for address3d at260,007,397 and260,160,510.
Packed30491b4c isfa3b (1000<<6|3b). The wire probe's later breakpoint at
215a26ce catches the other channel: its zero raw peek must not be interpreted
as an absent battery result. CLI--peek on MMIO reads backing memory rather
than invoking peripheral reads; the MMIO trace is the sample-read evidence.
The final off/no-card probes analog-hle-final-off and
analog-hle-final-absent-600m again match the prior complete reports, and the
no-card LCD hash remains identical.

The next RF boundary is now captured in analog-hle-next-rf-fatal and
analog-hle-next-rf-lifetime. Reporter EA020101 is reached at275,008,136
(the passive watch counts its entry as275,008,137). The requested slot byte
at304a8f48 is3, ARM result mask3049198c has low halfword0, and device flag
5050501c still has bit08. The active schedule mask60000658 is already0;
stored commands/records there are stale, not proof of an unsupported request.
The lifetime trace identifies the actual timing miss:

- Frame submission274,731,848 schedules the final slot3 completion at
 274,865,480 under the current control-relative32-instructions/unit model.
- Original collector205769f4 enters274,864,828, reads device maskf0 at
 274,864,830, writes ARM maskf0 at274,864,851 and W0C ACKff0f at274,864,855.
- HLE slot3 completes at274,865,480, about650instructions after the collector
 sampled flags. The original schedule mask clears at274,874,124/126.
- Original consumer2098dd98 consumes slots7..4, reducing ARM maskf0 to0,
 then requests slot3 at274,879,577 without a collected result and reports
 EA020101. This is a **synthetic frame-timing mismatch**, not established
 evidence of a new unsupported command. Do not widen calibration decoding
 or inject a result into the ARM mask to work around it. Next work is to
 align the declared HLE schedule with the original frame/collector timing.

### Checkpoint — 2026-10-31: experimental RF HLE reaches the next original check

Following the user's explicit choice to continue HLE, `src/dsp_rf.rs` adds
an experimental, default-off RF model. The prior requirement to find a full
ISA before implementation does not govern this HLE experiment. No RD16024
ISA/peripheral semantics have been recovered: ARM request/consumer facts
are established; the environment and timing remain model assumptions.

Enable `E830_SIM_PRESENT=1`, `E830_DSP_BOOT_HLE=1` and
`E830_DSP_RF_HLE=empty`. The model also requires the recognized-image runtime
phase. At a halfword0040 store to50505000 it snapshots the active schedule
and slot commands. A slot is accepted only with exactly ordered class05/00
and class02/0a records, zero arguments, increasing times below5000, the
2098d854 packer shape, clear band tag, calibration0335 and zero command
extensions. Unknown, partial, duplicated and inactive/stale requests do not
complete; unrelated slots remain untouched.

Explicit assumptions, **not hardware discoveries**:

- `0040` is a synthetic frame boundary. Schedule time is interpreted relative
 to that store, in integer timer units (32 elapsed guest instructions/unit).
 Completion occurs at the second record's time. Physical origin/units remain
 unverified; this does not establish how actual RF scheduling works.
- The environment has no transmitters/network/demodulation. Its fixed signed
 raw measurement is-400. With observed ARM correction-140 this yields
 -240half-dB (-120dBm), then the original clamp produces-220/RXLEV0.
 This is a declared synthetic level, not a recovered DSP no-signal value,
 not zero (which looks like-70dBm), and not an unavailable sentinel.

Each operation completes once per generation. Identical control rewrites
do not resubmit retained storage; the original guest mask clear cancels
unfinished work and ends the generation. Publication writes only the indexed
halfword600000dc+2*slot before ORing its device-owned bit at5050501c.
Existing timerIRQ21/original ARM collector handle delivery. No forced IRQ,
guest callback, ARM descriptor/RTOS mutation or firmware patch is introduced.
W0C ACK does not erase payload, since original ARM reads it afterward.

`sim-rf-hle-first` reaches273M without the former EA020101:
53,838,608fast-forwarded,1,588IRQs,no memory fault,still1LCD equivalent.
First submit271,685,823; slot7 completes271,702,975; slot3 completes271,819,455.
The original request stream continues beyond the former fatal point. This
proves progression under the model, not RF accuracy or a working menu.

The bounded600M candidate `sim-rf-hle-600m` stops at the next original fatal:
274,645,712instructions,53,939,180fast-forwarded,PC200141c0,R1=70012904,
LR215a3b39,no memory fault,1LCD equivalent,1HKBD init,5standby callbacks,
1,608IRQs. The LCD remains the genuine initial splash. Static215a3b14
requires its signed32-bit argument in[002625a0,004c4b40]
(2,500,000..5,000,000); here R4=fff992d4 (-421,164). Caller215a3c4c takes
message word+0xc, stores it at301ce478 and calls that check at215a3ce2 on
the type0 branch. Its physical meaning/producer remains unestablished:
do not substitute a passing constant or disable the assertion.

The final build reproduces that full next-fatal BootReport exactly in
`sim-rf-hle-final-trace`. Its80-hit mixed watch budget is exhausted by the
RF consumer before the message handler, so it cannot support an absence
claim for that handler. The separate40-hit `sim-rf-hle-message-trace` records
only3hits:215a3c4c at274,643,483,215a3cd6 at274,643,527 and215a3b14 at
274,643,534. The message pointer is3197faa0 and the check's actual R0 is
fff992d4; final peek301ce478 confirms that word and301ce488 is35.
This establishes the next message/check boundary without identifying its
upstream producer. `sim-rf-hle-final-off` again matches the entire previous
EA020101 baseline; watch logging and the restricted calibration decoder
do not change the next-fatal run.
The final once-per-mask-generation guard also reproduces the entire same
next-fatal report in `sim-rf-hle-generation-final` (33.205s), including
274,645,712instructions and70012904. Changed command storage cannot create
a second submission before a guest mask clear, even after completion.

`sim-rf-hle-off-regression` matches the entire prior
`sim-result-w0c-final` BootReport exactly, including272,124,313instructions
and EA020101. `sim-rf-hle-absent-600m` matches the prior
`sim-result-w0c-absent-600m` report and full LCD PPM bytes:600M instructions,
26LCD equivalents,48standby callbacks,11,792IRQs. LCD SHA256:
a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07.
The final calibration-restricted build's `sim-rf-hle-final-absent-600m`
again matches both the complete no-card BootReport and LCD bytes exactly.

Ten new Rust tests cover decoding/bounds, unsupported/inactive requests,
band/calibration rejection, delayed/once-only lifecycle, cancellation,
default-off/image gates, payload/ACK preservation and unchanged ARM RAM/
context, including actual guest MMIO submission/mask-clear stores.
All127Rust tests pass (117library+1CLI+9GUI), and all42Python tests pass.
Native release binaries are rebuilt in target/release only. Existing dist
GUI, launchers/layout/pickers, firmware and source NAND are unchanged.
No GitHub push, full RAM/NAND/DSP capture or large dependency was added;
about5GiB free remains. This is not deployed as a verified SIM/menu fix.

```powershell
.\build-windows.ps1 -Test
python -B tools/run-probe.py --env E830_SIM_PRESENT=1 `
  --env E830_DSP_BOOT_HLE=1 --env E830_DSP_RF_HLE=empty sim-rf-hle-check -- `
  --max-instructions 275000000 --break-at 0x200141c0 --break-after 271000000 `
  --onenand-image proof/native-windows/direct-call-side/tfs-v4/full-tfs-2k.onenand.bin
```

### Checkpoint — 2026-10-31: external ISA search and isolated RF collector contract

SIM is **not fixed**. No Rust board behavior, DSP execution, result flags,
firmware, RTOS state or GUI binaries are changed. The external search did not
yield the RD16024 opcode/ASI/peripheral reference or compatible SDK. The new
verification is an isolated original-ARM consumer test, not a producer or a
phone-boot workaround. Synthetic masks/payloads stay in that separate harness
and are never injected into a boot, saved as dumps or used as RF evidence.

Exact-name Internet Archive queries for RD16024, RD16000, R.E.A.L.+DSP and
Adelante+DSP/Semiconductors returned no items. Its broader Philips/software
query found CodeWarrior for **TriMedia**, not an established RD16024 SDK;
that package was not downloaded. GitHub searches for the exact core names,
Adelante+Philips/assembler and R.E.A.L.+Philips found no matching implementation.
Short code-search token matches belong to unrelated architectures or data.
OpenAlex/Crossref searches likewise did not identify an RD16024 ISA document.
Google supplied redirect-only pages; Brave/DuckDuckGo/Yandex supplied
CAPTCHA pages; NXP and the Bitsavers Philips subdirectories denied access.
These responses do not prove documentation does not exist. No CAPTCHA or
access restriction was bypassed and no large SDK/firmware archive downloaded.

Google Patents initially exposed usable descriptions with independent Philips
architecture references, before later requests began returning HTTP503:

- [US20060107028A1 — Loop control circuit for a data processor](https://patents.google.com/patent/US20060107028A1/en)
  describes conventional Philips R.E.A.L. DSP loop-end pre-initialization and
  initiation coupled to loading the loop counter. Its proposed loop-start
  field is contrasted with that conventional processor. It is **not** an
  RD16024 opcode table; do not apply the invention's encoding to this image.
- [EP0820006A2 — Signal processor](https://patents.google.com/patent/EP0820006A2/en)
  discusses compressed program commands, citing Philips
  **PCF5083/KISS** as an example. No compatibility with RD16024 or the E830
  uploaded image was established. Its examples cannot define E830 ASIs.
- [WO2002003559A1 — Analog interface in systems using time-division-duplex](https://patents.google.com/patent/WO2002003559A1/en)
  separates an ARM, bit-manipulation/RF timing blocks and a REAL DSP in a
  **DECT/TDD** design. This is not an E830 GSM register map or RF schedule;
  it cannot establish which E830 block executes class05/00 or class02/0a.

The executable local check `tools/verify_rf_collector.py` fingerprints the
144-byte original collector/literal region205769f4..20576a83 before running
it in isolated Unicorn memory. SHA256 is
9d1d64d5e17269f984c3660ee303fd7724b472d70da74f51a5b1afb2857704b9.
It verifies **520 synthetic cases**: all256 low-byte masks against two prior
accumulated masks, plus eight upper-bit/edge cases. Payloads vary in every
case and include both signed-halfword ranges. No part of the firmware image
is changed. The original instructions establish this access order:

1. Read halfword5050501c, retain only its low8bits, read timer50501000 and
   record1703/00ff/flags/timer into the original ARM event stream.
2. OR the low8bits into halfword3049198c, preserving all prior mask bits.
3. **Only for nonzero low8bits**, write their halfword complement to5050501c
   at20576a2a, then record1783/flags/a second timer read.
4. **Unconditionally, including with zero flags**, copy8halfwords
   600000dc..600000ea verbatim into304919a0..304919ae, then30halfwords
   600000fc..60000136 into304919b0..304919ea. Source memory is unchanged.

The harness checks precise MMIO/payload access order, event words/pointer,
source preservation, destination boundary guards and stack restoration.
An initial harness run omitted the separate3001a000 page and failed before
execution; adding that isolated mapping allowed all520cases to pass. This
was a verification-harness issue, not a new phone-boot fault.

**ACK precedes payload reads.** A future producer must not erase its result
payload immediately on this flag acknowledgement: original ARM reads it
afterward. Exact latch lifetime and the next operation's overwrite rules
remain unknown. Unconditional zero-flag copying is not completion evidence,
and this test does not establish raw measurement units/no-signal output.
Eight new synthetic unit tests pass; the full Python suite passes42tests
(37 DSP/RF checks plus5NAND-tool tests). Rust was not rebuilt/retested because
no Rust changed. The prior native117-test result is not a new verification.

```powershell
python -B tools/verify_rf_collector.py firmware/E830XEGG1.cla
python -B -m unittest discover -s tools -p test_verify_rf_collector.py -v
python -B -m unittest discover -s tools -p 'test_*.py' -v
```

No new boot probe, full RAM/NAND capture, DSP dump, GUI deployment or GitHub
push was performed. The next implementation prerequisite is still an actual
RD16024/ASI/peripheral specification, compatible SDK or reference-device RF
trace; the new consumer check constrains that implementation but supplies
neither instructions nor an invented result.

### Checkpoint — 2026-10-31: DSP upload identity and original loader write verified

SIM is **not fixed**. This step adds a read-only upload audit and synthetic
tests, not an RF-result generator or DSP instruction interpreter. No Rust
board behavior, firmware, flags, IRQs, RTOS state or GUI binaries are changed.
The purpose is to identify the actual uploaded code before trying to locate
its missing measurement producer; loading the program is not executing it.

The signature0305 branch at2098c270 selects the CLA source blocks:

| Boot space | CLA byte address | DSP word start | Halfword count |
| --- | --- | --- | --- |
| 0 | 20bf90be | 0000 | 6000 |
| 1 | 20c050be | 7000 | 1000 |

The raw space0 source hash is062e4f73ee9e333f, different from the existing
recognized upload hashb25c3385c96d670d. This is **not** a transfer bug. Before
DSP start, the original ARM loader sends command1, wordaddress0014, count1,
and4634, replacing sourcehalfwordface at that one address. Static call sites
2098c95e/c9a2/c9e6/ca2c establish the protocol; the bounded
`dsp-upload-runtime-word` watch records exactly four hits under its8-hit
budget, R0=1/14/1/4634 at instructions223,951,030/059/088/118. This is
original guest behavior, not an emulator patch to the CLA. The purpose of
this word and the DSP instruction encoding around it remain unestablished.

`dsp-upload-before-start` stops before2098cd5e (the start-command setup) at
224,111,207 instructions,25.016s, no memory fault. The CLI exports two
128KiB little-endian word-addressed boot-memory snapshots. The first contains
exactly the6000-halfword source block plus that single original loader write;
space1 contains exactly the1000-halfword block at7000. Nonuploaded areas are
zero in the CLI's sparse-storage export. Their FNV64 hashes match the existing
image checks: b25c3385c96d670d / e39e195e7cdc9922. These space numbers do **not**
identify the RD16024 program/X/Y memory or E830 ASI/peripheral mappings.

`dsp-upload-audit-sim` captures the same two spaces at the established SIM
fatal breakpoint,31.449s. Both full128KiB snapshots are byte-for-byte identical
to the before-start captures. Its complete BootReport exactly equals
`sim-result-w0c-final`:272,124,313 instructions,53,837,316 fast-forwarded,
PC200141c0,R1EA020101,LR2098ddf3,no memory fault,1 LCD equivalent,1 HKBD init,
3 standby callbacks,1,576 IRQs. Thus this investigation changed no boot
behavior; it confirms upload identity, not completion of RF work.

The preliminary `dsp-upload-audit` run is **not** an upload-stage capture:
with SIM absent it reaches its145M instruction budget before hitting the
chosen loader PC, and both exported spaces are still zero. Do not use those
zero captures as evidence of corrupted transfer or a later missing overlay.
The validated SIM-enabled before-start breakpoint supersedes that attempt.

New `tools/inspect_dsp_upload.py` fingerprints both original CLA blocks,
reconstructs the observed loader write only in its local reference copy,
optionally checks full CLI snapshots and displays raw DSP halfwords with
word addresses. It rejects wrong firmware, incomplete captures, changed
words and nonzero bytes outside the established upload. It neither writes
firmware/dumps nor decodes ARM/DSP instructions, invents a result, or changes
the emulator. Source contains only addresses/hashes, not retail instructions.
Fourteen synthetic upload tests pass; the8 schedule and7 ARM arithmetic
tests also pass (29 Python tests total). Rust was not rebuilt/retested in this
step because no Rust code changed; the previous117-test result remains the
previous checkpoint's verification.

```powershell
python tools/inspect_dsp_upload.py firmware/E830XEGG1.cla `
  --space0 proof/native-windows/dsp-upload-before-start-space0.bin `
  --space1 proof/native-windows/dsp-upload-before-start-space1.bin `
  --space 0 --words 0x10 --count 16
python -m unittest discover -s tools -p test_inspect_dsp_upload.py -v
```

ASCII inspection of the two source blocks (also swapping bytes and examining
the separate byte lanes) finds no usable ISA names, debug symbols or RF
routine strings. Repeated byte patterns and short apparent strings in binary
are not instruction definitions. Additional GitHub queries for RD16000,
RD16024, Adelante DSP and2100/NOP find no matching implementation; broad
code search returns unrelated token matches. Web searches return redirect/
challenge pages or irrelevant snippets and supply no usable documentation.
No new external implementation or reference-device evidence was acquired.

The next missing prerequisite remains an actual RD16024 opcode/ASI/peripheral
reference, compatible assembler/simulator, or a reference-device trace for
the active RF schedule and calibrated result. The loader write explains the
image fingerprint but cannot supply class05/00 or class02/0a execution,
frame timing, no-signal output or indexed completion. Do not reinterpret the
loader word as a successful RF response or add guessed opcodes/results.
Six small DSP snapshots (768KiB total) stay local; no full RAM/NAND capture,
GUI deployment or GitHub push was performed. About5GiB free disk remains.

### Checkpoint — 2026-10-31: schedule lifetime and RF serial path separated

SIM is **not fixed**. This checkpoint changes investigation notes only, not
board behavior, firmware, result values/flags, IRQs or GUI binaries. The new
probes distinguish the periodic shared schedule from a real, earlier RF
serial submission path. Repeating the same zero-result boot does not supply
the missing RD16024 execution semantics.

`sim-schedule-control-order` watches205759dc,2098d51c,201422fc,205747f2
and2118d408 after271.650M, with a40-hit budget. It records exactly9 hits:

| Event | First group / next cycle | Second group / next cycle |
| --- | --- | --- |
| Clear schedule at205759dc | 271,674,205 | 271,834,124 |
| Adjust timer/reset append index at2098d51c | 271,685,657 | 271,845,369 |
| Run pending ARM callbacks at201422fc | 271,685,728 | 271,845,440 |
| Write50505000=0040 at205747f2 | 271,685,823 | 271,845,535 |
| Clear that group's schedule on next cycle | 271,834,124 | 271,994,124 |

The earlier `sim-op3-shared` write log places each final03ff mask store at
271,684,780 and271,844,497, before the0040 writes. Thus the schedule remains
stored across each periodic control write and is cleared on the next cycle;
the clear is an original ARM store, not a DSP acknowledgement. This ordering
does **not** prove0040 is an RF start command or identify which frame/time
origin the DSP uses. In the existing first-control breakpoint the raw timer
is1495 (integer373), while the first schedule time is37; executing all records
immediately at0040 or comparing them to the current ARM counter would require
an additional, unestablished timing assumption.

Static205746d8 calls2098d51c and201422fc before its0040 store. The former
selects timing bounds, stores period/match1 at5050100c/14, clears the append
index3049191d, and returns. The latter dispatches pending original ARM
callbacks, not DSP measurement code. Full2098d5c8 confirms that each append
reads the current32-bit mask, computes(mask<<1)+1, checks the count<=16,
stores its low16 bits and explicitly stores zero to the high halfword. There
is no RF result store in these paths. Do not turn an append-index reset into
schedule consumption or result publication.

ARM veneer204bf004 targets2118d408, a genuine serial submission wrapper.
It allocates one of four descriptors at304917ec+0x40*i and installs callback
215a7cb9; that callback calls the submitted descriptor's original callback,
then frees the entry. This is not itself an RF power calculator. The wrapper
is reached from RF preparation205759d4 during initial setup, but not during
the two failing operation3 groups.

`sim-rf-serial-callers` watches2118d408 from boot to fatal, with a200-hit
budget. All119 calls fit in the budget; the last is at267,530,411. In
particular the205759d4 call occurs at229,116,464 withR0=3001a508. No later
call reaches either205759b8 or205759d4 during the271.676M/271.836M operation3
submissions. Static205756c8 scans four mode bytes at descriptor+2..5; when
all are zero it takes205757e8 to205759c0 and normally skips submission if
3049191c is already nonzero. At fatal that byte is1. This explains why the
real early serial path must not be mistaken for the active op3 transport.
The preliminary `sim-rf-serial-submit-lifetime` probe exhausts its80-hit
budget at246,883,180 and cannot support any absence claim after that point;
use the complete119-hit caller probe instead.

The control-order and complete caller probes take32.285s and34.107s. Each
complete BootReport exactly equals `sim-result-w0c-final`:272,124,313
instructions,53,837,316 fast-forwarded,PC200141c0,R1EA020101,LR2098ddf3,
no CPU/memory fault,1 LCD equivalent,1 HKBD init,3 standby callbacks and
1,576IRQs. Final result5050501c remains zero. No new LCD/full RAM/NAND/DSP
capture or large dependency was created; free disk space remains about5GiB.

Further GitHub searches for RD160/Adelante instruction, assembler, DSP-C,
VLIW and SDK information found no relevant public implementation. Broad
code-search matches include unrelated text and are not hardware evidence.
The next prerequisite is an actual RD16024 instruction/peripheral reference,
compatible SDK/simulator, or a reference device trace establishing the RF
operation/result contract. The public architecture brief and the ARM scale
verification are insufficient to synthesize a calibrated no-signal result.

### Checkpoint — 2026-10-31: ARM RF result scale verified, DSP output still missing

SIM is **not fixed**. No board behavior, firmware, GUI binary, result flag or
IRQ is changed. Following the other callers of2098dd98 establishes more of
the ARM-side measurement arithmetic, but not the missing DSP producer.

The consumer2098de1a..26 loads the result as a signed halfword, divides by4
with truncation toward zero, adds the descriptor correction loaded with LDRH,
and stores the low16 bits at descriptor+8. Other consumers then pass that
corrected value through208af8e8 to215b4ab8, a numeric clamp to[-220,100].
They include2099f3fe..402 and2099f596..59a. The clamp is not a no-signal
sentinel operation; it turns an ARM unavailable marker510 into100.

Static20998b40..7a reads a signed corrected measurement from an ARM buffer,
divides by2 with truncation toward zero, and maps values at or below-110 to
RXLEV0, at or above-47 toRXLEV63, otherwise adds110. These are the GSM
RXLEV boundaries and establish the corrected representation as half-dB.
An ARM buffer halfword510 is handled as unavailable255;511 also produces255
after division. This is **not evidence that the DSP emits510/511**, or that
unavailable means a measured empty channel. Nor is this reporting path
observed executing before the current SIM fatal. Its arithmetic is established
by the unchanged original instructions, not a successful boot.

With the observed correctionff74 (-140), a hypothetical raw DSP result0
would produce-140 half-dB (about-70dBm), RXLEV40. Thus zero cannot be chosen
as a no-network measurement. Raw-320 would produce-220 half-dB andRXLEV0,
but that inverse arithmetic alone does **not** establish-320 as an actual
empty-channel DSP output. The raw contribution is divided by4 before entering
the half-dB representation; its calibrated origin and DSP-side algorithm
remain unknown. RF gain calibration and a low RXLEV also do not establish
absence of a cell or successful/no-network completion.

New tool `tools/verify_rf_result_math.py` fingerprints three tiny instruction
slices in the user's local E830XEGG1 CLA and executes only those unchanged
arithmetic slices in an isolated Python Unicorn instance. It does not run the
phone, load a RAM snapshot, create RTOS state or inject any hardware result.
No retail instruction bytes are included in the source tool. It verifies
**196,653 cases**: all65,536 signed inputs for the observed correction,
45 additional correction/wrap cases, and all65,536 halfwords for each of
the clamp and RXLEV encoder. Every comparison passes. Seven new synthetic
tests pass without firmware/Unicorn; eight schedule tests and all117 native
Rust tests also pass. Python Unicorn2.1.4 was installed only in the local
`.tools/analysis` directory for this verification, not added to the emulator's
runtime dependencies.

The new bounded native probe `sim-rf-result-format` takes32.564s. It watches
2099a9bc,2098dd98 and the three downstream conversion functions after271.650M
with a30-hit budget. Exactly two entries are reached:2099a9bc at271,995,776
hasR0=1, and2098dd98 at271,995,790 has descriptor304a8f44. None of the three
downstream functions is reached before fatal. Descriptor correction/index
remainff74/7; descriptor+8, indexed collected/shared result and5050501c are
zero. Aligned3049198c=07ff0000 still has a **zero low result halfword**.
The complete BootReport exactly equals `sim-result-w0c-final`:272,124,313
instructions,53,837,316 fast-forwarded,PC200141c0,R1EA020101,LR2098ddf3,
no CPU/memory fault,1 LCD equivalent,1 HKBD init,3 standby callbacks,1,576IRQs.
The probe does not render a new LCD image or capture RAM/NAND.

```powershell
python .\tools\verify_rf_result_math.py .\firmware\E830XEGG1.cla
python -m unittest discover -s tools -p test_verify_rf_result_math.py -v
.\build-windows.ps1 -Test
```

The blocker is narrowed, not removed: establish class05/00 andclass02/0a
DSP execution/timing, calibrated measurement behavior with no RF input, and
the result publication lifecycle. Do not use the now-known ARM scale to
invent a power value or force the indexed completion bit. No GUI deployment,
full dump/firmware copy or GitHub push was performed.

### Checkpoint — 2026-10-31: decoded active SIM/RF schedule, still no DSP result

SIM is **not fixed**. Board behavior, firmware, GUI binaries and result flags
are unchanged in this checkpoint. Two new bounded native probes establish the
active operation3 schedule and distinguish stored requests from live ones.

`sim-op3-gain-timing` watches20575fe8,205760b8,205760f6 and2098d5c8 after
271.650M with a70-hit budget. Exactly50 hits are recorded: ten gain calls,
ten calibration-code stores, ten correction-halfword stores and twenty
schedule submissions. Calibration output is0335 on every request; the ARM
correction output isff74 (-140). This is an ARM-side correction, **not** an
observed DSP measurement or an established no-signal sentinel. Timing lower
bounds passed to2098d5c8 are3,545,1455,2365,3275 in each group. Static
20143f8c..20143fa6 selects an operation timing halfword and adds the computed
schedule adjustment; the units of those schedule values are not inferred.
The probe takes33.240s and its entire BootReport exactly equals
`sim-result-w0c-final`:272,124,313 instructions,53,837,316 fast-forwarded,
PC200141c0, R1EA020101, LR2098ddf3, no CPU/memory fault,1 LCD equivalent,
1 HKBD init,3 standby dispatches and1,576 IRQs.

`sim-op3-scheduled-frame` stops before205747f2 at271,685,822 (32.162s),
with mask60000658=03ff and the following ten active three-halfword records.
The header fields follow the original ARM2098d5c8: slot in bits15..13,
timing-table byte7 in bits12..8 and byte6 in bits7..0. The labels `class`
and `opcode` below are raw fields, **not decoded DSP ISA instructions**.

| Slot | Packer channel | First header / raw time | Second header / raw time |
| --- | --- | --- | --- |
| 7 | 0 | e500 /37 | e20a /536 |
| 6 | 1 | c500 /947 | c20a /1446 |
| 5 | 2 | a500 /1857 | a20a /2356 |
| 4 | 3 | 8500 /2767 | 820a /3266 |
| 3 | 4 | 6500 /3677 | 620a /4176 |

Each third halfword is zero. Thus each slot has class05/opcode00 and
class02/opcode0a, with499 raw units between them and910 between successive
first records. Every command matches the exact two-word2098d854 packer,
with calibration code0335 and its band tag clear. A clear tag alone does not
identify the band. Timer raw50501000=1495, integer counter373, period5000,
match1=4700. Do not equate these counter/schedule values without establishing
the DSP-side timing conversion. Result5050501c remains zero. Control50505000
already equals0040 before the repeated write. The probe has1,572 IRQs; it
does not run through the fatal reporter.

At the fatal breakpoint in `sim-op3-gain-timing`, the schedule mask is zero
although the same ten record values remain in shared RAM. Original
205759dc/e0 clears the mask. The second group's commands remain in slots7..3
and encode channels5..9. Retained request bytes after mask clearing are
therefore **not** evidence that another frame should execute them.

New read-only tool `tools/inspect_dsp_schedule.py` consumes ordinary CLI
`--peek` lines, decodes only active records, and recognizes only the exact
two-word packer shape. It rejects missing/alignment/conflicting data and
mask bits above the original producer's16-record limit; it does not infer
RF power, time units, completion flags or success. Eight synthetic tests in
`tools/test_inspect_dsp_schedule.py` pass, including the unaligned 6-byte
record boundary, stale records and the channel/upper-halfword distinction.
All117 existing native Rust tests also pass (107 core,1 CLI,9 GUI/runner).

```powershell
python .\tools\inspect_dsp_schedule.py `
  .\proof\native-windows\sim-op3-scheduled-frame.log
python .\tools\inspect_dsp_schedule.py `
  .\proof\native-windows\sim-op3-gain-timing.log
python -m unittest discover -s tools -p test_inspect_dsp_schedule.py -v
```

An exact-name public web search found Philips' October2005 **Adelante
RD16024 DSP core** brief, order9397 750 14171, at
<https://application-notes.digchip.com/004/4-8173.pdf> (local small public
reference `proof/native-windows/rd16024-public-brief.pdf`). It establishes
a Dual Harvard/VLIW core, compact16/32-bit instructions,96-bit VLIW escape,
256 programmable application-specific instructions and a separate SDK
assembler/simulator. It contains **no opcode encoding or RF measurement
algorithm**. ARM disassembly is not valid for the uploaded DSP program;
this general brief does not establish the E830's ASI table or memory-space
assignment. GitHub code searches did not yield the required ISA/SDK or
matching DSP-side measurement implementation. One search result was this
repository's own investigation; it is not independent corroboration.

No full RAM/NAND capture, firmware copy, GUI replacement, network subscription
or GitHub push was performed. The next blocker is still the DSP-side
class05/00 and class02/0a execution/result lifecycle: timing, payload units,
valid no-signal output and generation of the indexed result. A hardcoded
flag or arbitrary measurement would bypass the firmware check, not solve it.

### Checkpoint — 2026-10-31: actual DSP MMIO path during SIM boot

SIM still stops at `EA020101`; no result flag/payload is fabricated. A new
passive `E830_TRACE_DSP_IO=1` trace records the values already supplied to the
actual MMIO callbacks, not additional register reads. Its independent
`E830_TRACE_DSP_IO_AFTER` instruction threshold includes fast-forwarded slots;
`E830_TRACE_DSP_IO_LIMIT` defaults to1000. It covers serial50301000..50301203,
signals50505000..5050501f and boot interface60100000..6010000f. It neither
consumes another FIFO word nor generates a completion. The RAM `MEM_WRITE`
hook does not observe accesses handled by Unicorn `mmio_map`; therefore an
empty RAM trace targeting hardware registers does **not** establish absence
of DSP transport.

`sim-op3-serial-path` watches20143184,205769f8,2098d854 and204d0388 after
271.650M. Its 100-hit budget retains84: three IRQ21 drains, three result
collectors and ten command packers, but no serial queue start204d0388 in this
bounded interval. `sim-op3-dsp-io` then records the real hardware accesses:

- Collector205769f8 reads zero at5050501c at271,664,590,271,824,590 and
  271,984,590.
- Original205747f2 writes halfword0040 to50505000 at271,685,823 and
  271,845,535, after each group of five operation3 submissions.
- There are no serial50301000..50301203 or boot60100000..6010000f accesses
  in that interval before the fatal breakpoint. Only5 guest DSP MMIO accesses
  consume the1500-entry budget; the3 subsequent register peeks are host reads
  logged at the stopped PC, not firmware activity.

`sim-op3-dsp-io-wide` starts at270M with a3000-entry budget. It records26 guest
accesses before the fatal breakpoint:13 zero result reads,12 periodic0040
writes at205747f2 and one0002 write to50505004 at215a9654, instruction
270,236,413. No serial/boot-interface accesses occur in this wider window.
The0040 write also occurs in cycles without operation3, so its exact signal
semantics and relationship to DSP execution remain unproven. Function205746d8
performs timer bookkeeping then ends with this write. Function215a9654 writes
+4 after filling shared60000040..60000046; this is a separate path. Neither
observation authorizes treating0040 as an RF-result-complete event.

`sim-op3-first-control` stops **before**205747f2 at271,685,822. Shared schedule
mask60000658 is03ff; its first two three-halfword records are
`e500 0025 0000` and `e20a 0218 0000`. Slot7 remains
`0800 0001 0000 066b`. Timer raw50501000 is05d7, period5000 and match1=4700;
ARM low result flags3049198c are zero (the aligned32-bit peek07ff0000 includes
the unrelated next halfword). The preceding50505000 value is already0040:
this is a repeated write, not an observed zero-to-one edge.

Static2098d854 decoding corrects another potential misinterpretation. A packed
first word is `(band_tag | ((channel & 03ff) << 6) | 21) << 11`, with
band_tag10000 only for band08/10. Thus upper halfwords1,3,5,...,19 encode
channels **0..9**, not channels1,3,5,...,19. Calibration input R2 is0335 on
all ten watched calls. The packer applies a band04/08 adjustment before
packing its second word; do not generalize a band-independent formula. The
first-control snapshot's final input record304a86d0 contains channel4,
band02 and signed inputff40; it does not establish every request's band.

The stream pointed to by3001a684 includes copies of shared writes **and**
ordinary diagnostics. Its c000 records alone do not prove serial transmission;
the earlier checkpoint's term "transmit buffer" was stronger than the runtime
evidence. The result collector's complement ACK is actually stored at
**20576a2a** (20576a24 is its preceding comparison).

`sim-op3-serial-path`, `sim-op3-dsp-io` and `sim-op3-dsp-io-wide` exactly match
the complete `sim-result-w0c-final` BootReport, including272,124,313 instructions
and1,576 IRQs. The DSP-I/O LCD retains splash SHA256
`9140970baf8c6ef39893e9ac6469a27b76ce64c9cfaaace084ed10e41638f4f5`.
`dsp-io-absent-600m` takes69.321s and exactly matches the complete preceding
`sim-result-w0c-absent-600m` BootReport. Insert SIM/SOS remains unchanged:
PPM SHA256 `a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07`.

Reproduce the focused hardware trace without capturing RAM/NAND:

```powershell
python .\tools\run-probe.py --env E830_SIM_PRESENT=1 --env E830_DSP_BOOT_HLE=1 `
  --env E830_TRACE_DSP_IO=1 --env E830_TRACE_DSP_IO_AFTER=271650000 `
  --env E830_TRACE_DSP_IO_LIMIT=1500 `
  --env E830_WATCH_PC=0x204d0388,0x205769f8,0x20143184 `
  --env E830_WATCH_AFTER=271650000 --env E830_WATCH_LIMIT=100 `
  sim-op3-dsp-io -- --max-instructions 273000000 `
  --break-at 0x200141c0 --break-after 271000000 `
  --onenand-image proof/native-windows/direct-call-side/tfs-v4/full-tfs-2k.onenand.bin `
  --peek 0x5050501c --peek 0x50505000 --peek 0x50505004
```

GitHub code searches for RD16024/PNX5230/register50505000 did not locate a
relevant DSP implementation. TAT_RF_RX_POWER_MEASURE searches found SIM900
symbol/type names, not definitions establishing E830 DSP result semantics.
No third-party firmware, large dumps or new dependencies were downloaded.
Native tests pass **117** (107 core,1 CLI,9 GUI/runner), including a bounded
passive-trace state test and an ARM execution test that compares complete
BootReports with tracing enabled/disabled and proves one FIFO read consumes
exactly one word. Release CLI/GUI builds also succeed. Diagnostic changes do
not alter hardware responses; no candidate was copied to dist or pushed.
The previously noted keypad GUI PID13356 is no longer running; no user
process was stopped and both dist launchers/binaries remain untouched.
Next: establish the DSP shared-schedule execution and its real RF-result
representation, rather than parsing event-stream copies as a serial protocol.

### Checkpoint — 2026-10-31: SIM follow-up, DSP operation 3 and result ACK

The user reports a GUI freeze after entering one digit and redirects work to
SIM. The earlier single-digit CLI capture does not establish sustained input
or explain that freeze. It remains unresolved; the running keypad GUI
(PID13356, `dist/e830-emulator-keypad.exe`) and both launchers are preserved.

New native traces narrow the SIM-enabled `EA020101` failure to a concrete
request stream. `sim-request-descriptor` shows original interpreter
`2099a9e4` copying record `304a572c` into `304a8f44`: halfwords 1 and
`ff74`, then result-index byte 7. The consumer `2098dd98` is entered at
271,995,790. There is still no corresponding result flag/payload.

ARM veneer `208aeea8` targets the generic Thumb operation builder
**20143af8**. Function **2098dccc** can submit operation 7, but it is not hit
in the bounded SIM boot. Neither is operation-5 wrapper `2098de64`.
`sim-operation-origin` watches these entries from instruction zero to the
fatal breakpoint, with its 200-hit budget not exhausted: the builder is
called ten times, always with **R0=3**, through `2118e508` / `2118e580`.
Do not confuse the consumer's result index 7 with the operation code.

The two observed groups of five operation-3 submissions start at
271,676,222 and 271,835,939. Each group uses buffer slots **7,6,5,4,3**.
`sim-op3-shared` traces all shared-memory writes from 271.670M, with a
700-write budget not exhausted, and establishes:

- Builder `20143dea` writes **20 halfwords** per slot to
  `600006bc + 0x28 * slot`. Slot7 is therefore **600007d4**, not 600006bc.
  Its first group begins `0800 0001 0000 066b`; the second begins
  `0800 000b 0000 066b`. The other 16 halfwords of that slot are zero.
- `20143e06..20143e18` also serializes the shared-memory block write into
  the original ARM event stream, whose current pointer is stored at
  `3001a684`: header `c000 | (035e + 20 * slot)`, count20, then payload.
  This copy does not by itself prove DSP transport or measurement completion.
- Operation3 selects original table callback `20d94ddd` at
  `20c071a8 + 12*3`. Its callees `20575fe8` and `2098d854` use input-band,
  channel and calibration fields to pack the first command words. Exact DSP
  execution semantics, result units and a valid no-network result are not yet
  established from these ARM-side packers.
- `2098d5c8` additionally fills ten three-halfword scheduled records per
  group at `6000065c`, with their bit mask at `60000658/65a`. The mask grows
  to `03ff`, then the original `205759dc/e0` clears it on the next cycle.
  Storage of these records does not execute them or produce result flags.

The first broad shared-memory trace, `sim-shared-requests`, exhausted its
1,000-write budget on earlier periodic work. Its write log must not be used
to claim absence of later requests. Likewise, the narrower initial
600006bc..60000733 trace does not cover the actual operation3 slots.

One independently established board bug is corrected: DSP result signal
**5050501c** now has the same **write-zero-to-clear** acknowledgement shape
as startup/runtime signals. Original collector `20576a2a` writes the
complement of sampled result flags. Ordinary ARM writes no longer create
flags; byte/halfword writes preserve unwritten bytes. No result value, task
state, callback or IRQ is synthesized. Two unit tests exercise partial ACK,
independent startup/runtime state, no creation of flags or pending IRQs, and
no consumption of the runtime-ready second phase by a result ACK.

Native tests pass **115** (105 core,1 CLI,9 GUI/runner); release builds succeed.
`sim-result-w0c-final` takes 37.939s and exactly matches the pre-change
descriptor probe's complete BootReport: fatal entry `200141c0`, R1=`EA020101`,
LR=`2098ddf3`, 272,124,313 counted instructions, no CPU/memory fault,1 LCD
equivalent,1 HKBD init,3 standby dispatches,1,576 IRQs. This breakpoint is
before the reporter instruction executes; older notes count the entry as
272,124,314. Result flags5050501c, low halfword3049198c and result600000ea
remain zero. Its LCD is still the original Samsung splash, not a working
SIM screen (PPM SHA256 `9140970baf8c6ef39893e9ac6469a27b76ce64c9cfaaace084ed10e41638f4f5`).

`sim-result-w0c-absent-600m` takes 71.555s. Its complete BootReport equals
`perf-final-600m`:26 LCD equivalents,48 standby dispatches,11,792 IRQs and
clock1308. The final Insert SIM/SOS PPM retains SHA256
`a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07`.
Only clock is a shared peek between these runs; their other peeks differ.
Input NAND SHA256 remains
`d305c8e0b27e86ecec70534be1ee2c917c1556e675d5868573e600b7cf341a56`.
No full RAM/NAND captures, firmware copies, GUI replacement or GitHub push
were performed. The verified correction alone does **not** solve SIM boot.
Next: decode operation3 and its actual DSP execution/result delivery path,
including scheduling/transport, before modeling a supported completion.

### Checkpoint — 2026-10-31: genuine phone contacts and GUI keypad

Runtime driver initialization `2151bd60` selects device 6, base **50308000**,
IRQ **16** (`2151be0a` and the initialized per-line handler table). ISR
**208e51ac** reads control byte +0, coordinate halfword +4 and status byte +8.
Status is active-low: bit0 press, bit1 release, bit2 error; +8 is acknowledged
with ones (`7` in the original ISR). +4 packs row in bits0..4 and column in
bits5..9; the ISR creates its own event buffer and the original callback
**2118bf80** forwards it to HKBD. No guest message, callback, task or RAM is
seeded. IRQ16 is device-driven and honors controller and INTC/CPU masks.
Early startup GPIO scan `208daa1c` remains all-released; it is not the runtime
input path. Its original GPIO configuration storage is unchanged.

Coordinates are recovered from the original initialized 8-byte table at
**30031994** in existing `timer-fraction-late-600m.ram.bin`, not guessed host
keycodes. Digits 1..9 use (row,column): (1,8), (2,16), (2,8), (1,4), (1,16),
(2,4), (1,2), (4,2), (2,2); 0/*/#: (2,1), (1,1), (4,1). SoftLeft/SoftRight:
(8,4)/(8,8); Call/End: (8,2)/(16,2); Up/Down: (8,1)/(16,24);
Left/Right/OK: (20,17)/(20,18)/(20,20). Runtime HKBD identifies tested
SoftLeft as 0x0e, OK as 0x18 and Down as 0x15. Other semantic assignments
follow that table and await individual visual navigation checks.

`keypad-first-640m` uses valid installed NAND, DSP boot HLE and **SIM absent**.
External digit1 down at 600M, up at610M: ISR enters at600,000,432 and
610,000,396; callback2118bf80 at600,000,523 and610,000,484. HKBD sender215a614c
generates messages2203/2204 with group3/key1 at600,006,592/600,006,939.
Final640M report has no CPU/memory fault,28 LCD equivalents and52 standby
dispatches. LCD visually shows a red **1** on the original dialer, softkeys
**Call / Exit**. PPM SHA256:
`2dd9c7e8d2e8872adca7edf384e46712ef84077bd8fa43babb8c1b511a2384c0`.

`keypad-ok-680m` and `keypad-softleft-680m` press their respective key at600M,
release603M, then Down at640M/release643M. Both reach genuine HKBD handlers,
finish680M without CPU/memory fault and retain **Insert SIM / SOS**, not an
application list (27 LCD equivalents,53 standby dispatches). Their equal
final PPM SHA256 is
`14e6875a739206fc52cc47690e58299a585b212a3321befceb65bdc60f79130b`.
Application-menu navigation is therefore still unverified; this input change
does not resolve the SIM/DSP result blocker below.

Reproduce the digit check (no RAM or NAND capture required):

```powershell
python .\tools\run-probe.py --env E830_DSP_BOOT_HLE=1 `
  --env E830_WATCH_PC=0x208e51ac,0x2118bf80,0x215a614c `
  --env E830_WATCH_AFTER=600000000 --env E830_WATCH_LIMIT=30 keypad-first-640m `
  -- --onenand-image proof/native-windows/direct-call-side/tfs-v4/full-tfs-2k.onenand.bin `
  --max-instructions 640000000 --key-at 600000000:1:down --key-at 610000000:1:up `
  --dump-lcd-frame proof/native-windows/keypad-first-640m.ppm
```

GUI contacts travel through a worker-owned input channel. Short clicks retain
both transitions; duplicate held states do not repeat. Contacts release on
focus loss, leaving Run output or stop; the CPU stays in its owning thread.
The controller's host event queue is bounded64; overflow exposes error status, not guest
RAM writes. Full physical scan/debounce timings and hardware autorepeat are
unverified. GUI framebuffer revision now includes partial pixel updates while
keeping BootReport's full-screen-equivalent counter unchanged.
Mouse buttons and PC shortcuts: arrows/Enter, F1/F2 softkeys, F3/F4 Call/End,
F5/F6 */#, digits0..9. Existing LCD scaling, fullscreen and pickers remain.
The old running GUI PID8296 and its executable are preserved; a separately
named keypad build uses existing selected firmware/NAND without duplicating
them. The old SIM-enabled launcher is unchanged.
Final native `build-windows.ps1 -Test`: **113 passed** (103 core,1 CLI,9 GUI/runner).
Release build also succeeds. Separate GUI is7,479,992 bytes, SHA256
`27d97159289788b88331218991ef31e518d7528a57ee19c809168b065ba914b4`.
The new interactive GUI has not been launched on the user's behalf; runtime
evidence above is from CLI hardware contact probes and GUI input unit tests.

### Checkpoint — 2026-10-31: SIM APDU/RX development and next fatal check

The local development controller gathers the five automatic header bytes at
`+0x808..+0x818` and the TX DMA payload. Observed commands are answered with a
small, read-only fictional GSM test-card profile: MF `3f00`, GSM DF `7f20`,
TELECOM DF `7f10`, language files, ICCID, IMSI, Kc placeholder, PLMN/HPLMN,
SST, ACC, FPLMN, LOCI, AD and phase. Unknown FIDs return `9404`, unsupported
instructions `6d00`, invalid binary offsets `9402`. This is not a subscription,
authentication implementation or radio-network model. No real identity or key
is used. Directory metadata follows the TS 51.011 field offsets; all supplied
EFs are transparent and updates are forbidden. STATUS returns the current
directory even when an EF is selected.

`sim-directory-format-260m` establishes successful GET RESPONSE through the
original callback `201a30ec` at instruction 235,819,650. Hardware DMA channel 5
has source `50406034`, RAM destination `31f48ce4`, control `88000016`, config
`b013` and link zero. The model writes exactly the requested data bytes, updates
the DMA destination/count/enable and raises terminal count/IRQ29. The original
callback, not the host, appends SW1/SW2 from `+0x820/+0x824`. Response count is
at `+0x81c`. Card power, automatic RX controls and exact supported descriptors
are required. Replies are delayed and consumed once. APDU latency is currently
256 instruction slots and DMA latency 32 slots/byte, not measured ETU timing.

An intermediate implementation also raised SIM IRQ bit 11. This dispatched an
already-unlinked descriptor for a second time, accessed a NULL callback and
ended with invalid instruction at `2112311e` (`sim-select-response-242m`). That
extra IRQ was removed. Do not reintroduce it as unconditional DMA completion.
PIO TX, complete RX error/timeout signaling, record files, authentication and
full timer/reset/flush modes remain unimplemented. In particular, action 2 in
timer register `+0x20` is also used for software measurement; classifying every
such action as a physical card reset is not yet established.

The later reinitialization after the first directory response is not evidence
of rejection: firmware genuinely changes PMIC voltage profile through HECI
`5b20`, then power-cycles the card around 238.510M and accepts another ATR.
`sim-apdu-candidate-600m` (directory-only profile) executes 600M instructions
without a CPU/memory fault but still shows Samsung Fun Club, not standby.

The richer `sim-files-600m` probe proves genuine DMA reads of language, IMSI
(269,156,316), ICCID (269,426,812), ACC, LOCI, Kc, FPLMN and HPLMN
(270,428,834). It subsequently enters the firmware fatal reporter, ending at
PC `20014b4c`, despite the CLI reporting no CPU fault. A successful CLI exit
does not prove boot. `sim-files-first-failure` identifies the first fatal
report as `EA020101` at instruction 272,124,314, LR `2098ddf3`. Function
`2098dd98` tests a bit indexed by descriptor `304a8f44+4` against the halfword
at `3049198c`; the required bit is absent. The producer/consumer chain is the
next blocker, possibly in DSP/radio interaction, not yet proven to be caused
by the rejected optional `6f11`/`6f13` files. No guest state, callback, firmware
instruction or IRQ is forced to bypass this check.

Focused follow-ups `sim-dsp-flag-origin` and `sim-radio-result-missing` establish
the hardware source of this predicate. Descriptor byte `304a8f48` is **7**;
bit 7 of the result mask is required. Original function `205769f4` reads the
low byte of DSP result flags at **`5050501c`**, ORs it into `3049198c`
(`20576a22`), then copies eight signed result halfwords from **`600000dc`** to
`304919a0`. This is distinct from startup signal `50505010` and the limited
runtime-ready handshake `50505018`. In the focused trace all sampled result
flags are zero from 269.104M through 271.984M; the consumer fails at
271,995,808. No intervening write supplies bit 7. The corresponding result
halfword at `600000ea` belongs to index 7 (the aligned peek at `600000e8` is
zero). The DSP shared window currently provides storage only, not execution
or generation of these measurement results. The next work is to trace the
original request stream that requires this result and implement an established
DSP operation/result lifecycle. Merely setting bit 7 or raising a new IRQ
would bypass the check, not model the missing operation. No such change was
made. The result trace and final peeks do not by themselves establish a valid
radio measurement value or a complete DSP protocol.

Reproduce the focused read-only check with the local CLI build:

```powershell
python .\tools\run-probe.py `
  --env 'E830_SIM_PRESENT=1' --env 'E830_DSP_BOOT_HLE=1' `
  --env 'E830_TRACE_RAM_START=0x3049198c' `
  --env 'E830_TRACE_RAM_END=0x3049198d' `
  --env 'E830_TRACE_RAM_AFTER=269000000' --env 'E830_TRACE_RAM_LIMIT=50' `
  --env 'E830_WATCH_PC=0x205769fe,0x20576a22,0x2099a9e4,0x2098ddbc' `
  --env 'E830_WATCH_AFTER=269000000' --env 'E830_WATCH_LIMIT=50' `
  sim-radio-result-missing -- --max-instructions 273000000 `
  --break-at 0x200141c0 --break-after 271000000 `
  --onenand-image .\proof\native-windows\direct-call-side\tfs-v4\full-tfs-2k.onenand.bin `
  --peek 0x3049198c --peek 0x5050501c --peek 0x600000e8 --peek 0x304a572c
```

Native tests pass **98 core + 7 GUI/runner = 105** and release CLI/GUI builds
succeed. `sim-files-absent-regression-600m` takes 101.077 s and exactly matches
the preceding no-SIM release BootReport (only the LCD filename differs). Its
LCD PPM SHA-256 remains
`a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07`.
The old/new probes inspect different extra RAM addresses, so those peeks must
not be described as identical. Input NAND remains local and unchanged. No
candidate binaries are copied to `dist`, no running user GUI is replaced and
no public repository is pushed during this investigation.

### Checkpoint — 2026-10-31: opt-in SIM power, ATR and first TX request

`E830_SIM_PRESENT=1` now enables an experimental test-card/controller path.
It is off by default and is **not yet a usable SIM or a menu fix**. The exact
value `1` is required. No firmware, guest queue, task state or RAM callback
is patched; the original driver owns all RTOS messages. The existing `dist`
launcher, GUI layout, NAND picker and public mirror are unchanged.

Original driver evidence establishes the following chain:

- PMIC I2C slave `0x09`, presence register `0x2d` bit 1. Presence is an
  external input, not writable. Register `0x2c` bit 4 starts card power.
  In opt-in mode status bytes `1..4` are read-and-clear; masks `0x12..0x15`
  use **1=masked**. `HECI_ENABLE_REQ` (`0x5b10`, `0x2013df04`) clears a mask;
  `HECI_DISABLE_REQ` (`0x5b18`, `0x215a442c`) sets it, not the reverse.
- Power-sequence completion sets PMIC event **21** once per enable edge.
  IRQ_N is active-low GPIO **pin 3**, group 0 in the observed initialization.
  GPIO input `0x50303050`, latched status `+0x4c` (W0C), routing masks
  `+0x40/+0x44/+0x48` preserve other pins, including phone-open pin 9.
  No forced CPU interrupt or host SMCP message is supplied.
- GPIO ISR `0x200a80e4` invokes `0x2013dcdc`, waking HECI event 0. Handler
  `0x20d8fd54` reads PMIC events; dispatcher `0x2013ddc0` invokes activation
  callback `0x201a3180`. SMCP `0x211fa6d0` cancels its timeout and calls the
  original SIM-reset path `0x2013c5d4`.
- SIM is device **0x0c**, base **`0x50406000`**, **IRQ30**, ISR `0x200ea654`.
  `0x50407000`/IRQ27 is a different device. SIM IRQ raw/enable/W1C-ACK are
  `+0xfe0/+0xfe4/+0xfe8`; core status `+0` is also W1C. Reset action 2 in
  `+0x20` schedules reset-done (IRQ bit 5/core bit `0x100`). The ISR releases
  the receiver through `+0x0c` bit 7 before ATR delivery.
- FIFO control/status is `+0x30`, **not the byte port**. Low three bits select
  RX threshold minus one; bit 6 means RX empty. `+0x34` latches the next byte
  until firmware acknowledges it at `+0x40` bit 0. Interrupt ACK alone does
  not discard bytes. Diagnostic reads remain passive.

`sim-atr-first-242m` proves genuine ATR receipt: reset IRQ at instruction
235,685,239; original reads of `3b` and `00` at 235,689,682 and 235,689,759;
ATR callback `0x21186eac` at 235,689,818; accepted minimal T=0 path
`0x21186f26` at 235,689,847. ATR `3B 00` advertises direct convention,
default T=0, no history. No callback is invoked by the host.

The next original command is **`A0 A4 00 00 02 7F 20`**, SELECT GSM DF.
The controller supplies its five header bytes via `+0x808..+0x818`; DMA
provides `7F 20` from guest RAM `0x31f48dfc`. `sim-dma-start` stops naturally
at programmer return `0x214f994e`, instruction 235,778,167: hardware channel
5 (`0x501001a0`), source `0x31f48dfc`, destination `0x50406034`, link zero,
control `0x84000002`, config `0xaa41`, global enable one. This is request 9,
not UART0 request 6. The local DMA model accepts only this RAM-to-SIM byte
mode with card power and the observed automatic-header/TX controls enabled.
It transfers real RAM bytes and raises only DMA terminal count/IRQ29.

An intermediate probe (`sim-tx-first-242m`) incorrectly treated SIM
`+0x81c` and IRQ bit 11 as mere TX completion. It reached a firmware callback
without a card response and ended in `INSN_INVALID` at 235,782,673. That
behavior was **removed**, not retained as progress. `+0x81c` is checked by
the automatic exchange path (`0x2154bebc`); status getters read `+0x820` and
`+0x824`. DMA completion alone must not supply these or declare APDU success.
The guarded implementation captures the command but deliberately has no
card response/completed exchange until a T=0 card engine is implemented.

Remaining work: T=0 procedure/status bytes and automatic-command timing,
SELECT/GET RESPONSE/READ BINARY/READ RECORD, a consistent fictional test-card
filesystem/identity, PIN status and later actual firmware requests. PIO TX,
SIM RX DMA, errors/timeouts, FIFO flush and full controller modes remain
unimplemented. Power/reset/ATR latency is provisionally 4096 instruction
slots; ATR bytes arrive together, not at measured ETU intervals. The complete
PMIC map is unverified: opt-in IRQ bytes are separate from historical RTC
storage; the PCF50603 short-form reference supports interrupt polarity/masking
but does not establish every E830 register address. A virtual card provides
no radio network, operator credentials or real subscription.

Tests cover PMIC presence/power/masks/read-clear, GPIO polarity/masks/W0C,
delayed SIM reset, latched ATR FIFO/threshold/ACK, receiver-disable and poweroff
cancellation, and architectural IRQ30 entry with guest registers/stack
preserved. DMA tests preserve RAM and CPU state and reject absent/unpowered,
RX, linked, wide, out-of-RAM and wrong-request descriptors. They assert **no
SIM exchange IRQ or response counter** from mere DMA TX.

The ATR release's no-SIM 600M regression (`sim-atr-absent-regression-600m`)
took 65.258 s and matches `perf-final-600m` in its entire BootReport and both
peeks, normalizing only the LCD filename. PPM SHA-256 remains
`a7ad2f679d350cb9e684fabb2975de8a26f0484b266459e5aea7b9403a160d07`.
Proof files and the supplied NAND/firmware remain local and excluded from
public source packages. The guarded TX release is checked separately below.

Final guarded TX evidence: `sim-tx-guarded-242m` transfers `7F 20` at
235,778,231 and reaches the 242M instruction budget without a memory fault
or CPU error (one LCD frame, one HKBD init, three standby dispatches, 862
IRQs). It supplies no card response; firmware disables/reconfigures the SIM
path around 238.466M, so this is **not a successful SELECT** or an indefinitely
waiting card state. No menu is reached. The final no-SIM release probe
`sim-tx-absent-regression-600m` takes 89.990 s and matches the entire baseline
BootReport, both peeks and PPM SHA-256 above after LCD-filename normalization.
Input NAND SHA-256 is rechecked and unchanged. Native tests pass **91 core
+ 7 GUI/runner = 98 tests**; release CLI/GUI builds succeed. At this checkpoint
only local `target/release` binaries contained the experimental SIM path;
the subsequent GUI distribution refresh is recorded below.

### Checkpoint — 2026-10-31: GUI distribution refresh

At the user's request the current optimized release GUI and CLI, including
the opt-in SIM groundwork, replace the old binaries in the existing local
`dist`. A fresh native release build and all **91 core + 7 GUI/runner tests**
pass. Each distributed executable matches its `target/release` SHA-256:

- GUI: `f9a2f493975381627ad6f6bd596d420e36744da4bc9573ca6fea3e729f727ac4`.
- CLI: `f03dd9fb2c11444ec858cf7da50a9cc1fa1d4021c7950a20f803d6a3859dd9d8`.

Inspection found that the current GUI configuration selected the erased
`dist/firmware/E830XEGG1.onenand.bin` (SHA-256 `f89b94be...`). To retain the
verified standby boot, only the selected NAND path is corrected to the
existing `proof/native-windows/direct-call-side/tfs-v4/full-tfs-2k.onenand.bin`.
Its SHA-256 is rechecked as
`d305c8e0b27e86ecec70534be1ee2c917c1556e675d5868573e600b7cf341a56`.
No firmware/NAND file is changed or copied, and other configuration choices
are preserved. This is a local package referencing an existing local image,
not a portable firmware distribution.

`Run-E830.cmd` retains launch-local `E830_DSP_BOOT_HLE=1` and now clears
inherited `E830_SIM_PRESENT` within `setlocal`: the incomplete SIM does not
silently replace the verified no-card flow. Direct executable launches still
retain the default DSP behavior and opt-in environment semantics.

A brief launch of the updated distributed GUI renders the original Samsung
splash, captured in `proof/native-windows/gui-refresh-20261031.png`. The
verification window and worker close normally afterward. This smoke test does
not repeat the full standby run or verify application-list navigation; the
previous final no-SIM 600M regression above remains the standby evidence.
The user identifies the original Insert SIM / SOS screen as the reached
"menu". Source and local distribution READMEs now distinguish that screen
from unverified application navigation and unimplemented phone buttons.
No source-mirror synchronization or GitHub push is performed for this refresh.

### Checkpoint — 2026-10-31: user-requested SIM launch opt-in

Following the GUI refresh, the user explicitly requests enabling the test
SIM. The local `dist/Run-E830.cmd` now sets `E830_SIM_PRESENT=1` within
`setlocal`, alongside the existing `E830_DSP_BOOT_HLE=1`. This applies to
the next launch through that script, not to system settings or an already
running process. Direct executable launches still require an explicit opt-in.
The source and local distribution READMEs describe this and the no-SIM
rollback (`set "E830_SIM_PRESENT="`). No binary, firmware, NAND or controller
behavior is changed; no additional successful card exchange is claimed.
Power/ATR and first TX are implemented, but APDU replies remain absent as
documented above. Only the launcher setting is checked for this change;
there is no new runtime SIM probe or GUI launch.

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

The next lead initially appeared to be an **empty UI callback queue**, not
unscheduled HKBD/standby. The object at `0x302d3b80..0x302d3daf` has 20 entries
of 0x1c bytes, and both the pre-scan RAM snapshot and the 300M snapshot are all
zero. Flags at `0x3036c10c` are
`0x20000000`, so bit `0x100` is clear and standby takes the scan path. The
focused scan trace skips all 20 entries through `0x20157a84`; none can call
a UI callback. This does **not yet prove** why registrations are absent or
whether this queue must be populated to draw the main menu.

Later cross-build naming corrected this interpretation: the matching D500
object is `a_StoredMsgInfoTable`, part of `lk5` saved-message handling, not the
active menu key queue. The measurements below remain valid observations of
that table and its users, but they are no longer evidence for the Up-key bug.

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

### GUI keypad release and crash diagnostics (2026-11-01)

The late menu input failure at `PC=0x210e09da` is fixed. It was not an IRQ or
keypad-coordinate failure: the generic splash/fade delay optimizer also matched
the genuine Thumb delay at `0x209a2d8c`, then resumed at the following even PC.
Unicorn derives execution state from bit 0 on a PC write, so this synthetic
skip cleared Thumb state. The firmware subsequently reached valid Thumb bytes
at `0x210e09da` with `CPSR.T=0` and stopped with `INSN_INVALID`. The optimizer
now resumes at the odd Thumb address and explicitly preserves `CPSR.T`.
`late-menu-thumb-fixed-v2` applies SoftLeft down/up and Down down/up, reaches
1,745,000,000 instructions with `InstructionBudget`, no memory fault and no CPU
error. A focused Unicorn regression executes a Thumb instruction after the
collapsed delay and verifies that Thumb state remains set.

The desktop keypad kept a mouse contact asserted for the click/release frame by
combining `Response::is_pointer_button_down_on()` with `clicked_by()`. The latter
is a completed click reported on release, not a still-held physical contact.
The GUI now derives mouse contacts only from `is_pointer_button_down_on()`, so
release is forwarded on the actual release frame instead of one repaint later.
Firmware and keypad coordinates are unchanged.

The follow-up GUI input correction also preserves a mouse press and release
that both arrive between two repaints. Such a completed click has no observed
held frame, so the GUI now queues one explicit down/up pair only when the key
was neither held previously nor held currently. A normal held click still uses
the physical down state and releases immediately; `clicked_by` is not treated
as another held frame. This fixes easily missed quick clicks on the small
direction buttons without changing firmware coordinates or guest timing.

The worker now reports every applied contact transition with the cumulative
instruction count, key mask and preceding PC. CPU and guest-memory failures also
include CPSR/SP/LR and the last eight applied transitions. This is passive
diagnostics: it does not suppress errors, patch firmware or alter guest state.
The existing immediate down/up CLI probes still reach the requested instruction
budget: digit `1` opens the genuine `Call log` screen; `Star` and `Down` leave
`Messages / Select / Back` unchanged without a CPU or memory fault. All 178 Rust
tests and 66 Python tests pass after the GUI correction.

Deferred user report (2026-10-31): pressing the star key in the GUI stops
the CPU with `INSN_INVALID`, PC `0x210e09da`. This is not reproduced or
diagnosed here. At the user's request, continue RF/SIM HLE first; do not
change keypad handling or suppress the CPU error as part of that work.

1. Continue the opt-in analog/RF HLE checkpoints. Address3b serial raw1000
   passes70012904 through the original calibrated conversion. Timer-frame
   alignment now passes the275M late-result boundary through original IRQ21;
   the collector reads all five flagsf8. The0333/ff75 gain profile now passes
   the277,408,137 boundary and further channel518..522 requests. The traced
   next-frame pipeline and history-validated scratch residue now pass the
   277.884M,280.284M,288.444M,302.048M,304.764M and321.244M fatal boundaries.
   Same033f restarts, ff5d (-163) ARM correction and the captured885/033f
   to0..4/033f tail/range history pass329.4M and345.9M. Disabling loop
   collapse while RF work is pending fixes the proven356M temporal jump.
   Tagged0333 gain/correction, original packer inputs and the exact
   clear885/033f-to-tagged512..516/0333 transition now pass394.5M.
   The exact four-request807..810/0333 history and tagged810/0333-to-
   tagged512..516/033d transition now pass410,844,338. Exact033d four-request
   tail/history and tagged810/033d-to512..516/033f transition pass427,324,372.
   The exact033f tail438,569,060 and tagged810/033f-to512..516/033f wrap
   now pass the443.8M fatal; all264active wrap bytes match independent peeks.
   Long boot completes900M without a memory fault,60LCD equivalents. Local
   OCR reads Menu/Contacts; visual SIM-present standby and broader
   application-list navigation still need verification. SoftLeft after900M
   produces the genuine firmware screen OCR `Messages / Select / Back`.
   The former missing-result EA020101 at908,604,350 and bad-pointer stop at
   908,612,174 are now passed. The required preceding RF generation at
   908,166,322 is the independently captured tag-clear0..4/0335 transition
   retaining tagged810/033f in the first eight extension bytes; the remaining
   24 extension bytes are zero. The decoder accepts only that exact history,
   channel/profile/tag/residue shape, and producer-table reuse is restricted
   to the drained00ff generation. A930M SoftLeft run accepts the five slots at
   908,166,322, completes them at908,331,008 through908,447,488, and stops only
   at the requested instruction budget with no READ_UNMAPPED or INSN_INVALID.
   Proof: `rf-menu-transition-fixed-930m.log`, `.mmio`, `.ppm`, and local OCR
   image `rf-menu-transition-fixed-930m-ocr.png`.
   Match bounded active traces with independent peeks before adding any
   future tail/profile exception. The old READ_UNMAPPED at443,812,212,
   PC20847af4/address e24ee600 is passed; no address mapping/error suppression.
   Preserve rejection for unsupported/partial/noncontiguous batches and rerun
   ARM contracts, long boot and exact no-SIM baseline after another change.
   Raw -400 and deadlines remain unchanged. The three-request mask3f
   and885/0333-or-033d-or-033f residue
   provenance are established; arbitrary transitions are not.
   Do not just add profiles to an allowlist, ignore extension bytes,
   replay idle commands or seed a passing result. Unknown profiles remain
   rejected; full RF/demodulation behavior is still absent.
   70012c01 belongs to the other analog conversion and is
   not by itself proof of a halt. Do not answer address3d with the battery
   sample, seed a passing message or suppress original assertions.
   The first EA020101 is passed under the declared synthetic empty-air
   model, not fixed in default hardware emulation. Keep RF timing/
   environment assumptions explicit, restrict unsupported commands, and keep
   the mode default-off. No full RD16024 interpreter is required to continue
   HLE. Reference-device/ISA evidence is still needed before claiming accurate
   physical RF behavior. Preserve payload after ACK and original IRQ21 delivery.
   Reproduce request/consumer checks with inspect_dsp_schedule.py,
   verify_rf_command.py, verify_rf_result_math.py and verify_rf_collector.py; these are ARM contracts,
   not evidence of DSP ISA/timing. Upload identity is already checked by
   inspect_dsp_upload.py (including original word0014=4634); do not repeat
   searches for its binary patterns expecting known opcodes. Unknown classes,
   other calibration/band profiles, demodulation and registration remain absent.
2. Use the new hardware keypad to trace no-card UI decisions and test remaining
   semantic key mappings. Digit1 opens the dialer; no-card OK/SoftLeft have not
   opened an application list in earlier checks. The latest SIM-present
   SoftLeft now remains on OCR Messages/Select/Back through930M after the scoped
   RF transition. Validate right softkey Back and arrows without seeded
   guest messages. Investigate the user's reported GUI freeze after one digit;
   sustained multi-digit entry has not been verified. Physical timing/debounce/
   autorepeat remain unverified.
3. Establish the real SIM/UI transition and earliest missing event, and trace
   the original insert_sim IFG load/decode path for the colour anomaly already
   present in guest graphics RAM. A sampled
   idle PC is not a hang: the bounded trace shows repeated returns to RTOS work.
   Raw clock rates, additional DMA/LCD modes and wake behavior remain unverified.
4. Keep PCF5060X status/RTC and default DSP completions evidence-based.
   Experimental HLE assumptions must be declared and separately opt-in;
   do not invent power events or mailbox results merely to select a branch.
5. Preserve the real firmware, NAND and RTOS state; no forced IRQ, direct state
   seeding, fake menu or firmware patch is permitted.

Keep the real firmware untouched, preserve the desktop interface, and record
new evidence here. A missing filesystem must not be bypassed with RTOS state,
manual clock changes, a fake menu or fabricated success returns.

### Checkpoint — 2026-11-01: secondary timer releases the post-menu-animation wait

The secondary timer status at `0x5040901c` is device-owned W0C state, not a
guest enable register. Compare channels are armed by their control bits; a
counter crossing now latches the corresponding status bit and projects IRQ17
or IRQ18 only while that channel remains enabled. Guest writes can acknowledge
latched zero bits but cannot manufacture a timer interrupt.

In the SIM-present empty-air run, timer 0 channel 1 crosses compare `0x4b35`
at instruction `1,740,265,902`, sets status `0x04`, and raises IRQ18. The
original ISR reads that status, clears control bit `0x20`, acknowledges status
with `0xfb`, and invokes callback `0x204f4d10` at `1,740,266,077`. The callback
signals semaphore `0xbc`; UI task `0x3002e064` resumes at `1,740,266,769` and
returns past its former wait. Repeated callback execution was also observed.

The bounded follow-up reaches 161 LCD frames instead of 160 with no CPU or
memory fault, but its final saved frame is unchanged. This proves the specific
missing timer callback is fixed; continued interactive menu navigation still
requires a live GUI check and any later stall must be diagnosed separately.
Proof: `secondary-timer-status-trace-v3.log`,
`secondary-timer-callback-flow-v1.log`,
`secondary-timer-ui-resume-v1.log`, and
`secondary-timer-menu-progress-v1.log` under `proof/native-windows`.

### Checkpoint — 2026-11-01: post-animation direction input and GUI latency

A fresh build with the corrected secondary timer receives a late `Down` after
the menu animation through the complete original path: keypad IRQ16 enters
`0x208e51ac`, callback `0x2118bf80` runs, and HKBD emits press/release messages
`0x2203`/`0x2204` with key code `0x15`. The run advances from the former
161-frame checkpoint to 169 LCD frames by 1.84B instructions, changes the
saved frame, and has no CPU or memory fault. The firmware's initialized table
also confirms direction codes `Up=0x14`, `Down=0x15`, `Left=0x16`, and
`Right=0x17`; no direction remapping is justified.

The desktop worker previously ran two million instructions per batch and
drained every queued input transition before the next batch. A fast GUI click
could therefore queue both press and release while the firmware had no CPU
opportunity between them. Input-free execution retains the normal two-million-
instruction batch for boot speed. Only one queued transition is applied per
batch. The first GUI version used a 250,000-instruction input batch, which was
enough to observe IRQ16/HKBD delivery but was shorter than the successful late
CLI probe's one-million-instruction press/release separation and failed the live
GUI check. Input batches now use that proven one-million-instruction interval so
the UI task can consume the press before release. Firmware, key coordinates,
NAND and guest state remain unchanged. Proof:
`secondary-timer-post-animation-down-v1.log` and `.ppm`.

### Checkpoint — 2026-11-02: all four late menu directions compared

Five otherwise identical SIM/RF/analog-HLE runs open the genuine
`Messages / Select / Back` screen with SoftLeft at 1,740,000,000 instructions.
The control run applies no second key; the four comparison runs apply one
direction at 1,820,000,000 and release it at 1,821,000,000. All five reach
1,830,000,000 with `InstructionBudget`, `memory_fault: None` and 169 LCD
frames. `Up`, `Down`, `Left` and `Right` all retain exactly the control frame:
SHA-256 `de99a5ce285fd674da4d3ed9bbc57c400ba6e636fdca1e809d4d75e27f0feb34`.

This rules out a single swapped direction as the visible-menu failure. It
does not rule out keypad delivery: the earlier routing trace proves `Down`
passes the original HKBD dispatcher and queue function. Continue at the task
15 queue consumer/scheduling path rather than remapping direction codes or
changing GUI coordinates. Proof: `late-menu-baseline-v34`,
`late-menu-up-v35`, `late-menu-down-v36`, `late-menu-left-v37` and
`late-menu-right-v38` (`.log`, `.mmio`, `.ppm`) under
`proof/native-windows`.

### Checkpoint — 2026-11-02: late Up reaches the common HKBD message queue

Focused comparison now rules out keypad lookup and HKBD message construction as
the cause of the inert menu arrows. Two otherwise identical SIM/RF/analog-HLE
runs open the genuine menu with SoftLeft at 900,000,000 instructions and apply
either digit `1` or `Up` at 920,000,000, releasing it at 921,000,000. Both reach
921,050,000 with `InstructionBudget` and no CPU or memory fault.

The live initialized keypad table resolves digit `1` input `0x0801` at
`0x30031994` and `Up` input `0x0108` at `0x30031a44`. The second lookup from
`0x20140be6` also returns the same corresponding records. For `Up`, the original
HKBD sender at `0x215a61ce` receives semantic key code `0x14` in R4/R5 and takes
the direction-event branch with R6 zero. It writes `0x14` into the outgoing
object, then calls the same queue function `0x214d90c4` used by the working
digit path with message `0x2203`. The digit path carries key code `1`, group
value `3`; the direction path carries key code `0x14`, group value zero. This is
the original firmware's expected event-format split, not evidence for a host
key remap.

Therefore `PhoneKey::Up => (8,1)` remains correct, the live lookup table is not
damaged, and the event is not lost before the common queue. Do not add a
sentinel, remap directions or synthesize a callback. Continue from the consumer
of the `0x214d90c4` queue and compare active-screen dispatch of the working
digit payload with the ignored direction payload. Proof:
`late-digit-ui-v88.log/.mmio` and `late-up-ui-v89.log/.mmio` under
`proof/native-windows`. The private traces are diagnostic artifacts and are not
part of the public repository.

### Checkpoint — 2026-11-02: static EDP consumer boundary and focused late-input trace

The local E830 DEF and CLA now move the investigation beyond the generic send
routine without publishing either input. `PROCESS_HKBD` is 34 (`0x22`),
`PROCESS_EDP` is 59 (`0x3b`), and `0x2203` is
`MOBI_HKBD_KEY_PRESSED_IND`. At the proven press path, `0x214d90c4` therefore
sends the payload from HKBD to EDP. EDP state table `0x20aa5fbc` maps message
`0x2203` to callback `0x20d75404`.

Names recovered from the supplied D500 MAP, then checked against the E830 code
shape and state table, identify `0x20d75404` as the E830 equivalent of
`edp0_1PipeKeyPressed` and `0x20d76304` as the equivalent of
`edp0_6DiffuseKbdEvents`. D500 addresses were not transferred, and the complete
MAP remains local. The E830 path transforms the HKBD event into
`APPI_EDP_KEY_PRESSED_IND` (`0x3b5b`) before MMI processing. A working digit can
take a direct application-notification branch; direction key `0x14` instead
depends on the diffusion path.

The `0x14` comparison in `edp0_1PipeKeyPressed` selects an alternate/editor
route rather than proving a drop. The D500 name `edp2_51NoDiffAppiMessages`
matches the role and compact shape of E830 routine `0x20559318`, but static E830
decoding now excludes it as the ordinary-menu cause: it first requires
`[0x3036c108] == 8`, and nearby firmware strings name state 8
`MMI_STATE_JAVA`. Only that Java state can combine with RAM byte `0x3048c264`
to release the event object at `0x20d7632a`. The Samsung menu should continue
through `0x20d7634e`, leaving MMI keyboard configuration and key filtering as
the leading boundary.

Correction: `0x302d3b80` is not an active UI action queue. The D500 MAP names
the matching 20-by-`0x1c` object `a_StoredMsgInfoTable`, and the E830 call shape
places it in `lk5` saved-message handling. The previous empty-table observation
therefore says nothing about ordinary menu navigation.

The stronger MMI path is now `0x2119fe50` (`lk1_1rout_fsm` equivalent),
`0x20156e80` (`lk2_set_keyboard` equivalent) and `0x211a57c0..0x211a599e`
(`lk2_33key_in_filter` equivalent). `lk2_set_keyboard` stores the active profile
at `0x3040067c` and sends `APPI_EDP_CHANGE_KBD_CONFIG_REQ` (`0x3b68`). The key
EDP table routes that request to `0x21582028`, which forwards its payload to
HKBD as `MOBI_HKBD_CONFIG_REQ` (`0x2201`) at `0x21582042`. The key filter has
an explicit jump-table branch at `0x211a593a` for normalized key
`0x14`, testing keyboard-mask bits before returning its decision.

The `MCFN_MCC_GETKEY_ACCEPT_REJECT_S` caller at `0x200d4d94` establishes the
return convention:
filter result 1 continues normal handling, while other values reach the
rejected-key path at `0x200d4dcc`. Digit `1` tests mask bit 3; Up (`0x14`) tests
bit 17. The same menu parser installs the source masks at `0x200d4cc4` and then
requests keyboard profile 4 or 5. A stale or incorrectly delivered profile is
therefore one possible downstream cause, although the actual late-menu mask
values still require a paired runtime trace.

Final field-order check: generic HKBD sender `0x215a614c` stores its third
argument at event `+9` as `keyInfo` and its second argument at `+0xa` as the
digit/key offset. Short-release site `0x20140efe` passes `r3=2`. EDP builder
`0x205585e4` then orders source bytes `+8/+9/+0xa` as outgoing
`key/key_info/key_offset`. R1 at the shared filter is therefore `key_info`, and
the fifth stack argument is `key_offset`. For Up the predicate is
`mask_a.bit17 && (key_offset != 1 || mask_b.bit17)`. The passive diagnostic
records both fields, both masks, both Up bits and the final result.

The late direction trace already proves Up enters the generic sender with its
second argument zero (`record+2 == 0`). In the `0x2204` branch at `0x215a6240`,
that selects the special direction encoding: `record+6` becomes payload key
`+8`, while payload `+0xa` is explicitly zeroed. EDP carries that byte to
`3b53 +0xc`, proving ordinary Up has `key_offset=0`. Mask B therefore does not
participate; only mask A bit 17 decides acceptance.

When `MCC_GETKEY_S` accepts a key, `0x21112b96` forwards it to common UI
dispatch. That dispatcher has an explicit Up case at `0x215b8a30`, mapping
semantic key `0x14` to command `0xce`. Up is therefore not designed as a no-op
at this consumer; failure before this point is a real delivery/filter problem.

Command `0xce` enters common UI dispatcher `0x2159dd48`, which has several
firmware-state gates before its action call at `0x2159de34` (plus alternate
paths). The passive trace now covers entry, early return and action invocation,
so it also detects a downstream UI suppression after a successful key filter.

Cross-build table ordering identifies two other callers of the same filter:
`0x21112b34` is `MCFN_MCC_GETKEY_S`, and `0x2153667c` is
`MCFN_MCC_GETKEY_INCALL_S`. Static code alone does not prove which MCFN command
owns the visible `Messages / Select / Back` screen. The trace now classifies
the universal filter entry by LR instead of assuming the accept/reject caller.

Correction: the two key masks are fields of the dynamic MCFN command object at
`+0x1b0/+0x1b8`, not globals in `0x302e7714`. The object base is R6 for
accept/reject, R4 for `GETKEY_S`, and R5 for `GETKEY_INCALL_S`. The universal
filter trace now selects the base by LR and dumps `+0x180..+0x1bf`; no stale-RAM
claim is made without that runtime value.

Further static decoding identifies `0x21112a6c` as the `MCC_GETKEY_S` setup and
`0x2153656c` as the analogous `MCC_GETKEY_INCALL` setup. Each packs active
descriptor halfwords `+2/+4` directly into mask A at command object `+0x1b0`.
The passive trace captures descriptor and destination context before this copy,
so a clear Up bit can be attributed either to intended scenario data or to an
earlier wrong-descriptor selection.

For ordinary short/release Up, `key_info=2` and the direction encoding proves
`key_offset=0`. Mask B does not participate; only mask A bit 17 decides
acceptance. `MCC_GETKEY_S` loads mask A directly from the active scenario
descriptor. Therefore the emulator must not patch that bit. The remaining
in-scope causes are missing `0x2204/0x3b53` delivery, a descriptor whose mask A
rejects Up, or an earlier emulation error that selected the wrong MCFN
command/descriptor.

Further static decoding corrects the scope of that filter. It handles
`APPI_EDP_KEY_INFO_IND` (`0x3b53`), not immediate pressed indication `0x3b5b`.
MMI consumes `0x3b5b` at `0x211a08b6`; Up reaches the distinct direction path
at `0x211a27e6`. HKBD's later short/release logic emits
`MOBI_HKBD_KEY_INFO_IND` (`0x2204`) at `0x20140efe`; EDP consumes it at
`0x205579b8`, builds `0x3b53` at `0x205585e4`, and forwards it at
`0x2055864c`. The next paired trace must first prove whether Up produces this
second-stage event before blaming the MMI mask itself.

The generic HKBD sender's actual `0x2204` queue call is `0x215a6266`, distinct
from the earlier `0x2203` queue call at `0x215a6236`. The passive trace now
captures both the request at `0x20140efe` and the completed payload immediately
before its HKBD-to-EDP queue insertion at `0x215a6266`.

### Checkpoint — 2026-11-03: true navigation consumer narrowed

Static decoding establishes that the active-menu mask filter is driven by
`APPI_EDP_KEY_INFO_IND` (`0x3b53`), not directly by the already proven
`APPI_EDP_KEY_PRESSED_IND` (`0x3b5b`). HKBD can emit the corresponding
`MOBI_HKBD_KEY_INFO_IND` (`0x2204`) from its short/release processing; EDP then
builds and forwards `0x3b53`. The earlier late Up trace stopped after proving
the `0x2203/0x3b5b` path, so it cannot establish whether the active menu ever
received Up as `KEY_INFO`.

No contact-lifetime change is justified yet. Existing historical probes used
different screens and hold intervals; they cannot prove that increasing the
late-menu GUI hold fixes Up. The worker remains unchanged. The next paired
trace must compare digit and Up at `0x20140efe`, `0x205579b8`, `0x205585e4`
and universal filter entry `0x211a57c0`, recording its caller, mask A/B,
key_info and result before any behavioral change.

Static decoding adds an earlier and more likely `0x2204` boundary. HKBD loads
the active 8-byte key record from `[0x301ce2f4+8]`. Record type 3 selects
configuration slot 3; record type 0 selects the slot at record `+4`. Only slot
values 2 or 4 reach `0x20140ec4`; all other types/values branch to
`0x20140f1a` without sending `0x2204`. The menu trace now records key-record
fields `+2/+4/+6`, selected slot/value and both branch destinations. A previous
post-animation Down run emitted `0x2204`, so the release hardware path works in
at least one screen/configuration; the active menu may instead have stale or
wrong keyboard configuration.

HKBD state table entry `0x20a8e88c` maps `MOBI_HKBD_CONFIG_REQ` (`0x2201`) to
handler `0x20d9221c`. The handler reads a pair count from payload `+9` and
copies each `(slot,value)` pair beginning at `+0xa` directly into
`0x301ce2c8`, ignoring only slots `>=0x1b`. The trace now captures the full
payload on entry and the complete post-copy configuration at `0x20d9225a`.
This tests whether the active menu sent the wrong profile data or whether the
correct `3b68 -> 2201` update failed to reach HKBD before release.

New opt-in `E830_TRACE_MENU_INPUT` diagnostics cover these exact HKBD, EDP and
MMI boundaries, dump only ordinary RAM, report keyboard profile/config sends
and the complete `0x2204 -> 0x3b53` route, record every write to the
`0x3048c264` Java-only gate, and are bounded by `_AFTER` and `_LIMIT`. They do
not read MMIO, modify guest state, inject a message or alter keypad handling.
The trace now also covers the three release-gate comparisons at `0x20140edc`
and their decision at `0x20140ef0`. D500 cross-build names resolve helper
`0x2001eb3e` as `RtkNumberMsgInMailBox`, selector table `0x20a8ea9c` as
`a_hkbd_ProcessToScan`, and limits `0x20a8e91c` as `a_hkbd_MaxMailbox`. E830
code and data agree: it checks HSC/EDP/HBAT (`0x23/0x3b/0x50`) mailbox counts
against `40/10/10`; any full mailbox skips `0x2204`. This is not a duration,
hardware-timer, keypad-MMIO or I2C check. If it fires in the late menu, the
emulator-owned lead is RTOS queue consumption/scheduling, not contact lifetime.
The passive trace now dumps the full `0x38`-byte HSC, EDP and HBAT process
records at this boundary, not only the three summed mailbox counts.
It also records their entries from process directory
`0x3002eaac + process*8`, so a full mailbox can be tied to its task group.

The identical scan on the press path is now traced at
`0x20140c1e/0x20140c3e`. Because the existing Up trace proves `0x2203` was
sent, those counts were below their limits before the press. RTOS receive at
`0x208476ca..0x20847724` decrements the recipient's mailbox count before its
state-table callback, and the existing trace reaches EDP callback `0x20d75404`.
Thus the processed Up press itself cannot remain as EDP's tenth message at
release. The paired scan still tests later EDP traffic and HSC/HBAT
backpressure, but the simple EDP `9 -> 10` case is no longer the leading cause.
Static source table `0x205eaa28 + process*8`, copied into that directory during
initialization, maps HSC to task group 2, EDP to group 4 and HBAT to group 1.
Therefore an EDP count of 10 narrows the emulator-owned scheduler lead to task
group 4 rather than the previously investigated UI task 15. The menu trace now
also includes the current RTOS task record at every sampled boundary.
Static extraction of the immutable HKBD key-record table at `0x201cbf94` now
fixes the Up record at `0x201cc044`: physical `0x0108`, key offset zero,
configuration slot 20 and semantic key `0x14`. HKBD initialization `0x215a5f60`
clears the configuration array, and the profile-4 branch `0x2015708c` writes
`4,11,12=1`, `18,19=0`, `6,7=3` without touching slot 20. Profile 5 at
`0x201570e6` likewise writes `5,11,12=1`, `18,19=3` without slot 20. A stale
zero for slot 20 therefore explains the observed release rejection without
requiring a firmware patch or event injection; the runtime trace must still
prove the active branch and actual slot value first. Diagnostic coverage now
includes profile-4/5 branch entries and the key-record lookup at `0x215a5fc0`.

The next paired run must identify the first missing or differing boundary and
then confirm visible navigation after an emulator-owned fix. No build or
runtime test was performed for this checkpoint at the user's request.

### Checkpoint — 2026-11-03: static decode of the HKBD config builder and profile map

The `0x2201` configuration builder `0x20e008e8` was decoded against its
consumer `0x20d9221c`. The payload format is `(value, slot count, slots...)`,
not `(slot,value)` pairs; every `lk2_set_keyboard` call site agrees with the
builder order. The complete key-record table is at `0x201cbf94` (27 x 8B;
`physical,type,slot,key`), with Up = record 22 (slot 20), Down = record 21
(slot 21), Left = 22, Right = 23.

The static bodies of profiles 2..20 were decoded from the jump table at
`0x20156f24`. Only profile 18 writes slot 20 (Up) with the release-valid value
`2`; no profile writes slot 21 (Down) with `2/4`. The release gate
`0x20140e9c` still requires slot `2` or `4`. Because Down is reported working,
the active menu profile and the real `0x301ce2c8[20]/[21]` byte values are the
remaining unknown; that requires the runtime trace, not more static decoding.
A second HKBD slot-clearing loop was found at `0x20989e58` (all 27 slots,
bound `0x1b`); order relative to profile writes must be captured in the same
trace.
