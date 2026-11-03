# E830 late-menu HKBD input slice

Status: trace-backed producer plus static E830 consumer slice, 2026-11-03.
Firmware images, full DEF/MAP files and private traces are intentionally omitted.

## Established path

The following is semantic pseudocode, not a line-for-line C reconstruction.
Unknown object fields and unverified branch conditions are deliberately left
unnamed.

```c
void hkbd_handle_matrix_event(uint16_t matrix_input, bool pressed)
{
    const KeyRecord *record = keypad_lookup(matrix_input);
    if (record == NULL)
        return;

    uint8_t key = record->semantic_key;

    if (pressed) {
        HkbdMessage *message = hkbd_build_press_message();
        message->key = key;

        /* The original firmware uses a separate payload group for ordinary
           digits and direction keys. */
        message->group = is_direction_key(key) ? 0 : record->group;
        hkbd_queue_message(0x2203, message);  /* 0x214d90c4 */
    } else {
        hkbd_queue_release_or_info(record);
    }
}
```

Observed values for the compared press paths:

| Input | Matrix value | Live record | Semantic key | Group | Message |
| --- | ---: | ---: | ---: | ---: | ---: |
| Digit `1` | `0x0801` | `0x30031994` | `0x01` | `0x03` | `0x2203` |
| Up | `0x0108` | `0x30031a44` | `0x14` | `0x00` | `0x2203` |

The initialized direction-key table gives `Up=0x14`, `Down=0x15`,
`Left=0x16`, and `Right=0x17`.

## Trace-backed address annotations

| Address | Annotation |
| ---: | --- |
| `0x208e51ac` | IRQ16 keypad handler entry observed for late input. |
| `0x2118bf80` | Keypad callback reached from the original IRQ path. |
| `0x20140be6` | Second live keypad-record lookup site. |
| `0x215a61ce` | HKBD press sender site; `Up` arrives as key `0x14`. |
| `0x214d90c4` | Common queue function called by digit and direction paths. |
| `0x20aa5fbc` | EDP state table; `0x2203` selects `0x20d75404`. |
| `0x20d75404` | `edp0_1PipeKeyPressed` equivalent for the E830 image. |
| `0x20d76304` | `edp0_6DiffuseKbdEvents` equivalent for the E830 image. |
| `0x20156b34` | Wildcard callback selected by `a_lk_StandbyLook`. |
| `0x2119fe50` | E830 equivalent of the large `lk1_1rout_fsm` root router. |
| `0x20156e80` | E830 equivalent of `lk2_set_keyboard`. |
| `0x21582028` | EDP handler for `APPI_EDP_CHANGE_KBD_CONFIG_REQ` (`0x3b68`). |
| `0x21582042` | EDP forwards the payload as `MOBI_HKBD_CONFIG_REQ` (`0x2201`). |
| `0x20d9221c` | HKBD consumes `0x2201` and applies its slot/value pairs. |
| `0x20d9225a` | All configuration pairs have been copied to `0x301ce2c8`. |
| `0x20140efe` | HKBD requests `MOBI_HKBD_KEY_INFO_IND` (`0x2204`). |
| `0x215a6266` | Generic HKBD sender queues the completed `0x2204` payload to EDP. |
| `0x205579b8` | EDP consumer for `0x2204`. |
| `0x205585e4` | EDP builds `APPI_EDP_KEY_INFO_IND` (`0x3b53`). |
| `0x211a08b6` | MMI has loaded the `APPI_EDP_KEY_PRESSED_IND` payload. |
| `0x211a27e6` | Direction-specific part of the `0x3b5b` handler. |
| `0x211a57c0` | E830 equivalent of `lk2_33key_in_filter`. |
| `0x211a593a` | Key-filter branch selected for normalized key `0x14` (Up). |
| `0x21112b34` | E830 equivalent of `MCFN_MCC_GETKEY_S`. |
| `0x21112b84` | `MCC_GETKEY_S` calls the shared filter. |
| `0x21112b96` | Accepted `MCC_GETKEY_S` key enters UI dispatch. |
| `0x2153667c` | E830 equivalent of `MCFN_MCC_GETKEY_INCALL_S`. |
| `0x200d4cc4` | E830 equivalent of `MCFN_MCC_GETKEY_ACCEPT_REJECT`. |
| `0x200d4d28` | E830 equivalent of `MCFN_MCC_GETKEY_ACCEPT_REJECT_S`. |
| `0x200d4d94` | Accept/reject handler calls the `KEY_INFO` filter. |
| `0x200d4dcc` | Its rejected-key path after a non-1 filter result. |

## What this rules out

- The GUI-to-matrix mapping for Up is not missing: it produces `0x0108`.
- The live keypad table resolves that value to its initialized Up record.
- HKBD preserves semantic key `0x14` and creates press message `0x2203`.
- The direction event reaches the same queue function as a working digit event.

Therefore a host remap, sentinel key, synthetic callback or fabricated menu
event is not justified by current evidence.

## Static consumer path

The local E830 DEF identifies process 34 as HKBD and process 59 (`0x3b`) as
EDP.  At the `0x2203` send site, `0x214d90c4` receives sender `0x22`, recipient
`0x3b`, message `0x2203` and the allocated payload.  The first EDP state-table
entry is therefore the real consumer boundary, not the generic RTOS send
routine.

Static decoding establishes this path:

```text
MOBI_HKBD_KEY_PRESSED_IND (0x2203)
  -> EDP state table 0x20aa5fbc
  -> 0x20d75404 (edp0_1PipeKeyPressed equivalent)
  -> 0x20d76304 (edp0_6DiffuseKbdEvents equivalent)
  -> APPI_EDP_KEY_PRESSED_IND (0x3b5b)
  -> MMI / 0x211a08b6

HKBD short/release processing
  -> MOBI_HKBD_KEY_INFO_IND (0x2204)
  -> EDP callback 0x205579b8
  -> 0x205585e4 (build APPI_EDP_KEY_INFO_IND, 0x3b53)
  -> MMI active-menu parser
  -> 0x211a57c0 (lk2_33key_in_filter equivalent)
```

The D500 MAP was used only to recover the stable function names above.  Its
addresses and complete symbol set are not E830 evidence and are not included.

The digit and direction paths are intentionally different inside EDP.  A digit
can take a direct application-notification branch, while direction key `0x14`
enters an alternate/editor branch and then the keyboard-event diffusion path.
That distinction is expected; it is not itself a drop.

An apparent early drop can now be excluded for the ordinary menu.  The D500
name `edp2_51NoDiffAppiMessages` matches the role and compact shape of E830
routine `0x20559318`.  That E830 predicate first requires
`gs_look.v_State == 8`; firmware diagnostic strings identify state 8 as
`MMI_STATE_JAVA`.  Only then can RAM byte `0x3048c264` cause
`edp0_6DiffuseKbdEvents` to release the object at `0x20d7632a`.  The active
Samsung menu is not the Java state, so this branch is retained in diagnostics
but is no longer the leading explanation.

The earlier interpretation of `0x302d3b80` as a UI action queue was wrong.
D500 names the corresponding 20-by-`0x1c` object
`a_StoredMsgInfoTable`; E830 call shape agrees that it belongs to `lk5` saved
message handling. It is not the active menu key consumer and is no longer
sampled by the late-input diagnostic.

Static E830 decoding now identifies two stronger consumer boundaries.
`0x20156e80` is the `lk2_set_keyboard` equivalent: it compares the requested
profile with byte `0x3040067c`, builds one or more
`APPI_EDP_CHANGE_KBD_CONFIG_REQ` (`0x3b68`) payloads and sends them to EDP.
EDP state table `0x20aa5fbc` maps `0x3b68` to `0x21582028`; that handler
forwards the contained configuration to HKBD as `MOBI_HKBD_CONFIG_REQ`
(`0x2201`) at `0x21582042`.
`0x211a57c0..0x211a599e` is a separate 480-byte key-filter routine, close to
the 500-byte D500 `lk2_33key_in_filter`; it switches on normalized keys
`0..0x18` and tests per-key bits in current and requested keyboard masks.
Its jump table selects `0x211a593a` for Up (`0x14`). This is static
identification, not yet a runtime-proven drop or an emulator fix.

The shared MCFN mask filter belongs to the `KEY_INFO` path, not directly to
`KEY_PRESSED`. The `0x3b5b` branch at `0x211a08b6` handles the immediate press;
for direction key `0x14` it reaches `0x211a27e6`. HKBD separately produces
`0x2204`, which EDP transforms into `0x3b53`; this later event reaches the
active-menu filter.

The accept/reject caller confirms the return convention. At `0x200d4d94` it
passes the normalized key and two menu-owned masks into the filter; only return
value `1` continues normal handling. A different value branches to
`0x200d4dcc`, which reports/ignores the key except for a few special cases.
For digit `1`, the filter tests bit 3. For Up (`0x14`), it tests bit 17.
Rejected Up has an additional firmware exception controlled by RAM byte
`0x304013a7`; the passive trace records it as `rejected_up_override`.
The generic HKBD sender at `0x215a614c` fixes the source-field semantics: its
third argument is logged and stored at event byte `+9` as `keyInfo`, while its
second argument is stored at `+0xa` as the digit/key offset. The short-release
call at `0x20140efe` passes `r3=2`. EDP builder `0x205585e4` then deliberately
orders the outgoing bytes as source `+8`, `+9`, `+0xa`, i.e. `key`, `key_info`,
`key_offset`. Thus the filter arguments are `key` (R0), `key_info` (R1), two
32-bit masks (R2/R3), and `key_offset` (the fifth stack argument). For Up the
exact predicate is `mask_a.bit17 && (key_offset != 1 || mask_b.bit17)`.

The direction-format trace already proves Up reaches the generic sender with
its second argument zero (`record+2 == 0`). In the `0x2204` branch at
`0x215a6240`, that selects the special encoding which writes `record+6` as the
key at payload `+8` and explicitly writes zero at payload `+0xa`. EDP carries
that byte to `3b53 +0xc`, so ordinary Up has `key_offset=0`. Therefore only
mask A bit 17 decides Up acceptance; mask B is irrelevant for this event. The
diagnostic still reports both raw fields and both mask bits.
`0x2204/0x3b53` delivery and the actual MCFN
caller/masks are now the unresolved boundaries. Runtime values are still
required before changing behavior.

An accepted Up is not a deliberate no-op: common UI dispatch compares semantic
key `0x14` at `0x215b8a30` and maps it to command `0xce`. Therefore failure to
reach that point is a real rejection/delivery problem, not merely the focus
already being at a list boundary.

Command `0xce` then enters common UI dispatcher `0x2159dd48`. That routine has
multiple firmware-state gates before invoking an action at `0x2159de34` or an
alternate call site. The trace covers entry, early return and action calls so a
post-filter suppression is distinguishable from missing input.

The masks belong to the dynamic MCFN command object at offsets `+0x1b0` and
`+0x1b8`, not to global `0x302e7714`. At the shared filter entry the object is
held in R6 for accept/reject, R4 for `GETKEY_S`, and R5 for `GETKEY_INCALL_S`.
The trace selects the correct base from LR and dumps its `+0x180..+0x1bf`
context.

The `MCC_GETKEY_S` setup routine starts at `0x21112a6c`; the corresponding
`MCC_GETKEY_INCALL` setup starts at `0x2153656c`. Their active descriptor is the
pointer at the command object's first word; halfwords `+2/+4` are packed
directly into mask A at object `+0x1b0`. The passive trace dumps both descriptor
and destination context before the copy. This distinguishes a legitimate
scenario mask with bit 17 clear from an earlier emulator error that selected
the wrong descriptor.

For ordinary short/release Up, `key_info=2` and `key_offset=0`; mask B is
irrelevant and mask A bit 17 alone decides acceptance. In `MCC_GETKEY_S`, setup
loads mask A directly from the active scenario descriptor. The remaining
emulator-owned possibilities are therefore (a) Up never reaches `0x3b53`, (b)
the active descriptor legitimately or incorrectly has mask A bit 17 clear, or
(c) an earlier emulation error selected the wrong MCFN command/descriptor. Do
not force bit 17.

## Next runtime boundary

Run the opt-in `E830_TRACE_MENU_INPUT=1` diagnostic for otherwise identical
digit and Up inputs.  It samples only ordinary guest RAM and reports:

1. which branch `0x20d75404` selects;
2. confirmation that the Java-only `0x20559318` no-diff predicate stays false;
3. whether `0x20d7632a` drops the direction object, `0x20d76334` takes the
   special no-diff route, or `0x20d7634e` continues toward MMI;
4. calls to `lk2_set_keyboard`, the active profile byte at `0x3040067c`, and
   the `0x3b68` sends plus their `0x2201` forwarding from EDP to HKBD;
5. whether Up requests `0x2204` at `0x20140efe`, queues its completed payload
   at `0x215a6266`, and reaches EDP/MMI as `0x3b53` through
   `0x205579b8/0x205585e4`;
6. entry to `lk2_33key_in_filter`, selection of the Up branch at `0x211a593a`,
   both keyboard masks and the final filter result at `0x211a599e`;
7. `filter_caller`, `filter_key_info`, `filter_key_offset`, mask A/B and their
   Up bits at the
   universal `0x211a57c0` entry, identifying whether
   the active screen is using `MCC_GETKEY_S`, `MCC_GETKEY_INCALL_S`,
   `MCC_GETKEY_ACCEPT_REJECT_S`, or another caller.

After builds are allowed again, reproduce with the local installed NAND and a
fresh capture name. The Up run should keep executing after release so the
second-stage event can complete:

```powershell
python .\tools\run-probe.py `
  --env E830_SIM_PRESENT=1 --env E830_DSP_BOOT_HLE=1 `
  --env E830_DSP_RF_HLE=empty --env E830_ANALOG_HLE=battery `
  --env E830_TRACE_MENU_INPUT=1 `
  --env E830_TRACE_MENU_INPUT_AFTER=899000000 `
  --env E830_TRACE_MENU_INPUT_LIMIT=400 late-up-key-info-v1 `
  -- --onenand-image "C:\Users\ALEXX\Downloads\firmware\firmware\E830XEGG1.onenand.bin" `
  --max-instructions 930000000 `
  --key-at 900000000:soft-left:down --key-at 901000000:soft-left:up `
  --key-at 920000000:up:down --key-at 921000000:up:up
```

The first unresolved runtime boundary is now the second-stage `KEY_INFO` path.
The existing late Up trace proves only `0x2203`; it does not show whether Up
reaches `0x2204`, `0x3b53`, or the active-menu mask filter. Historical probes
from other screens do not justify changing contact lifetime. No behavioral fix
has been made yet.

There is an earlier release gate before the mailbox scan. HKBD loads the active
8-byte key record from `[0x301ce2f4+8]`. Record type `3` selects configuration
slot 3; type `0` selects the slot indexed by record `+4`. Only slot values `2`
or `4` reach `0x20140ec4` and continue toward `0x2204`; other record types or
slot values branch to `0x20140f1a`. The trace now reports record `+2/+4/+6`, the
selected slot/value, and explicit accepted/rejected PCs. Since an earlier late
Down run emitted `0x2204`, this gate is screen/profile dependent rather than a
fundamental keypad-release failure.

The immutable key-record table begins at `0x201cbf94`. Each entry is eight bytes:
physical code, key offset, configuration slot and semantic key. The Up record is
`0x201cc044`: physical `0x0108`, offset `0`, slot `20`, semantic key `0x14`.
HKBD init at `0x215a5f60` clears all configuration slots before profiles are
requested. The profile-4 branch at `0x2015708c` writes only `4,11,12=1`,
`18,19=0`, and `6,7=3`; it does not write slot 20. Therefore a stale zero in
slot 20 is now the leading static candidate for the rejected release path. The
profile-5 branch at `0x201570e6` writes `5,11,12=1` and `18,19=3`, also without
slot 20. Runtime must prove which branch and slot value are actually active
before any behavioral change.

The `0x2201` state-table entry selects HKBD handler `0x20d9221c`. Its payload
contains a pair count at `+9`, followed by `(slot,value)` bytes at `+0xa`; each
valid slot below `0x1b` is copied directly to `0x301ce2c8`. The trace covers
handler entry and completion at `0x20d9225a`, including the complete payload and
post-copy configuration. This separates a wrong MMI profile payload from a
delivery/scheduling failure between `3b68`, EDP and HKBD.

Static decoding also identifies the exact HKBD release gate before `0x2204`.
D500 names the matching helper `RtkNumberMsgInMailBox` and the two three-byte
tables `a_hkbd_ProcessToScan` and `a_hkbd_MaxMailbox`. E830 code shape and data
match exactly: helper `0x2001eb3e` indexes the RTOS process record and sums its
two mailbox-count halfwords; process IDs are `0x23/0x3b/0x50` (HSC/EDP/HBAT),
with maxima `40/10/10`. At `0x20140edc`, any count at or above its maximum makes
`0x20140ef0` skip the `0x2204` send. This is mailbox backpressure, not a
hold-duration test, hardware timer, keypad MMIO or I2C path. If the gate fires,
the emulator-owned lead is wrong RTOS queue consumption/scheduling, not GUI
contact lifetime. The trace dumps the complete `0x38`-byte process records for
HSC, EDP and HBAT along with the three computed counts, so the full mailbox and
task state is available at the failing comparison.

The same mailbox scan runs on the press path at `0x20140c1e/0x20140c3e` before
the already-proven `0x2203`. Since Up reaches `0x2203`, all three counts were
below their limits at that point. The expanded trace compares press and release
snapshots directly. However, the existing EDP callback trace already proves
that RTOS receive dequeued the Up `0x2203` before `0x20d75404`; receive decrements
the process mailbox count before dispatch. The processed press itself therefore
cannot remain as EDP's tenth message at release. The scan remains relevant for
new traffic or HSC/HBAT backpressure, but EDP `9 -> 10` is not the leading
explanation.
The same snapshot includes the HSC/EDP/HBAT entries from the RTOS process
directory at `0x3002eaac + process*8`, linking a full process mailbox to its
configured task group without another probe.
The immutable source table at `0x205eaa28 + process*8` establishes those groups
statically: HSC is group 2, EDP is group 4, and HBAT is group 1. Thus a full EDP
mailbox points to task group 4 scheduling/consumption, not UI task 15. The trace
also dumps the current RTOS task record at each menu boundary.

Do not patch the firmware, force an MMI message, seed saved-message state or remap
`0x14`. Fix only the emulator-owned condition that the paired traces prove is
wrong.

## Physical-key normalization and live lookup

The inbound physical word is not used directly. `0x20140bb4` computes
`normalized = (phys & 0xff) | ((phys >> 3) & 0x1fe0)` and stores it at
`0x301ce2f4+4`; `0x2118be80` then searches the live record table at
`0x30031994` for a record whose first halfword equals that normalized value.
Checklist for the emulator mapping:

| key | physical (CLA) | normalized | emulator keypad.rs |
| --- | ---: | ---: | ---: |
| Up | `0x0108` | `0x28` | (8,1) -> `0x28` |
| Down | `0x1810` | `0x310` | (16,24) -> `0x310` |
| Left | `0x1114` | `0x234` | (20,17) -> `0x234` |
| Right | `0x1214` | `0x254` | (20,18) -> `0x254` |
| 1 | `0x0801` | `0x101` | (1,8) -> `0x101` |

Therefore the emulated physical mapping is correct; the firmware itself
normalizes it back. `keypad.rs` is not the source of the Up drop.

## Release dispatch is fully mapped; the remaining unknown is the active profile

HKBD message dispatch (two parallel arrays at `0x20a8e87c`, `msg -> handler`):

| msg | handler | role |
| ---: | ---: | --- |
| `0x2201` / `0x2209` | `0x20d92134` / `0x20d9221c` | configuration apply |
| `0x2208` | `0x20140bb4` | press processing (normalizes physical -> lookup) |
| `0x2207` | `0x20140e9c` | release gate (requires slot 2/4) |
| `0x2207` | `0x20989ebc` | periodic event class (auto-repeat family) |
| `0x220c` | `0x20989eb8` | slot-clear / state reset |

Both `0x2207` handlers are confirmed by the dispatch table; the release gate is
the only path that produces `0x2204` for direction keys, and it requires the
record's configuration slot to hold `2` or `4`. With the corrected physical
mapping (Up `0x0108` -> normalized `0x28`, slot 20; Down `0x1810` -> `0x310`,
slot 21) and the profile table above, no profile 2..20 leaves either slot in a
release-valid state except profile 18 (slot 20 = 2). Static analysis therefore
cannot explain a working Down on the current mapping; either the active menu
screen installs profile 18, or an external clear/rewrite sequence changes the
live slots. The runtime trace (already wired for `hkbd_record_*` +
`hkbd_config_*`) decides between those two at menu entry.

## Runtime confirmation (2026-11-03): config[20] = 3 rejects Up at the release gate

An updated RAM dump taken at the end of a `full-tfs-2k.onenand.bin` run
(the run stalls in the bootloader before reaching the menu) still populated the
initialized live keypad table. The relevant records are identical to the CLA
layout:

| key | record | slot | config[slot] in dump |
| --- | --- | ---: | ---: |
| Up | rec22 `0x0108` | 20 | 3 |
| Down | rec21 `0x1810` | 21 | 0 |
| Left | rec24 `0x1114` | 22 | 0 |
| Right | rec25 `0x1214` | 23 | 0 |

HKBD release gate `0x20140e9c` compares the selected configuration slot with
`2` and `4` and rejects everything else through `0x20140f1a`. A slot value of
`3` therefore makes Up's `0x2204`/`0x3b53` request impossible. This run also
reports profile byte `0x3040067c = 1`, matching the boot/default profile.
The same profile call sites (10/15/17) write slot 20 with value `3`, so a
default or boot profile that applies one of those leaves Up at `3`; only
profile 18 would leave it at the release-valid `2`. The remaining open item is
still which profile the genuine menu screen installs, and whether any profile
transition is lost between MMI and HKBD.

## Root cause and exact call path

The active menu uses profile 3 for ordinary (non-special) numeric screens:
`0x209adb30` selects the configuration, and when `[0x302e7503]==0` it calls
`lk2_set_keyboard(3)` (via stub `0x208a7f00`). Profile body 3 executes through
the fallback body `0x20157586`, which writes slot 20 (`config[20]`) with value
`3`, leaving slot 21 (Down) at `0`.

The two HKBD gates then both reject Up:

```text
press  (0x20140c68, type-0 records):  if config[slot] is 3 or 4 -> skip 0x2204
release(0x20140e9c, type-0 records):  if config[slot] is not 2 or 4 -> reject
```

Up: type=0, slot 20, config[20]=3 -> blocked on both press and release.
Down: type=0, slot 21, config[21]=0 -> passes the press gate (config 0 is
allowed there) and only the release gate matters, which is why Down navigates.
This exactly reproduces the reported "Up ignored, Down works" symptom.

Confirmed at runtime: the `full-tfs-2k` boot-ram dump shows `config[20] = 3`,
`config[21] = 0`, profile byte `0x3040067c = 1` (default), matching profile 3's
write of slot 20 = 3. The fix must make the menu apply a profile that leaves
`config[20]` at `2` (profile 18) without patching firmware or remapping events;
e.g. correct delivery/application of the profile transition between MMI and
HKBD so the active menu no longer lands on profile 3.

## Static slot analysis of the profile 2..20 switch

This section records what was learned without a build. The config builder at
`0x20e008e8` (reached through the ARM stub `0x2007f12c`) has now been decoded
with the `0x2201` consumer as ground truth:

```text
builder(0x20e008e8): payload[0] = value
                      payload[1] = slot count
                      payload[2..] = slots

consumer(0x20d9221c): for each (value, count, slot...) group:
                          if slot < 0x1b: 0x301ce2c8[slot] = value
```

That is the opposite order of the earlier "pair count at +9 / (slot,value)"
description. The EDP forwarding code matches the builder order in every call
site listed below: every profile call is
`(length, value, count, slot0, slot1, ...)`.

The 27 immutable key records at `0x201cbf94` are eight bytes each:
`physical(2), type(2), slot(2), key(2)`. Relevant direction records:

| key | physical | type | slot | record address |
| --- | ---: | ---: | ---: | ---: |
| Up `0x14` | `0x0108` | `0` | `20` | `0x201cc044` |
| Down `0x15` | `0x1810` | `0` | `21` | `0x201cc03c` |
| Left `0x16` | `0x1114` | `0` | `22` | `0x201cc054` |
| Right `0x17` | `0x1214` | `0` | `23` | `0x201cc05c` |

The `0x2204` release gate at `0x20140e9c` allows the event only when the
record's type is `3` and slot `3` is `2/4`, or the type is `0` and
`0x301ce2c8[record.slot]` is `2/4`. Everything else exits through
`0x20140f1a` without requesting `0x2204`.

The static bodies of profiles 2..20 (from `lk2_set_keyboard` at `0x20156e80`)
were decoded from the jump table at `0x20156f24`. Payloads below list
`(value, slots...)`; all bodies are linear and terminate at `0x2015741e` or
`0x201575ea`.

| profile | payload groups |
| ---: | --- |
| 2 | `0 -> 14,15,6,7,3,11,12`; `2 -> 4,5,18,19`; `3 -> 2` |
| 3 | `0 -> 14,15,6,7,3,11,12`; `2 -> 4,5,18,19`; `3 -> 2` |
| 4 | `1 -> 4,11,12`; `0 -> 18,19`; `3 -> 6,7` |
| 5 | `1 -> 5,11,12`; `3 -> 18,19` |
| 6 | `1 -> 4,11,12`; `0 -> 18,19`; `2 -> 4,5` |
| 7 | `2 -> 4,5,18,19` |
| 8 | `2 -> 4,5` |
| 9 | `2 -> 4,5,18,19` |
| 10 | `0 -> 14,15,6,7,5,11,12,18,19,3,13,4`; `3 -> 20` |
| 11 | `0 -> 14,15,6,7,3,11,12`; `2 -> 4,5,18,19`; `3 -> 2` |
| 12 | `0 -> 14,15,6,7,3,11,12`; `2 -> 6,7`; `3 -> 2` |
| 13 | `0 -> 14,15,6,7,3,11,12`; `2 -> 4,5`; `3 -> 2` |
| 14 | `0 -> 14,15,6,7`; `3 -> 4,5,6,7,18` |
| 15 | `0 -> 14,15,6,7`; `1 -> 18,19`; `2 -> 6,7`; `3 -> 20,2`; `1 -> 4,5` |
| 16 | `0 -> 14,15,6,7,3,11,12`; `4 -> 6,7`; `3 -> 2`; `0 -> 4,5` |
| 17 | same as 10 |
| 18 | `0 -> 14,15`; `2 -> 6,7,4,5,18,19,3,11,12,13,20`; `3 -> 2` |
| 19 | `0 -> 14,15,6,7,3,11,12`; `2 -> 18,19`; `3 -> 6,7` |
| 20 | separate large routine `0x2015b136`, no config-builder calls |

Only profile 18 writes slot 20 (Up) with the release-valid value `2`. No
profile writes slot 21 (Down) with `2/4`. Earlier "working Down" runs
predate the corrected physical-key map: they used the secondary record with
key `0x12` (slot 18, physical `0x0208`) which profile 2 configures, not the
cluster Down `0x15` (slot 21, physical `0x1810`). On the current physical
mapping every direction key on profiles 2..20 is therefore release-gated
unless the active profile is 18 (slot 20) or an external clear/rewrite
sequence changes `0x301ce2c8[20]/[21]`. The runtime trace reports the active
profile and the real `0x301ce2c8[20]/[21]` values at menu entry (already
sampled as `hkbd_config_index`/`hkbd_config_value`). If it shows profile
2/4/5 active, the emulator-owned failure is the missing/incorrect profile
transition; if it shows a profile-18-style config that is later cleared, the
failure is an HKBD slot-clear or a `0x2201` delivery/scheduling problem.

Additional static anchors found while testing that hypothesis:

- `0x20989e58` clears all 27 configuration slots (loop bound `0x1b` at
  `0x20989e72`) together with the small 3-byte profile vector. It is one of
  the HKBD "clear" operations and must be observed in the same trace window as
  the profile writes to decide ordering.
- `0x20989f0c` sends `0x220d` with `0x43c`; `0x20d9217e` sends the
  `0x00222206/0x22220d` pair from the `0x2206`-class handler. These are the
  enable/disable events that frame profile changes; the trace already includes
  both message ids.
- The HKBD message table at `0x20a8e880` is not a flat `[id,handler]` table as
  first read; its first four bytes are a function pointer followed by message
  ids. Do not rely on any single "0x2207 -> release" claim from it; the only
  hard facts are the two consumer functions `0x20d92134`/`0x20d9221c`, which
  are confirmed by direct `0x2201` handling code.

The release gate, mailbox backpressure scan and the profile/slot question are
now all statically mapped; the single remaining unknown is runtime state.
