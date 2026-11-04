use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use crate::cpu::{EngineProt as Prot, HookType, RegisterARM, Unicorn, Arm946Cpu};
use crate::cpu::{arm_condition_passed, TcmRegisters, E830_DTCM_BASE, E830_TCM_SIZE};
use thiserror::Error;

use crate::{
    FirmwareSet, E830_TFS_SOURCE_BASE, FLASH_BASE,
    RAM_BASE, RAM_SIZE,
};

const PAGE_SIZE: u64 = 0x1000;
const E830_TFS_SOURCE_MIN_SIZE: u64 = 0x0001_0000;
const LOW_VECTOR_BASE: u64 = 0x0000_0000;
const LOW_VECTOR_SIZE: u64 = 0x0001_0000;
const FLASH_ALIAS_BASE: u64 = 0x0001_0000;
const FLASH_ALIAS_OFFSET: usize = 0x0001_0000;
const E830_BOOT_ROM_MIRROR_BASE: u64 = 0xea00_0000;
// Auxiliary DSP data window; the physical size still needs verification.
const DSP_AUX_RAM_BASE: u64 = 0x6010_0000;
const DSP_AUX_RAM_SIZE: u64 = 0x0040_0000;
const DSP_BOOT_MMIO_SIZE: u64 = PAGE_SIZE;
// Shared DSP data/mailbox addresses used by the firmware (including
// 0x60000008 and 0x600007e0). No DSP ISA execution; opt-in RF HLE uses
// only its recognized request and result windows.
const DSP_SHARED_RAM_BASE: u64 = 0x6000_0000;
const DSP_SHARED_RAM_SIZE: u64 = 0x0001_0000;
const E830_DSP_STARTUP_MAGIC: u32 = 0x7073_2b02;
const ONENAND_BASE: u64 = 0x4800_0000;
const ONENAND_SIZE: u64 = 0x0002_0000;

const EXTERNAL_FLASH_WINDOW_BASE: u64 = 0x21c0_0000;
const EXTERNAL_FLASH_WINDOW_SIZE: u64 = 0x0040_0000;
const NSF_NOR_OFFSET: usize = 0x0002_0000;
const SOC_BOOT_REGISTER_BASE: u64 = 0x5000_0000;
const SOC_BOOT_REGISTER_SIZE: u64 = 0x0001_0000;

const SOC_MEMORY_CONTROL_BASE: u64 = 0x3200_0000;
const SOC_MEMORY_CONTROL_SIZE: u64 = 0x0001_0000;
const SOC_CONFIG_REGISTER_BASE: u64 = 0x5030_0000;
const SOC_CONFIG_REGISTER_SIZE: u64 = 0x0001_0000;
const SOC_SECONDARY_REGISTER_BASE: u64 = 0x5040_0000;
const SOC_SECONDARY_REGISTER_SIZE: u64 = 0x0001_0000;
const UART_BASES: [u32; 2] = [0x5040_3000, 0x5040_4000];
const DMA_BASE: u32 = 0x5010_0000;
const DMA_IRQ: u32 = 29;
// Original SL3 UART0 TX descriptor: byte RAM source increments, UART THR
// stays fixed, no linked list, terminal-count interrupt. Other modes remain
// unimplemented. Instruction-based transfer latency is provisional.
const DMA_UART0_TX_CONTROL: u32 = 0x8400_0000;
const DMA_UART0_TX_CONFIG: u32 = 0x8841;
// Captured at 0x214f994e: SIM request 9, hardware channel 5, two-byte
// RAM -> SIM +0x34 transfer. Same byte control, distinct request config.
const DMA_SIM_TX_CONFIG: u32 = 0xaa41;
const DMA_SIM_RX_CONFIG: u32 = 0xb013;
const DMA_BYTE_INSTRUCTIONS: u64 = 32;
const I2C_BASE: u32 = 0x5040_5000;
const I2C_IRQ: u32 = 28;
const SERIAL_BASE: u32 = 0x5040_8000;
const SECONDARY_TIMER_BASES: [u32; 2] = [0x5040_9000, 0x5040_a000];
const SECONDARY_TIMER_IRQS: [u32; 2] = [18, 17];
// Firmware delay APIs multiply milliseconds by 1000 before programming these
// free-running 16-bit compare timers. Until the peripheral clock is measured
// independently, use the same observed/provisional ARM ratio as SOC_TIMER.
const SECONDARY_TIMER_CPU_CLOCK_RATIO: u64 = 8;
const SECONDARY_TIMER_COMPARE_OFFSETS: [u32; 4] = [0x0c, 0x10, 0x14, 0x18];
// HKBD copies profile configuration to this array. Slot 20 belongs to Up and
// profile 1 (the menu fallback profile) writes the blocking value 3 there.
const HKBD_CONFIG_BASE: u64 = 0x301c_e2c8;
const HKBD_UP_CONFIG_SLOT: u64 = 20;
// Press and release gates accept this value, so HKBD itself sends 0x2204.
const HKBD_UP_CONFIG_BLOCKED: u8 = 3;
const HKBD_UP_CONFIG_ALLOWED: u8 = 0;
const SECONDARY_TIMER_CONTROL_BITS: [u32; 4] = [0x08, 0x20, 0x80, 0x200];
const SECONDARY_TIMER_STATUS_BITS: [u32; 4] = [0x02, 0x04, 0x08, 0x10];
const SYSTEM_TIMER_IRQ: u32 = 20;
const SYSTEM_TIMER_MATCH_IRQ: u32 = 21;
const SYSTEM_TIMER_CPU_CLOCK_RATIO: u64 = 8;
// Timer API 0x2051e9e0 reads the integer counter as raw >> 2, while
// 0x2051eb30/46/5c use 14-bit period/compare fields. The counter retains
// two fractional bits; compare values are not raw 16-bit counter values.
const SYSTEM_TIMER_COMPARE_MASK: u16 = 0x3fff;
const SYSTEM_TIMER_FRACTION_BITS: u32 = 2;
// Timer API commands at 0x2051e8dc/0x2051ebb4 independently set and clear
// these match-channel bits. Firmware uses period +0x0c = 5000, match0 +0x10 =
// 0 and match1 +0x14 = 4700.
const SYSTEM_TIMER_MATCH0_ENABLE: u32 = 0x200;
const SYSTEM_TIMER_MATCH1_ENABLE: u32 = 0x400;
const SYSTEM_TIMER_MATCH_CHANNELS: u32 =
    SYSTEM_TIMER_MATCH0_ENABLE | SYSTEM_TIMER_MATCH1_ENABLE;
const SYSTEM_TIMER_IRQ20_ENABLE: u32 = 0x20;
const SYSTEM_TIMER_IRQ21_ENABLE: u32 = 0x40;
const SYSTEM_TIMER_IRQ20_STATUS: u32 = 0x80;
const SYSTEM_TIMER_IRQ21_STATUS: u32 = 0x100;
const SYSTEM_TIMER_STATUS_MASK: u32 =
    SYSTEM_TIMER_IRQ20_STATUS | SYSTEM_TIMER_IRQ21_STATUS;
const SOC_POWER_CONTROL_BASE: u64 = 0x5010_0000;
const SOC_POWER_CONTROL_SIZE: u64 = 0x0001_0000;
const SOC_REGISTER_BASE: u64 = 0x5050_0000;
const SOC_REGISTER_SIZE: u64 = 0x0001_0000;
const SOC_EXTERNAL_CONTROL_BASE: u64 = 0x4400_0000;
const SOC_EXTERNAL_CONTROL_SIZE: u64 = 0x0001_0000;
pub const LCD_WIDTH: usize = 176;
pub const LCD_HEIGHT: usize = 220;
const LCD_PIXEL_COUNT: usize = LCD_WIDTH * LCD_HEIGHT;
const LCD_GRAM_REGISTER: u16 = 0x22;
const SOC_TIMER: u32 = 0x5050_1000;
const SYSTEM_TIMER_BOOKKEEPING_START: u64 = 0x3001_a4f4;
const SYSTEM_TIMER_BOOKKEEPING_END: u64 = 0x3001_a4f7;
const RTOS_GLOBALS_TRACE_START: u64 = 0x3090_0160;
const RTOS_GLOBALS_TRACE_END: u64 = 0x3090_01bf;
const RTOS_NESTING_DEPTH: u64 = 0x3090_0169;
const DSP_CONTROL_BASE: u64 = 0x6010_0000;
const DSP_MAILBOX_BASE: u64 = 0x6000_0640;
const DSP_SIGNAL_BASE: u32 = 0x5050_5000;
const DSP_START_SIGNAL: u32 = DSP_SIGNAL_BASE + 0x10;
const DSP_RUNTIME_SIGNAL: u32 = DSP_SIGNAL_BASE + 0x18;
const DSP_RESULT_SIGNAL: u32 = DSP_SIGNAL_BASE + 0x1c;
const E830_DSP_SPACE0_WORDS: usize = 0x6000;
const E830_DSP_SPACE1_START: u16 = 0x7000;
const E830_DSP_SPACE1_WORDS: usize = 0x1000;
const E830_DSP_SPACE0_FNV64: u64 = 0xb25c_3385_c96d_670d;
const E830_DSP_SPACE1_FNV64: u64 = 0xe39e_195e_7cdc_9922;
const DSP_ASYNC_QUEUE: u32 = 0x30a2_c354;
const DSP_ASYNC_CALLBACK: u32 = 0x215a_7cb9;
const DSP_ASYNC_EVENT_HANDLER_PC: u64 = 0x204d_0388;
const DSP_ASYNC_COMPLETE_DELAY: u64 = 4096;
const DSP_SERIAL_BASE: u32 = 0x5030_1000;
const DSP_SERIAL_IRQ: u32 = 9;
// The original start handler enables bit 0 for the 32-bit TX FIFO at +0x100
// and bit 1 for the 16-bit TX FIFO at +0x200. Packed transfers (+0x0f=1)
// use control 2 << entry[+4], observed as bit 2 for a one-word transfer.
// All three observed modes dispatch through the same original IRQ9 ISR.
const DSP_SERIAL_TX_ENABLE: u32 = 7;
const DSP_SERIAL_TX_COMPLETE: u32 = 4;
const DSP_SERIAL_STATUS_MASK: u32 = 0x3fc;
const DSP_TRACE_PCS: [u64; 10] = [
    0x2098_c180,
    0x2098_c188,
    0x2098_c214,
    0x2098_c220,
    0x2118_f212,
    0x2118_f21e,
    0x2118_f252,
    0x2118_f260,
    0x2118_f294,
    0x2118_f2a2,
];
const RTOS_GLOBALS_TRACE_TARGETS: [(u64, u64); 4] = [
    (0x3090_0170, 0x3090_0173),
    (0x3090_018c, 0x3090_018f),
    (0x3090_01a0, 0x3090_01a3),
    (0x3090_01b8, 0x3090_01bb),
];
const SOC_INIT_CONTROL_REGISTER: u32 = 0x5030_2008;
const SOC_INIT_STATUS_REGISTER: u32 = 0x5030_2004;
const SOC_BOOT_READY_INPUT_REGISTER: u32 = 0x5030_3050;
const SOC_BOOT_READY_INPUT_MASK: u32 = 1 << 9;
const GPIO_BASE: u32 = 0x5030_3000;
const PMIC_GPIO_PIN: u32 = 1 << 3;
const SOC_KEYPAD_INPUT_REGISTERS: [u32; 8] = [
    0x5050_4008,
    0x5050_400c,
    0x5050_4010,
    0x5050_4018,
    0x5050_401c,
    0x5050_4020,
    0x5050_4024,
    0x5050_4028,
];
const SOC_KEYPAD_SCAN_READ_PCS: [u32; 2] = [0x208d_aa30, 0x208d_aa5c];
const RESET_CPSR: u64 = 0x0000_00d3;
const INITIAL_STACK: u64 = 0x31f4_8f74;
const MAX_RECENT_MMIO: usize = 64;
// DEF state names identify dispatch tables, not executable entry points.
const KEYBOARD_INIT_TABLE: u32 = 0x20e8_2eac;
const STANDBY_LOOK_TABLE: u32 = 0x2171_7f98;
const STATE_TABLE_DISPATCH_PC: u64 = 0x2084_78f2;
const MENU_INPUT_KEY_BLOCK_CANDIDATE: u64 = 0x3048_c264;
const MENU_INPUT_TRACE_PCS: [u64; 66] = [
    0x215a_6236, // HKBD sends MOBI_HKBD_KEY_PRESSED_IND to EDP.
    0x20d7_5404, // EDP key-press pipe entry.
    0x20d7_547c, // EDP direct APPI_EDP_KEY_PRESSED_IND route.
    0x20d7_54da, // EDP alternate/editor route.
    0x20d7_6304, // EDP keyboard-event diffusion entry.
    0x2055_9318, // edp2_51NoDiffAppiMessages cross-build candidate.
    0x2055_933a, // The no-diff predicate returns true.
    0x2055_933e, // The no-diff predicate returns false.
    0x20d7_631e, // Diffusion has the no-diff predicate result in r0.
    0x20d7_632a, // Proven early free/return gate inside diffusion.
    0x20d7_6334, // No-diff special route when the key-block gate is clear.
    0x20d7_634e, // Diffusion continues toward the MMI recipient.
    0x2015_6b34, // a_lk_StandbyLook wildcard callback.
    0x2119_fe50, // lk1_1rout_fsm equivalent entered through the standby callback.
    0x2015_6e80, // lk2_set_keyboard equivalent; requested profile is in r0.
    0x2015_6eda, // First APPI_EDP_CHANGE_KBD_CONFIG_REQ send site.
    0x2015_6f72, // Per-profile APPI_EDP_CHANGE_KBD_CONFIG_REQ send site.
    0x2015_741a, // Late per-profile APPI_EDP_CHANGE_KBD_CONFIG_REQ send site.
    0x2015_708c, // Keyboard profile 4 branch entry.
    0x2015_70e6, // Keyboard profile 5 branch entry.
    0x215a_5fc0, // HKBD key-record lookup over the RAM table at 0x30031994.
    0x2158_2028, // EDP receives APPI_EDP_CHANGE_KBD_CONFIG_REQ.
    0x2158_2042, // EDP forwards MOBI_HKBD_CONFIG_REQ to HKBD.
    0x20d9_221c, // HKBD receives MOBI_HKBD_CONFIG_REQ and begins applying slots.
    0x20d9_225a, // HKBD has applied every configuration slot in the request.
    0x2014_0c1e, // HKBD compares one mailbox count before sending 0x2203.
    0x2014_0c3e, // HKBD decides whether all press-path mailbox checks passed.
    0x2014_0e9c, // HKBD release/short processing that can emit 0x2204.
    0x2014_0ea6, // HKBD compares the active key-record type.
    0x2014_0ebe, // HKBD compares the selected configuration-slot value.
    0x2014_0ec4, // Active key record/configuration is eligible for 0x2204.
    0x2014_0edc, // HKBD compares one mailbox count with its configured maximum.
    0x2014_0ef0, // HKBD decides whether all three mailbox checks passed.
    0x2014_0efe, // HKBD emits MOBI_HKBD_KEY_INFO_IND.
    0x2014_0f1a, // Active key record/configuration rejected the 0x2204 path.
    0x215a_6266, // Generic HKBD sender queues the completed 0x2204 payload to EDP.
    0x2055_79b8, // EDP consumes MOBI_HKBD_KEY_INFO_IND.
    0x2055_85e4, // EDP builds APPI_EDP_KEY_INFO_IND.
    0x2055_864c, // EDP forwards APPI_EDP_KEY_INFO_IND to MMI.
    0x211a_08b6, // MMI has loaded the APPI_EDP_KEY_PRESSED_IND payload.
    0x211a_27e6, // MMI direction-key branch inside KEY_PRESSED handling.
    0x211a_57c0, // lk2_33key_in_filter equivalent entry.
    0x211a_593a, // Up (0x14) filter branch.
    0x211a_599e, // Keyboard filter result in r0.
    0x200d_4cc4, // MCC_GETKEY_ACCEPT_REJECT installs masks/profile.
    0x2111_2a6c, // MCC_GETKEY_S installs mask A from its active descriptor.
    0x2153_656c, // MCC_GETKEY_INCALL installs mask A from its active descriptor.
    0x200d_4d94, // MCC_GETKEY_ACCEPT_REJECT_S calls the key filter.
    0x200d_4d9c, // MCC_GETKEY_ACCEPT_REJECT_S tests the result.
    0x200d_4dcc, // MCC_GETKEY_ACCEPT_REJECT_S rejected-key path.
    0x2111_2b84, // MCC_GETKEY_S calls the shared key filter.
    0x2111_2b8c, // MCC_GETKEY_S tests the filter result.
    0x2153_66a0, // MCC_GETKEY_INCALL_S calls the shared key filter.
    0x2153_66a8, // MCC_GETKEY_INCALL_S tests the filter result.
    0x2111_2b96, // MCC_GETKEY_S forwards an accepted key to UI dispatch.
    0x215b_8a30, // UI dispatch selects the Up (0x14) command.
    0x2159_dd48, // Common UI command dispatcher entry; command in r0.
    0x2159_dd60, // Common UI command dispatcher early return.
    0x2159_de34, // Common UI command dispatcher invokes the selected action.
    0x2159_defe, // Alternate common UI action invocation.
    0x2098_9e58, // HKBD clear: zeroes all 27 config slots (loop bound 0x1b).
    0x208e_51ac, // IRQ16 keypad handler entry; release source events arrive here.
    0x208a_74d0, // keypad callback post-release: physical-to-record resolve.
    0x208a_703c, // keypad callback post-release: sends resolved key to HKBD.
    0x2014_0c68, // HKBD press gate: reads active key record and config slot.
    0x2014_0d54, // HKBD release/short alternate handler (sends 0x2204/0x220e class).
];
const RTOS_TASK_TRACE_PCS: [u64; 9] = [
    0x2084_7660, // receive: task group in r7, request in r5
    0x2084_7686, // event-mask wait has been stored in current task
    0x2084_76c4, // empty message queue wait
    0x2084_7724, // receive result: message in r7
    0x2084_7d14, // semaphore decrement: object in r4
    0x204b_01d8, // context switch: next task in r1
    0x2084_7a02, // message send: target task in r5, queue in r6
    0x2084_78f2, // state-table callback: table at [r5+4], entry in r4
    0x2084_7826, // direct callback: process slot in r7
];

fn state_dispatch_kind(pc: u64, table: u32) -> (bool, bool) {
    (
        pc == STATE_TABLE_DISPATCH_PC && table == KEYBOARD_INIT_TABLE,
        pc == STATE_TABLE_DISPATCH_PC && table == STANDBY_LOOK_TABLE,
    )
}

// Keep diagnostics away from MMIO: even a read may consume peripheral data.
fn diagnostic_ram_read(uc: &Unicorn<'_, E830State>, address: u64, size: usize) -> Option<Vec<u8>> {
    let end = address.checked_add(size as u64)?;
    if address < u64::from(RAM_BASE) || end > u64::from(RAM_BASE) + RAM_SIZE as u64 {
        return None;
    }
    uc.mem_read_as_vec(address, size).ok()
}

// Static MCFN descriptors live in the immutable CLA mapping, while their
// command objects live in RAM. Keep this helper away from external flash and
// every MMIO window; an unsuccessful read simply produces no diagnostic data.
fn diagnostic_code_or_ram_read(
    uc: &Unicorn<'_, E830State>,
    address: u64,
    size: usize,
) -> Option<Vec<u8>> {
    let end = address.checked_add(size as u64)?;
    let in_ram = address >= u64::from(RAM_BASE)
        && end <= u64::from(RAM_BASE) + RAM_SIZE as u64;
    let in_cla = address >= u64::from(FLASH_BASE) && end <= EXTERNAL_FLASH_WINDOW_BASE;
    (in_ram || in_cla).then(|| uc.mem_read_as_vec(address, size).ok()).flatten()
}

fn diagnostic_ram_u32(uc: &Unicorn<'_, E830State>, address: u64) -> Option<u32> {
    diagnostic_ram_read(uc, address, 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
}

fn diagnostic_ram_u8(uc: &Unicorn<'_, E830State>, address: u64) -> Option<u8> {
    diagnostic_ram_read(uc, address, 1).and_then(|bytes| bytes.first().copied())
}

fn diagnostic_ram_u16(uc: &Unicorn<'_, E830State>, address: u64) -> Option<u16> {
    diagnostic_ram_read(uc, address, 2)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
}

/// Emulator-side workaround for the E830 menu keyboard profile.
///
/// Profile 1, selected for menu-like UI states, writes the blocking value `3`
/// to HKBD's configuration slot 20. The native press/release gates then reject
/// Up before it can produce `MOBI_HKBD_KEY_INFO_IND` (`0x2204`). After HKBD
/// finishes applying a profile, replace only that known-bad value with the
/// value used by working navigation keys. The firmware path and all other
/// slots are left untouched.
fn hkbd_hle_unblock_up(uc: &mut Unicorn<'_, E830State>) {
    let address = HKBD_CONFIG_BASE + HKBD_UP_CONFIG_SLOT;
    let Some(value) = diagnostic_ram_u8(uc, address) else {
        return;
    };
    if value != HKBD_UP_CONFIG_BLOCKED {
        return;
    }
    if uc.mem_write(address, &[HKBD_UP_CONFIG_ALLOWED]).is_ok() {
        uc.get_data_mut().hkbd_up_unblocked += 1;
        eprintln!(
            "[e830-hkbd-hle] up slot={} blocked={} -> allowed={} count={}",
            HKBD_UP_CONFIG_SLOT, value, HKBD_UP_CONFIG_ALLOWED,
            uc.get_data().hkbd_up_unblocked
        );
    }
}

fn menu_input_stage(address: u64) -> &'static str {
    match address {
        0x215a_6236 => "hkbd-to-edp",
        0x20d7_5404 => "edp-pipe-entry",
        0x20d7_547c => "edp-direct-route",
        0x20d7_54da => "edp-alternate-route",
        0x20d7_6304 => "edp-diffuse-entry",
        0x2055_9318 => "edp-no-diff-predicate",
        0x2055_933a => "edp-no-diff-true",
        0x2055_933e => "edp-no-diff-false",
        0x20d7_631e => "edp-diffuse-predicate-result",
        0x20d7_632a => "edp-diffuse-drop",
        0x20d7_6334 => "edp-diffuse-special-route",
        0x20d7_634e => "edp-diffuse-forward",
        0x2015_6b34 => "mmi-standby-entry",
        0x2119_fe50 => "mmi-root-router",
        0x2015_6e80 => "mmi-set-keyboard",
        0x2015_6eda => "mmi-kbd-config-send-initial",
        0x2015_6f72 => "mmi-kbd-config-send-profile",
        0x2015_741a => "mmi-kbd-config-send-late-profile",
        0x2015_708c => "mmi-keyboard-profile-4",
        0x2015_70e6 => "mmi-keyboard-profile-5",
        0x215a_5fc0 => "hkbd-key-record-lookup",
        0x2158_2028 => "edp-kbd-config-receive",
        0x2158_2042 => "edp-to-hkbd-config",
        0x20d9_221c => "hkbd-config-receive",
        0x20d9_225a => "hkbd-config-applied",
        0x2014_0c1e => "hkbd-press-mailbox-count-compare",
        0x2014_0c3e => "hkbd-press-mailbox-count-decision",
        0x2014_0e9c => "hkbd-key-info-processing",
        0x2014_0ea6 => "hkbd-key-info-record-type",
        0x2014_0ebe => "hkbd-key-info-config-value",
        0x2014_0ec4 => "hkbd-key-info-config-accepted",
        0x2014_0edc => "hkbd-mailbox-count-compare",
        0x2014_0ef0 => "hkbd-mailbox-count-decision",
        0x2014_0efe => "hkbd-key-info-send",
        0x2014_0f1a => "hkbd-key-info-config-rejected",
        0x215a_6266 => "hkbd-key-info-to-edp",
        0x2055_79b8 => "edp-key-info-entry",
        0x2055_85e4 => "edp-key-info-build",
        0x2055_864c => "edp-key-info-forward",
        0x211a_08b6 => "mmi-key-pressed-consumer",
        0x211a_27e6 => "mmi-key-pressed-direction",
        0x211a_57c0 => "mmi-key-filter-entry",
        0x211a_593a => "mmi-key-filter-up",
        0x211a_599e => "mmi-key-filter-result",
        0x200d_4cc4 => "mcc-getkey-accept-reject-config",
        0x2111_2a6c => "mcc-getkey-s-config",
        0x2153_656c => "mcc-getkey-incall-config",
        0x200d_4d94 => "mcc-getkey-accept-reject-s-filter-call",
        0x200d_4d9c => "mcc-getkey-accept-reject-s-filter-decision",
        0x200d_4dcc => "mcc-getkey-accept-reject-s-rejected",
        0x2111_2b84 => "mcc-getkey-s-filter-call",
        0x2111_2b8c => "mcc-getkey-s-filter-decision",
        0x2153_66a0 => "mcc-getkey-incall-s-filter-call",
        0x2153_66a8 => "mcc-getkey-incall-s-filter-decision",
        0x2111_2b96 => "mcc-getkey-s-ui-dispatch",
        0x215b_8a30 => "ui-dispatch-up",
        0x2159_dd48 => "ui-command-entry",
        0x2159_dd60 => "ui-command-early-return",
        0x2159_de34 => "ui-command-action",
        0x2159_defe => "ui-command-action-alternate",
        0x2098_9e58 => "hkbd-clear-all-config",
        0x208e_51ac => "keypad-irq16-entry",
        0x208a_74d0 => "keypad-release-resolve",
        0x208a_703c => "keypad-release-send",
        0x2014_0c68 => "hkbd-press-gate-record",
        0x2014_0d54 => "hkbd-release-short-alternate",
        _ => "unknown",
    }
}

fn diagnostic_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn key_filter_caller(lr: u32) -> &'static str {
    match lr & !1 {
        0x200d_4d98 => "mcc-getkey-accept-reject-s",
        0x2111_2b88 => "mcc-getkey-s",
        0x2153_66a4 => "mcc-getkey-incall-s",
        _ => "other",
    }
}

fn key_filter_context(lr: u32, r4: u32, r5: u32, r6: u32) -> u32 {
    match lr & !1 {
        0x200d_4d98 => r6,
        0x2111_2b88 => r4,
        0x2153_66a4 => r5,
        _ => 0,
    }
}

fn trace_menu_input_stage(uc: &mut Unicorn<'_, E830State>, address: u64, total: u64) {
    let enabled = {
        let state = uc.get_data();
        state.trace_menu_input
            && total >= state.trace_menu_input_after
            && state.trace_menu_input_remaining > 0
            && MENU_INPUT_TRACE_PCS.contains(&address)
    };
    if !enabled { return; }

    let r0 = uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32;
    let r1 = uc.reg_read(RegisterARM::R1).unwrap_or(0) as u32;
    let r2 = uc.reg_read(RegisterARM::R2).unwrap_or(0) as u32;
    let r3 = uc.reg_read(RegisterARM::R3).unwrap_or(0) as u32;
    let r4 = uc.reg_read(RegisterARM::R4).unwrap_or(0) as u32;
    let r5 = uc.reg_read(RegisterARM::R5).unwrap_or(0) as u32;
    let r6 = uc.reg_read(RegisterARM::R6).unwrap_or(0) as u32;
    let sp = uc.reg_read(RegisterARM::SP).unwrap_or(0) as u32;
    let lr = uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;

    let object = match address {
        0x215a_6236 | 0x215a_6266 => r3,
        0x20d7_5404 | 0x2015_6b34 => r0,
        0x20d7_547c | 0x20d7_54da | 0x20d7_631e | 0x20d7_632a
        | 0x20d7_6334 | 0x20d7_634e => r5,
        0x20d7_6304 => r1,
        0x2015_6eda | 0x2015_6f72 | 0x2015_741a | 0x2158_2042 => r3,
        0x2015_708c | 0x2015_70e6 | 0x215a_5fc0 => r0,
        0x2158_2028 | 0x20d9_221c => r0,
        0x20d9_225a => r5,
        0x2055_79b8 | 0x2055_85e4 => r0,
        0x2055_864c => r1,
        0x211a_08b6 | 0x211a_27e6 => r4,
        0x211a_57c0 => key_filter_context(lr, r4, r5, r6),
        0x200d_4cc4 | 0x2111_2a6c | 0x2153_656c => r0,
        _ => 0,
    };
    let object_bytes = if object == 0 {
        "-".into()
    } else {
        diagnostic_ram_read(uc, u64::from(object), 0x1c)
            .map(|bytes| diagnostic_hex(&bytes))
            .unwrap_or_else(|| "unreadable".into())
    };
    let indirect = if matches!(address, 0x20d7_5404 | 0x2015_6b34 | 0x2055_79b8 | 0x2055_85e4 | 0x200d_4cc4 | 0x2111_2a6c | 0x2153_656c | 0x20d9_221c) {
        diagnostic_ram_u32(uc, u64::from(object))
            .and_then(|pointer| diagnostic_code_or_ram_read(uc, u64::from(pointer), 0x20)
                .map(|bytes| format!("{pointer:08x}:{}", diagnostic_hex(&bytes))))
            .unwrap_or_else(|| "-".into())
    } else {
        "-".into()
    };
    let keyboard_profile = diagnostic_ram_u8(uc, 0x3040_067c).unwrap_or(u8::MAX);
    let pending_message = diagnostic_ram_u32(uc, 0x3001_4df4).unwrap_or(u32::MAX);
    let filter_key = match address {
        0x211a_57c0 | 0x200d_4d94 | 0x2111_2b84 | 0x2153_66a0 => r0,
        0x211a_593a => 0x14,
        _ => u32::MAX,
    };
    let filter_key_info = if matches!(
        address,
        0x211a_57c0 | 0x211a_593a | 0x211a_599e
            | 0x200d_4d94 | 0x2111_2b84 | 0x2153_66a0
    ) {
        r1
    } else {
        u32::MAX
    };
    let (filter_mask_a, filter_mask_b, filter_key_offset) = match address {
        0x211a_57c0 | 0x200d_4d94 | 0x2111_2b84 | 0x2153_66a0 => (
            r2,
            r3,
            diagnostic_ram_u32(uc, u64::from(sp)).unwrap_or(u32::MAX),
        ),
        0x211a_593a | 0x211a_599e => (r2, r6, r5),
        _ => (u32::MAX, u32::MAX, u32::MAX),
    };
    let filter_result = if matches!(address, 0x211a_599e | 0x200d_4d9c | 0x2111_2b8c | 0x2153_66a8) {
        r0
    } else {
        u32::MAX
    };
    let up_allowed_a = if filter_mask_a != u32::MAX {
        (filter_mask_a >> 17) & 1
    } else {
        u32::MAX
    };
    let up_allowed_b = if filter_mask_b != u32::MAX {
        (filter_mask_b >> 17) & 1
    } else {
        u32::MAX
    };
    let lk_state = diagnostic_ram_read(uc, 0x3036_c108, 0x20)
        .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
    let gate = diagnostic_ram_u8(uc, MENU_INPUT_KEY_BLOCK_CANDIDATE).unwrap_or(u8::MAX);
    let gate_context = diagnostic_ram_read(uc, 0x3048_5708, 4)
        .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
    let editor_mode = diagnostic_ram_u8(uc, 0x302e_7540).unwrap_or(u8::MAX);
    let editor_gate = diagnostic_ram_u8(uc, 0x302e_74e8).unwrap_or(u8::MAX);
    let pipe_gate = diagnostic_ram_u8(uc, 0x3001_4df1).unwrap_or(u8::MAX);
    let hkbd_config = diagnostic_ram_read(uc, 0x301c_e2c8, 0x1b)
        .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
    let hkbd_active = diagnostic_ram_read(uc, 0x301c_e2f4, 0x10)
        .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
    let hkbd_key_record = diagnostic_ram_u32(uc, 0x301c_e2fc)
        .map(|record| {
            let bytes = diagnostic_ram_read(uc, u64::from(record), 8)
                .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
            format!("{record:08x}:{bytes}")
        }).unwrap_or_else(|| "?".into());
    let hkbd_record_type = diagnostic_ram_u32(uc, 0x301c_e2fc)
        .and_then(|record| diagnostic_ram_u16(uc, u64::from(record) + 2))
        .unwrap_or(u16::MAX);
    let hkbd_record_index = diagnostic_ram_u32(uc, 0x301c_e2fc)
        .and_then(|record| diagnostic_ram_u16(uc, u64::from(record) + 4))
        .unwrap_or(u16::MAX);
    let hkbd_record_key = diagnostic_ram_u32(uc, 0x301c_e2fc)
        .and_then(|record| diagnostic_ram_u16(uc, u64::from(record) + 6))
        .unwrap_or(u16::MAX);
    let hkbd_config_index = match hkbd_record_type {
        0 => hkbd_record_index,
        3 => 3,
        _ => u16::MAX,
    };
    let hkbd_config_value = if hkbd_config_index == u16::MAX {
        u8::MAX
    } else {
        diagnostic_ram_u8(uc, 0x301c_e2c8 + u64::from(hkbd_config_index))
            .unwrap_or(u8::MAX)
    };
    let hkbd_mailbox_processes = diagnostic_ram_read(uc, 0x301c_e2bd, 3)
        .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
    let hkbd_mailbox_counts = diagnostic_ram_read(uc, 0x301c_e2e6, 6)
        .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
    let hkbd_mailbox_limits = diagnostic_code_or_ram_read(uc, 0x20a8_e91c, 3)
        .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
    let hkbd_mailbox_records = diagnostic_ram_u32(uc, 0x3090_0190)
        .map(|base| {
            [0x23u32, 0x3b, 0x50].into_iter().map(|process| {
                let address = u64::from(base) + u64::from(process) * 0x38;
                let record = diagnostic_ram_read(uc, address, 0x38)
                    .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
                format!("{process:02x}@{address:08x}:{record}")
            }).collect::<Vec<_>>().join(",")
        }).unwrap_or_else(|| "?".into());
    let hkbd_process_directory = [0x23u64, 0x3b, 0x50].into_iter().map(|process| {
        let address = 0x3002_eaac + process * 8;
        let entry = diagnostic_ram_read(uc, address, 8)
            .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
        format!("{process:02x}@{address:08x}:{entry}")
    }).collect::<Vec<_>>().join(",");
    let current_rtos_task = diagnostic_ram_u32(uc, 0x3090_0194)
        .map(|task| {
            let record = diagnostic_ram_read(uc, u64::from(task), 0x38)
                .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into());
            format!("{task:08x}:{record}")
        }).unwrap_or_else(|| "?".into());
    let rejected_up_override = diagnostic_ram_u8(uc, 0x3040_13a7).unwrap_or(u8::MAX);
    let filter_caller = if matches!(address, 0x211a_57c0) {
        key_filter_caller(lr)
    } else {
        "-"
    };
    let mcc_base = if matches!(address, 0x200d_4cc4 | 0x2111_2a6c | 0x2153_656c) {
        r0
    } else if address == 0x211a_57c0 {
        key_filter_context(lr, r4, r5, r6)
    } else if matches!(address, 0x200d_4d94 | 0x200d_4d9c | 0x200d_4dcc) {
        r6
    } else if matches!(address, 0x2111_2b84 | 0x2111_2b8c) {
        r4
    } else if matches!(address, 0x2153_66a0 | 0x2153_66a8) {
        r5
    } else {
        0
    };
    let mcc_context = if mcc_base != 0 {
        diagnostic_ram_read(uc, u64::from(mcc_base) + 0x180, 0x40)
            .map(|bytes| diagnostic_hex(&bytes)).unwrap_or_else(|| "?".into())
    } else {
        "-".into()
    };
    let ui_command_gates = if matches!(address, 0x2159_dd48 | 0x2159_dd60 | 0x2159_de34 | 0x2159_defe) {
        [
            0x3022_18dc, 0x3022_19c7, 0x3045_0abf, 0x3022_3c0e,
            0x3022_3c19, 0x3022_3c21, 0x3040_1393, 0x3040_139d,
            0x3040_13b8, 0x302e_74f7,
        ].into_iter().map(|gate_address| {
            let value = diagnostic_ram_u8(uc, gate_address).unwrap_or(u8::MAX);
            format!("{gate_address:08x}:{value:02x}")
        }).collect::<Vec<_>>().join(",")
    } else {
        "-".into()
    };
    eprintln!(
        "[e830-menu-input] stage={} pc=0x{address:08x} insns={total} r0=0x{r0:08x} r1=0x{r1:08x} r2=0x{r2:08x} r3=0x{r3:08x} r4=0x{r4:08x} r5=0x{r5:08x} r6=0x{r6:08x} sp=0x{sp:08x} lr=0x{lr:08x} object=0x{object:08x}:{object_bytes} indirect={indirect} filter_caller={filter_caller} filter_key=0x{filter_key:08x} filter_key_info=0x{filter_key_info:08x} filter_mask_a=0x{filter_mask_a:08x} filter_mask_b=0x{filter_mask_b:08x} filter_key_offset=0x{filter_key_offset:08x} up_allowed_a=0x{up_allowed_a:08x} up_allowed_b=0x{up_allowed_b:08x} filter_result=0x{filter_result:08x} rejected_up_override=0x{rejected_up_override:02x} keyboard_profile=0x{keyboard_profile:02x} pending_message=0x{pending_message:08x} pipe_gate={pipe_gate:02x} editor_gate={editor_gate:02x} editor_mode={editor_mode:02x} key_block_candidate={gate:02x} gate_ctx={gate_context} hkbd_config={hkbd_config} hkbd_active={hkbd_active} hkbd_key_record={hkbd_key_record} hkbd_record_type=0x{hkbd_record_type:04x} hkbd_record_index=0x{hkbd_record_index:04x} hkbd_record_key=0x{hkbd_record_key:04x} hkbd_config_index=0x{hkbd_config_index:04x} hkbd_config_value=0x{hkbd_config_value:02x} hkbd_mailbox_processes={hkbd_mailbox_processes} hkbd_mailbox_counts={hkbd_mailbox_counts} hkbd_mailbox_limits={hkbd_mailbox_limits} hkbd_mailbox_records={hkbd_mailbox_records} hkbd_process_directory={hkbd_process_directory} current_rtos_task={current_rtos_task} mcc_context={mcc_context} ui_command_gates={ui_command_gates} lk={lk_state}",
        menu_input_stage(address),
    );
    uc.get_data_mut().trace_menu_input_remaining -= 1;
}

fn advance_dsp_rf(uc: &mut Unicorn<'_, E830State>) {
    if !uc.get_data().dsp_rf.busy() { return; }
    let state = uc.get_data_mut();
    let now = state.instructions.saturating_add(state.fast_forwarded_instructions);
    if !state.dsp_boot.runtime_hle_active() {
        state.dsp_rf.reset();
        return;
    }
    let results = state.dsp_rf.take_ready(now);
    for (slot, raw) in results {
        // Never touch ARM descriptors/RTOS state, or invoke a guest callback.
        // Publish indexed hardware payload before raising its W0C result bit.
        // The existing timer IRQ21 collector performs the guest-side delivery.
        if uc.mem_write(crate::dsp_rf::RESULT_BASE + u64::from(slot) * 2,
            &raw.to_le_bytes()).is_ok()
        {
            *uc.get_data_mut().registers.entry(DSP_RESULT_SIGNAL).or_default() |= 1 << slot;
            eprintln!("[e830-dsp-rf-hle] complete slot={slot} raw={raw} time={now}");
        }
    }
}

fn advance_dma(uc: &mut Unicorn<'_, E830State>) {
    let now = uc.get_data().instructions.saturating_add(uc.get_data().fast_forwarded_instructions);
    for channel in 0..8 {
        if !uc.get_data().dma_deadlines[channel].is_some_and(|deadline| now >= deadline) {
            continue;
        }
        uc.get_data_mut().dma_deadlines[channel] = None;
        let uart = uc.get_data().dma_uart0_tx(channel);
        let sim = uc.get_data().dma_sim_tx(channel);
        if let Some((destination, count)) = uc.get_data().dma_sim_rx(channel) {
            let Some(bytes) = uc.get_data_mut().sim.receive_dma(count, now) else {
                uc.get_data_mut().dma_deadlines[channel] = Some(now.saturating_add(1));
                continue;
            };
            if uc.mem_write(u64::from(destination), &bytes).is_err() { continue; }
            let state = uc.get_data_mut();
            let base = DMA_BASE + 0x100 + channel as u32 * 0x20;
            state.registers.insert(base + 4, destination + count as u32);
            *state.registers.entry(base + 0x0c).or_default() &= !0xfff;
            *state.registers.entry(base + 0x10).or_default() &= !1;
            *state.registers.entry(DMA_BASE + 0x14).or_default() |= 1 << channel;
            if state.trace_sim && now >= state.trace_sim_after && state.trace_sim_remaining != 0 {
                state.trace_sim_remaining -= 1;
                eprintln!("[e830-sim] dma-rx destination=0x{destination:08x} bytes={bytes:02x?} insns={now}");
            }
            continue;
        }
        let Some((source, count)) = uart.or(sim) else { continue; };
        // DMA reads ordinary RAM only; never consume an RX/MMIO register to
        // manufacture data. Invalid/unsupported descriptors get no completion.
        let Some(bytes) = diagnostic_ram_read(uc, u64::from(source), count) else { continue; };
        let state = uc.get_data_mut();
        if uart.is_some() {
            for byte in bytes { state.uarts[0].write(0, u32::from(byte)); }
        } else {
            if !state.sim.transmit_dma(&bytes, now) { continue; }
            if state.trace_sim && now >= state.trace_sim_after && state.trace_sim_remaining != 0 {
                state.trace_sim_remaining -= 1;
                eprintln!("[e830-sim] dma-tx source=0x{source:08x} bytes={bytes:02x?} insns={now}");
            }
        }
        let base = DMA_BASE + 0x100 + channel as u32 * 0x20;
        state.registers.insert(base, source + count as u32);
        *state.registers.entry(base + 0x0c).or_default() &= !0xfff;
        *state.registers.entry(base + 0x10).or_default() &= !1;
        *state.registers.entry(DMA_BASE + 0x14).or_default() |= 1 << channel;
        // IRQ29 owns all CPU/queue/callback changes, including SL3 ready flags.
    }
}

fn read_irq_nesting(uc: &Unicorn<'_, E830State>) -> u8 {
    // No heap allocation for a single diagnostic RAM byte.
    let mut byte = [u8::MAX];
    if uc.mem_read(RTOS_NESTING_DEPTH, &mut byte).is_ok() { byte[0] } else { u8::MAX }
}

fn dispatch_table(uc: &Unicorn<'_, E830State>) -> Option<u32> {
    let node = uc.reg_read(RegisterARM::R5).ok()?;
    let bytes = diagnostic_ram_read(uc, node.checked_add(4)?, 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

// Interrupt controller (base 0x50302000), derived from the firmware's ARM IRQ
// vector handler at 0x20cb0cd0: it loads r4 = 0x50302000, reads the pending
// line number as a byte at the base (already multiplied by four), filters it
// against the pending bitmask at +0x1c, dispatches through a RAM table, then
// writes 0x80 to the end-of-interrupt register at +0x08 to acknowledge.
const INTC_PENDING_NUMBER_REGISTER: u32 = 0x5030_2000;
const INTC_PENDING_MASK_REGISTER: u32 = 0x5030_201c;
const INTC_MASKED_PENDING_REGISTER: u32 = 0x5030_2014;
// Interrupt-line mask: a set bit masks the source. +0x14 reports unmasked
// pending sources; +0x1c reports the accepted source until controller EOI.
const INTC_ENABLE_MASK_REGISTER: u32 = 0x5030_2010;
// ARM exception vector for IRQ entry, resolved from the vector table literal at
// 0x20000164 (`ldr pc,[pc,#0x144]`). 
const IRQ_VECTOR_TARGET: u64 = 0x20cb_0cd0;
const CPSR_MODE_MASK: u32 = 0x1f;
const CPSR_MODE_IRQ: u32 = 0x12;
const CPSR_IRQ_DISABLE: u32 = 1 << 7;
const CPSR_THUMB: u32 = 1 << 5;
// Optional diagnostic IRQ generator, not a hardware timer. IRQ 29 is DMA;
// real peripheral interrupts are driven by their devices. Respect both masks.
const DEFAULT_TIMER_IRQ_INDEX: u32 = 29;
const DEFAULT_TIMER_IRQ_PERIOD: u64 = 0;
const WAKE_MMIO_START: u32 = 0x5050_0000;
const WAKE_MMIO_END: u32 = 0x5050_2018;
const WAKE_STATE_START: u64 = 0x304a_20d4;
const WAKE_STATE_END: u64 = 0x304a_2104;
const WAKE_STUB_START: u64 = 0x31f0_0000;
const WAKE_STUB_END: u64 = 0x31f0_01ab;
const WAKE_WAIT_TRACE_START: u64 = 0x2012_179c;
const WAKE_WAIT_TRACE_END: u64 = 0x2012_17d6;
const WAKE_IRQ_TRACE_PCS: [u64; 10] = [
    IRQ_VECTOR_TARGET,
    0x2012_1498,
    0x31f0_0140,
    0x208a_0fd8,
    0x2001_0bb2,
    0x2001_3d08,
    0x2001_0a40,
    0x2001_40cc,
    0x2001_07e8,
    0x20d7_29c0,
];
const TIMER_TRACE_PCS: [u64; 7] = [
    0x2001_172c,
    0x2001_17d4,
    0x200e_8cd0,
    0x200e_8d08,
    0x2013_e2e8,
    0x2084_8732,
    IRQ_VECTOR_TARGET,
];

fn env_u64(name: &str) -> Option<u64> {
    let raw = std::env::var(name).ok()?;
    let trimmed = raw.trim();
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse().ok()
    }
}

fn interrupt_delivery_allowed(cpsr: u32, in_service: bool) -> bool {
    cpsr & CPSR_IRQ_DISABLE == 0
        && cpsr & CPSR_MODE_MASK != CPSR_MODE_IRQ
        && !in_service
}

fn interrupt_service_returned(
    cpsr: u32,
    in_service: bool,
    eoi_seen: bool,
) -> bool {
    in_service
        && eoi_seen
        && cpsr & CPSR_IRQ_DISABLE == 0
        && cpsr & CPSR_MODE_MASK != CPSR_MODE_IRQ
}

fn periodic_timer_crossed(start: u16, ticks: u64, period: u16, target: u16) -> bool {
    if ticks == 0 || period == 0 || target >= period {
        return false;
    }
    let period = u64::from(period);
    let start = u64::from(start) % period;
    let target = u64::from(target);
    let distance = if target > start {
        target - start
    } else {
        period - start + target
    };
    ticks >= distance
}

#[derive(Debug, Error)]
pub enum MachineError {
    #[error("CLA image must contain more than 64 KiB for the E830 boot aliases")]
    ClaTooShortForBootMap,
    #[error("NSF payload exceeds the E830 NOR data area")]
    NsfTooLarge,
    #[error("OneNAND dump has {size} bytes; this 32 MiB profile requires 33554432 main bytes or 34603008 interleaved main+spare bytes (1024+32 per page)")]
    InvalidOneNandImage { size: usize },
    #[error("the instruction budget must be greater than zero")]
    ZeroInstructionBudget,
    #[error("guest function address or argument count is invalid")]
    InvalidGuestCall,
    #[error("installer buffer is outside SDRAM")]
    InvalidInstallerBuffer,
    #[error("Unicorn {operation} failed: {error}")]
    Unicorn {
        operation: &'static str,
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    InstructionBudget,
    Breakpoint,
    CpuError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFault {
    pub kind: String,
    pub address: u64,
    pub size: usize,
    pub pc: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmioAccess {
    pub write: bool,
    pub address: u32,
    pub size: usize,
    pub value: u64,
    pub pc: u32,
}

#[derive(Debug, Clone)]
pub struct BootReport {
    pub instructions: u64,
    pub fast_forwarded_instructions: u64,
    pub pc: u32,
    pub cpsr: u32,
    pub r0: u32,
    pub r1: u32,
    pub r2: u32,
    pub r3: u32,
    pub r4: u32,
    pub r5: u32,
    pub r6: u32,
    pub r7: u32,
    pub sp: u32,
    pub lr: u32,
    pub stop_reason: StopReason,
    pub stack_words: Vec<u32>,
    pub memory_fault: Option<MemoryFault>,
    pub recent_mmio: Vec<MmioAccess>,
    pub eeprom_size: usize,
    pub tfs_size: Option<usize>,
    pub nor_size: usize,
    pub onenand_image_size: Option<usize>,
    pub lcd_frames: u64,
    pub keyboard_init_dispatches: u64,
    pub standby_look_dispatches: u64,
    pub irqs_taken: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DspBootParser {
    Command,
    Running,
    RuntimeReadArguments {
        memory_space: u8,
        address: Option<u16>,
    },
    Arguments {
        command: u16,
        address: Option<u16>,
    },
    Upload {
        memory_space: u8,
        address: u16,
        remaining: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DspEvent {
    StartupReady,
    RuntimeReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DspHlePhase {
    Disabled,
    AwaitingStart,
    E830Running,
    E830RuntimeReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DspQueueHlePhase {
    Idle,
    CompletePending { deadline: u64 },
    CompleteSignaled,
}

#[derive(Debug)]
struct DspBootInterface {
    control: u32,
    last_tx: u16,
    auxiliary: u32,
    parser: DspBootParser,
    rx_fifo: VecDeque<u16>,
    memories: [HashMap<u16, u16>; 2],
    hle_phase: DspHlePhase,
    runtime_completion_pending: bool,
}

impl Default for DspBootInterface {
    fn default() -> Self {
        Self::new(false)
    }
}

impl DspBootInterface {
    fn new(hle_enabled: bool) -> Self {
        Self {
            control: 0,
            last_tx: 0,
            auxiliary: 0,
            parser: DspBootParser::Command,
            rx_fifo: VecDeque::new(),
            memories: Default::default(),
            hle_phase: if hle_enabled {
                DspHlePhase::AwaitingStart
            } else {
                DspHlePhase::Disabled
            },
            runtime_completion_pending: false,
        }
    }

    const TX_READY: u32 = 1 << 0;
    const RX_READY: u32 = 1 << 1;

    fn status(&self) -> u32 {
        (self.control & !(Self::TX_READY | Self::RX_READY))
            | Self::TX_READY
            | if self.rx_fifo.is_empty() { 0 } else { Self::RX_READY }
    }

    fn snapshot(&self) -> [u32; 4] {
        [
            self.status(),
            u32::from(self.rx_fifo.front().copied().unwrap_or(0)),
            u32::from(self.last_tx),
            self.auxiliary,
        ]
    }

    fn memory_snapshot(&self, memory_space: usize) -> Option<Vec<u8>> {
        let memory = self.memories.get(memory_space)?;
        let words = match memory_space {
            0 => 0x1_0000,
            1 => 0x1_0000,
            _ => return None,
        };
        let mut bytes = Vec::with_capacity(words * 2);
        for address in 0..words {
            bytes.extend_from_slice(&memory.get(&(address as u16)).copied().unwrap_or(0).to_le_bytes());
        }
        Some(bytes)
    }

    fn read(&mut self, address: u32, size: usize) -> u64 {
        let offset = address.wrapping_sub(DSP_CONTROL_BASE as u32);
        let aligned = offset & !3;
        let raw = match aligned {
            0 => self.status(),
            4 => u32::from(self.rx_fifo.pop_front().unwrap_or(0)),
            8 => u32::from(self.last_tx),
            12 => self.auxiliary,
            _ => 0,
        };
        let shift = (offset & 3) * 8;
        u64::from((raw >> shift) & width_mask(size))
    }

    fn write(&mut self, address: u32, size: usize, value: u64) -> Option<DspEvent> {
        let offset = address.wrapping_sub(DSP_CONTROL_BASE as u32);
        let aligned = offset & !3;
        if aligned == 8 {
            self.last_tx = value as u16;
            return self.transmit(self.last_tx);
        }

        let shift = (offset & 3) * 8;
        let value_mask = width_mask(size);
        let mask = value_mask.wrapping_shl(shift);
        let update = |register: &mut u32| {
            *register = (*register & !mask) | (((value as u32).wrapping_shl(shift)) & mask);
        };
        match aligned {
            0 => update(&mut self.control),
            12 => update(&mut self.auxiliary),
            _ => {}
        }
        None
    }

    fn transmit(&mut self, word: u16) -> Option<DspEvent> {
        if std::env::var_os("E830_TRACE_DSP_TX").is_some()
            && matches!(
                self.hle_phase,
                DspHlePhase::E830Running | DspHlePhase::E830RuntimeReady
            )
        {
            eprintln!(
                "[e830-dsp-tx] parser={:?} phase={:?} word=0x{word:04x}",
                self.parser, self.hle_phase
            );
        }
        match self.parser {
            DspBootParser::Command => match word {
                0x000b => self.rx_fifo.push_back(0x0305),
                0x0001 | 0x0002 | 0x0005 | 0x0006 | 0x0003 => {
                    self.parser = DspBootParser::Arguments {
                        command: word,
                        address: None,
                    };
                }
                // Runtime DSP commands are intentionally left for the runtime
                // mailbox model. TX remains ready, but boot ROM behavior is not
                // invented for commands not established by the loader.
                _ => {}
            },
            DspBootParser::Arguments { command: 0x0003, .. } => {
                // Start execution takes one address word and has no boot-ROM
                // response. DSP execution itself is not implemented here.
                self.parser = DspBootParser::Running;
                if self.hle_phase == DspHlePhase::AwaitingStart
                    && word == 0
                    && self.matches_e830_image()
                {
                    self.hle_phase = DspHlePhase::E830Running;
                    return Some(DspEvent::StartupReady);
                }
                self.hle_phase = DspHlePhase::Disabled;
            }
            DspBootParser::Running => {
                if matches!(self.hle_phase, DspHlePhase::E830Running | DspHlePhase::E830RuntimeReady) {
                    match word {
                        // The ARM continues using the boot interface's two
                        // established read commands after DSP start to verify
                        // and copy overlays. Return only bytes uploaded earlier.
                        0x0005 | 0x0006 => {
                            self.parser = DspBootParser::RuntimeReadArguments {
                                memory_space: if word == 0x0005 { 0 } else { 1 },
                                address: None,
                            };
                        }
                        0x1001 => {
                            // This command has two observed phases: the DSP
                            // accepts it, then raises the same signal again
                            // after the ARM acknowledges the first event.
                            self.runtime_completion_pending = true;
                            self.hle_phase = DspHlePhase::E830RuntimeReady;
                            return Some(DspEvent::RuntimeReady);
                        }
                        0x1002 | 0x2001 => {
                            // Retail call sites send these commands and wait
                            // for one DSP runtime event before returning.
                            // Without RD16024 execution, model only that
                            // observed completion notification.
                            self.hle_phase = DspHlePhase::E830RuntimeReady;
                            return Some(DspEvent::RuntimeReady);
                        }
                        0x4001 => {
                            self.hle_phase = DspHlePhase::E830RuntimeReady;
                            return Some(DspEvent::RuntimeReady);
                        }
                        _ => {}
                    }
                }
            }
            DspBootParser::RuntimeReadArguments { memory_space, address: None } => {
                self.parser = DspBootParser::RuntimeReadArguments {
                    memory_space,
                    address: Some(word),
                };
            }
            DspBootParser::RuntimeReadArguments { memory_space, address: Some(address) } => {
                for index in 0..word {
                    let location = address.wrapping_add(index);
                    self.rx_fifo.push_back(
                        self.memories[memory_space as usize]
                            .get(&location)
                            .copied()
                            .unwrap_or(0),
                    );
                }
                self.parser = DspBootParser::Running;
            }
            DspBootParser::Arguments { command, address: None } => {
                self.parser = DspBootParser::Arguments {
                    command,
                    address: Some(word),
                };
            }
            DspBootParser::Arguments {
                command,
                address: Some(address),
            } => {
                let memory_space = match command {
                    0x0001 | 0x0005 => 0,
                    0x0002 | 0x0006 => 1,
                    _ => {
                        self.parser = DspBootParser::Command;
                        return None;
                    }
                };
                if matches!(command, 0x0001 | 0x0002) {
                    self.parser = if word == 0 {
                        DspBootParser::Command
                    } else {
                        DspBootParser::Upload {
                            memory_space,
                            address,
                            remaining: word,
                        }
                    };
                } else {
                    for index in 0..word {
                        let location = address.wrapping_add(index);
                        self.rx_fifo.push_back(
                            self.memories[memory_space as usize]
                                .get(&location)
                                .copied()
                                .unwrap_or(0),
                        );
                    }
                    self.parser = DspBootParser::Command;
                }
            }
            DspBootParser::Upload {
                memory_space,
                address,
                remaining,
            } => {
                self.memories[memory_space as usize].insert(address, word);
                self.parser = if remaining == 1 {
                    DspBootParser::Command
                } else {
                    DspBootParser::Upload {
                        memory_space,
                        address: address.wrapping_add(1),
                        remaining: remaining - 1,
                    }
                };
            }
        }
        None
    }

    fn matches_e830_image(&self) -> bool {
        self.memories[0].len() == E830_DSP_SPACE0_WORDS
            && self.memories[1].len() == E830_DSP_SPACE1_WORDS
            && self.memory_hash(0, 0, E830_DSP_SPACE0_WORDS) == Some(E830_DSP_SPACE0_FNV64)
            && self.memory_hash(1, E830_DSP_SPACE1_START, E830_DSP_SPACE1_WORDS)
                == Some(E830_DSP_SPACE1_FNV64)
    }

    fn memory_hash(&self, space: usize, start: u16, count: usize) -> Option<u64> {
        let memory = self.memories.get(space)?;
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for offset in 0..count {
            let address = usize::from(start).checked_add(offset)?;
            let word = memory.get(&(u16::try_from(address).ok()?))?;
            for byte in word.to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        Some(hash)
    }

    fn runtime_signal_acknowledged(&mut self) -> bool {
        std::mem::take(&mut self.runtime_completion_pending)
    }

    fn runtime_hle_active(&self) -> bool {
        matches!(
            self.hle_phase,
            DspHlePhase::E830Running | DspHlePhase::E830RuntimeReady
        )
    }

}

struct E830State {
    keypad: crate::keypad::Keypad,
    registers: HashMap<u32, u32>,
    dsp_boot: DspBootInterface,
    dsp_rf: crate::dsp_rf::RfHle,
    nor: crate::nor::NorFlash,
    lcd_bus_mode: u16,
    lcd_register_index: u16,
    lcd_registers: [u16; 256],
    lcd_framebuffer: Vec<u16>,
    lcd_pixel_cursor: usize,
    lcd_pixels_since_frame: usize,
    lcd_frames: u64,
    keyboard_init_dispatches: u64,
    standby_look_dispatches: u64,
    hkbd_up_unblocked: u64,
    timer_counter: u16,
    timer_last_update: u64,
    timer_clock_remainder: u64,
    system_timer_irq_pending: bool,
    system_timer_match_irq_pending: bool,
    secondary_timer_counters: [u16; 2],
    secondary_timer_last_update: u64,
    secondary_timer_clock_remainder: u64,
    secondary_timer_pending: [u8; 2],
    instructions: u64,
    fast_forwarded_instructions: u64,
    instruction_budget: u64,
    next_device_update: u64,
    // The retail image has only a handful of recognized loop PCs. A sorted
    // compact table avoids hashing the current PC in the per-instruction hook.
    loop_kinds: Vec<(u64, u8)>,
    trace_mmio: bool,
    // Bounded passive SIM-controller trace; never performs an extra FIFO read.
    trace_sim: bool,
    trace_sim_after: u64,
    trace_sim_remaining: u64,
    // Passive MMIO callback trace: Unicorn's RAM MEM_WRITE hook does not
    // observe writes to mmio_map devices. No extra reads or DSP responses.
    trace_dsp_io: bool,
    trace_dsp_io_after: u64,
    trace_dsp_io_remaining: u64,
    pmic_gpio_asserted: bool,
    recent_mmio: VecDeque<MmioAccess>,
    memory_fault: Option<MemoryFault>,
    breakpoint: Option<(u64, u64)>,
    breakpoint_hit: bool,
    tcm_registers: TcmRegisters,
    uarts: [crate::uart::Uart; 2],
    dma_deadlines: [Option<u64>; 8],
    i2c: crate::i2c::I2c,
    sim: crate::sim::SimController,
    serial: crate::serial::SerialController,
    analog_hle: crate::analog_hle::AnalogHle,
    onenand: crate::onenand::OneNand,
    active_irq_line: u32,
    // Optional diagnostic source, separate from hardware devices.
    timer_irq_index: u32,
    timer_irq_period: u64,
    timer_irq_counter: u64,
    timer_irq_pending: bool,
    // Handlers may switch to System mode while servicing an IRQ. Block another
    // delivery until EOI instead of relying on CPU mode alone.
    timer_irq_in_service: bool,
    irq_eoi_seen: bool,
    irq_entry_nesting: u8,
    irqs_taken: u64,
    enabled_pending_cache: u32,
    enabled_pending_dirty: bool,
    // Read-only trace for the interrupt-controller service lifecycle.
    // Enabled by E830_TRACE_IRQ_SERVICE and bounded by its _AFTER/_LIMIT pair.
    trace_irq_service: bool,
    trace_irq_service_after: u64,
    trace_irq_service_remaining: u64,
    trace_irq_service_last: Option<(bool, bool, u8, u8, u32, u32, u32)>,
    // Diagnostic: when `pc_log_remaining` is non-zero and the executed
    // instruction count has passed `pc_log_after`, each real (non
    // fast-forwarded) instruction's PC is logged to stderr and the counter
    // decremented. Driven by E830_PC_LOG / _AFTER; off by default.
    pc_log_after: u64,
    pc_log_remaining: u64,
    // Optional bounded PC trace restricted to one current RTOS task. This
    // excludes interrupt handlers and unrelated tasks from late UI traces.
    task_pc_log_target: Option<u32>,
    task_pc_log_after: u64,
    task_pc_log_remaining: u64,
    // Optional read-only register trace at selected instruction addresses.
    watch_pcs: HashSet<u64>,
    watch_after: u64,
    watch_remaining: u64,
    // Bounded read-only trace for the late menu key path. It samples only
    // ordinary RAM at proven HKBD/EDP/MMI boundaries and never consumes MMIO.
    trace_menu_input: bool,
    trace_menu_input_after: u64,
    trace_menu_input_remaining: u64,
    // Bounded, read-only task/event snapshots; never changes guest state.
    trace_tasks: bool,
    trace_tasks_after: u64,
    trace_tasks_remaining: u64,
    trace_tasks_pcs: HashSet<u64>,
    trace_tasks_target: Option<u32>,
    trace_sp_below: Option<u32>,
    trace_sp_after: u64,
    trace_sp_remaining: u64,
    trace_sp_last: Option<u32>,
    trace_sp_last_pc: u64,
    // Diagnostic: when set (E830_TRACE_IRQEN), log the first PC at
    // which the CPSR IRQ mask (I) and FIQ mask (F) are observed cleared. This
    // pins down whether and where early boot ever unmasks interrupts.
    trace_irqen: bool,
    irqen_logged_i: bool,
    irqen_logged_f: bool,
    // Read-only focused trace for the post-mount power/wakeup investigation.
    // E830_TRACE_WAKE enables it; E830_TRACE_WAKE_LIMIT bounds stderr output.
    trace_wake: bool,
    trace_wake_after: u64,
    trace_wake_remaining: u64,
    // Read-only focused trace for the system timer/IRQ20 investigation.
    // Unlike the broader wake trace, counter reads are omitted so early boot
    // polling cannot consume the output budget before timer initialization.
    trace_timer: bool,
    trace_timer_after: u64,
    trace_timer_remaining: u64,
    // Read-only trace for the RTOS globals involved in the late object lookup.
    // This records the firmware instruction responsible for every write; it
    // does not initialize or otherwise alter guest state.
    trace_rtos_globals: bool,
    trace_rtos_globals_after: u64,
    trace_rtos_globals_remaining: u64,
    // Read-only focused trace for the critical-section nesting byte. Kept
    // separate from the broader RTOS-globals budget because this byte changes
    // frequently during normal scheduling.
    trace_nesting: bool,
    trace_nesting_after: u64,
    trace_nesting_remaining: u64,
    // Read-only trace for the ARM/DSP control and shared mailbox handshake.
    trace_dsp: bool,
    trace_dsp_after: u64,
    trace_dsp_remaining: u64,
    trace_dsp_last: Option<(u64, [u32; 8])>,
    dsp_queue_hle_phase: DspQueueHlePhase,
}

impl E830State {
    fn new(
        trace_mmio: bool,
        flash: Vec<u8>,
        delay_loop_pcs: HashSet<u64>,
        nested_delay_loop_pcs: HashSet<u64>,
        memset_loop_pcs: HashSet<u64>,
        fade_delay_loop_pcs: HashSet<u64>,
    ) -> Self {
        let mut registers = HashMap::new();
        registers.insert(SOC_BOOT_READY_INPUT_REGISTER, SOC_BOOT_READY_INPUT_MASK);
        Self {
            registers,
            keypad: Default::default(),
            dsp_boot: DspBootInterface::new(std::env::var_os("E830_DSP_BOOT_HLE").is_some()),
            dsp_rf: crate::dsp_rf::RfHle::new(
                std::env::var("E830_DSP_RF_HLE").as_deref() == Ok("empty")),
            nor: crate::nor::NorFlash::new(flash),
            lcd_bus_mode: 0,
            lcd_register_index: 0,
            lcd_registers: {
                let mut registers = [0; 256];
                registers[3] = 0x1030;
                registers[0x44] = ((LCD_WIDTH - 1) << 8) as u16;
                registers[0x45] = ((LCD_HEIGHT - 1) << 8) as u16;
                registers
            },
            lcd_framebuffer: vec![0; LCD_PIXEL_COUNT],
            lcd_pixel_cursor: 0,
            lcd_pixels_since_frame: 0,
            lcd_frames: 0,
            keyboard_init_dispatches: 0,
            standby_look_dispatches: 0,
            hkbd_up_unblocked: 0,
            timer_counter: 0,
            timer_last_update: 0,
            timer_clock_remainder: 0,
            system_timer_irq_pending: false,
            system_timer_match_irq_pending: false,
            secondary_timer_counters: [0; 2],
            secondary_timer_last_update: 0,
            secondary_timer_clock_remainder: 0,
            secondary_timer_pending: [0; 2],
            instructions: 0,
            fast_forwarded_instructions: 0,
            instruction_budget: u64::MAX,
            next_device_update: 0,
            loop_kinds: collect_loop_kinds([
                delay_loop_pcs, nested_delay_loop_pcs, memset_loop_pcs, fade_delay_loop_pcs,
            ]),
            trace_mmio,
            trace_sim: std::env::var_os("E830_TRACE_SIM").is_some(),
            trace_sim_after: env_u64("E830_TRACE_SIM_AFTER").unwrap_or(0),
            trace_sim_remaining: env_u64("E830_TRACE_SIM_LIMIT").unwrap_or(1000),
            trace_dsp_io: std::env::var_os("E830_TRACE_DSP_IO").is_some(),
            trace_dsp_io_after: env_u64("E830_TRACE_DSP_IO_AFTER").unwrap_or(0),
            trace_dsp_io_remaining: env_u64("E830_TRACE_DSP_IO_LIMIT").unwrap_or(1000),
            pmic_gpio_asserted: false,
            recent_mmio: VecDeque::with_capacity(MAX_RECENT_MMIO),
            memory_fault: None,
            breakpoint: None,
            breakpoint_hit: false,
            tcm_registers: TcmRegisters::default(),
            uarts: Default::default(),
            dma_deadlines: [None; 8],
            i2c: crate::i2c::I2c::with_sim_present(
                std::env::var("E830_SIM_PRESENT").as_deref() == Ok("1")
            ),
            serial: Default::default(),
            analog_hle: crate::analog_hle::AnalogHle::new(
                std::env::var("E830_ANALOG_HLE").as_deref() == Ok("battery")
            ),
            sim: Default::default(),
            onenand: Default::default(),
            active_irq_line: 0,
            timer_irq_index: env_u64("E830_TIMER_IRQ")
                .map(|value| value as u32)
                .unwrap_or(DEFAULT_TIMER_IRQ_INDEX),
            timer_irq_period: env_u64("E830_TIMER_PERIOD")
                .unwrap_or(DEFAULT_TIMER_IRQ_PERIOD),
            timer_irq_counter: 0,
            timer_irq_pending: false,
            timer_irq_in_service: false,
            irq_eoi_seen: false,
            irq_entry_nesting: 0,
            irqs_taken: 0,
            enabled_pending_cache: 0,
            enabled_pending_dirty: true,
            trace_irq_service: std::env::var("E830_TRACE_IRQ_SERVICE").is_ok(),
            trace_irq_service_after: env_u64("E830_TRACE_IRQ_SERVICE_AFTER").unwrap_or(0),
            trace_irq_service_remaining: env_u64("E830_TRACE_IRQ_SERVICE_LIMIT").unwrap_or(1000),
            trace_irq_service_last: None,
            pc_log_after: env_u64("E830_PC_LOG_AFTER").unwrap_or(0),
            pc_log_remaining: env_u64("E830_PC_LOG").unwrap_or(0),
            task_pc_log_target: env_u64("E830_TASK_PC_LOG_TARGET").map(|value| value as u32),
            task_pc_log_after: env_u64("E830_TASK_PC_LOG_AFTER").unwrap_or(0),
            task_pc_log_remaining: env_u64("E830_TASK_PC_LOG_LIMIT").unwrap_or(0),
            watch_pcs: std::env::var("E830_WATCH_PC").unwrap_or_default()
                .split(',').filter_map(|raw| {
                    let raw = raw.trim();
                    raw.strip_prefix("0x").map(|hex| u64::from_str_radix(hex, 16))
                        .unwrap_or_else(|| raw.parse()).ok().map(|pc| pc & !1)
                }).collect(),
            watch_after: env_u64("E830_WATCH_AFTER").unwrap_or(0),
            watch_remaining: env_u64("E830_WATCH_LIMIT").unwrap_or(1000),
            trace_menu_input: std::env::var_os("E830_TRACE_MENU_INPUT").is_some(),
            trace_menu_input_after: env_u64("E830_TRACE_MENU_INPUT_AFTER").unwrap_or(0),
            trace_menu_input_remaining: env_u64("E830_TRACE_MENU_INPUT_LIMIT").unwrap_or(200),
            trace_tasks: std::env::var_os("E830_TRACE_TASKS").is_some(),
            trace_tasks_after: env_u64("E830_TRACE_TASKS_AFTER").unwrap_or(0),
            trace_tasks_remaining: env_u64("E830_TRACE_TASKS_LIMIT").unwrap_or(1000),
            trace_tasks_pcs: std::env::var("E830_TRACE_TASKS_PCS").ok().map(|value| {
                value.split(',').filter_map(|raw| {
                    let raw = raw.trim();
                    raw.strip_prefix("0x").map(|hex| u64::from_str_radix(hex, 16))
                        .unwrap_or_else(|| raw.parse()).ok().map(|pc| pc & !1)
                }).filter(|pc| RTOS_TASK_TRACE_PCS.contains(pc)).collect()
            }).unwrap_or_else(|| RTOS_TASK_TRACE_PCS.into_iter().collect()),
            trace_tasks_target: env_u64("E830_TRACE_TASKS_TARGET").map(|value| value as u32),
            trace_sp_below: env_u64("E830_TRACE_SP_BELOW").map(|value| value as u32),
            trace_sp_after: env_u64("E830_TRACE_SP_AFTER").unwrap_or(0),
            trace_sp_remaining: env_u64("E830_TRACE_SP_LIMIT").unwrap_or(1000),
            trace_sp_last: None,
            trace_sp_last_pc: 0,
            trace_irqen: std::env::var("E830_TRACE_IRQEN").is_ok(),
            irqen_logged_i: false,
            irqen_logged_f: false,
            trace_wake: std::env::var("E830_TRACE_WAKE").is_ok(),
            trace_wake_after: env_u64("E830_TRACE_WAKE_AFTER").unwrap_or(0),
            trace_wake_remaining: env_u64("E830_TRACE_WAKE_LIMIT").unwrap_or(4000),
            trace_timer: std::env::var("E830_TRACE_TIMER").is_ok(),
            trace_timer_after: env_u64("E830_TRACE_TIMER_AFTER").unwrap_or(0),
            trace_timer_remaining: env_u64("E830_TRACE_TIMER_LIMIT").unwrap_or(4000),
            trace_rtos_globals: std::env::var("E830_TRACE_RTOS_GLOBALS").is_ok(),
            trace_rtos_globals_after: env_u64("E830_TRACE_RTOS_GLOBALS_AFTER").unwrap_or(0),
            trace_rtos_globals_remaining: env_u64("E830_TRACE_RTOS_GLOBALS_LIMIT").unwrap_or(1000),
            trace_nesting: std::env::var("E830_TRACE_NESTING").is_ok(),
            trace_nesting_after: env_u64("E830_TRACE_NESTING_AFTER").unwrap_or(0),
            trace_nesting_remaining: env_u64("E830_TRACE_NESTING_LIMIT").unwrap_or(1000),
            trace_dsp: std::env::var("E830_TRACE_DSP").is_ok(),
            trace_dsp_after: env_u64("E830_TRACE_DSP_AFTER").unwrap_or(0),
            trace_dsp_remaining: env_u64("E830_TRACE_DSP_LIMIT").unwrap_or(1000),
            trace_dsp_last: None,
            dsp_queue_hle_phase: DspQueueHlePhase::Idle,
        }
    }

    fn trace_wake_line(&mut self, line: String) {
        let total = self.instructions.saturating_add(self.fast_forwarded_instructions);
        if !self.trace_wake
            || total < self.trace_wake_after
            || self.trace_wake_remaining == 0
        {
            return;
        }
        self.trace_wake_remaining -= 1;
        eprintln!("[e830-wake] {line}");
    }

    fn trace_timer_line(&mut self, line: String) {
        let total = self.instructions.saturating_add(self.fast_forwarded_instructions);
        if !self.trace_timer
            || total < self.trace_timer_after
            || self.trace_timer_remaining == 0
        {
            return;
        }
        self.trace_timer_remaining -= 1;
        eprintln!("[e830-timer] {line}");
    }

    fn record_mmio(&mut self, access: MmioAccess) {
        if self.trace_dsp_io
            && self.trace_dsp_io_remaining > 0
            && ((DSP_SERIAL_BASE..DSP_SERIAL_BASE + 0x204).contains(&access.address)
                || (DSP_SIGNAL_BASE..DSP_SIGNAL_BASE + 0x20).contains(&access.address)
                || (DSP_CONTROL_BASE as u32..DSP_CONTROL_BASE as u32 + 0x10)
                    .contains(&access.address))
        {
            let total = self.instructions.saturating_add(self.fast_forwarded_instructions);
            if total >= self.trace_dsp_io_after {
                self.trace_dsp_io_remaining -= 1;
                let action = if access.write { "write" } else { "read" };
                eprintln!(
                    "[e830-dsp-io] {action} addr=0x{:08x} size={} value=0x{:08x} pc=0x{:08x} insns={total}",
                    access.address, access.size, access.value, access.pc
                );
            }
        }
        if self.trace_sim && self.trace_sim_remaining != 0
            && (0x5040_6000..0x5040_7000).contains(&access.address)
        {
            let total = self.instructions.saturating_add(self.fast_forwarded_instructions);
            if total >= self.trace_sim_after {
                self.trace_sim_remaining -= 1;
                let action = if access.write { "write" } else { "read" };
                eprintln!(
                    "[e830-sim] {action} addr=0x{:08x} size={} value=0x{:08x} pc=0x{:08x} insns={total}",
                    access.address, access.size, access.value, access.pc
                );
            }
        }
        if self.recent_mmio.len() == MAX_RECENT_MMIO {
            self.recent_mmio.pop_front();
        }
        if self.trace_mmio {
            let action = if access.write { "write" } else { "read " };
            eprintln!(
                "[e830-mmio] {action} 0x{:08x} size={} value=0x{:08x} pc=0x{:08x}",
                access.address, access.size, access.value, access.pc
            );
        }
        if (WAKE_MMIO_START..=WAKE_MMIO_END).contains(&access.address)
            || (INTC_PENDING_NUMBER_REGISTER..=INTC_PENDING_MASK_REGISTER)
                .contains(&access.address)
        {
            let action = if access.write { "write" } else { "read" };
            let total = self.instructions.saturating_add(self.fast_forwarded_instructions);
            self.trace_wake_line(format!(
                "mmio {action} addr=0x{:08x} size={} value=0x{:08x} pc=0x{:08x} insns={total}",
                access.address, access.size, access.value, access.pc
            ));
        }
        if (access.write
            && ((SOC_TIMER..=SOC_TIMER + 0x14).contains(&access.address)
                || access.address == INTC_ENABLE_MASK_REGISTER))
            || SECONDARY_TIMER_BASES.iter().any(|base| {
                (*base..=*base + 0x20).contains(&access.address)
            })
        {
            let action = if access.write { "write" } else { "read" };
            let total = self.instructions.saturating_add(self.fast_forwarded_instructions);
            let interrupt_mask = self
                .registers
                .get(&INTC_ENABLE_MASK_REGISTER)
                .copied()
                .unwrap_or(u32::MAX);
            let irq20_masked = interrupt_mask & (1 << SYSTEM_TIMER_IRQ) != 0;
            let irq21_masked = interrupt_mask & (1 << SYSTEM_TIMER_MATCH_IRQ) != 0;
            let irq18_masked = interrupt_mask & (1 << SECONDARY_TIMER_IRQS[0]) != 0;
            let irq17_masked = interrupt_mask & (1 << SECONDARY_TIMER_IRQS[1]) != 0;
            self.trace_timer_line(format!(
                "mmio {action} addr=0x{:08x} size={} value=0x{:08x} pc=0x{:08x} irq17_masked={irq17_masked} irq18_masked={irq18_masked} irq20_masked={irq20_masked} irq21_masked={irq21_masked} insns={total}",
                access.address, access.size, access.value, access.pc
            ));
        }
        self.recent_mmio.push_back(access);
    }

    // Device interrupt levels persist until their own status is cleared.
    fn pending_lines(&self) -> u32 {
        let diagnostic = if self.timer_irq_pending { 1 << (self.timer_irq_index & 31) } else { 0 };
        let system_timer = if self.system_timer_irq_pending { 1 << SYSTEM_TIMER_IRQ } else { 0 };
        let system_timer_match = if self.system_timer_match_irq_pending {
            1 << SYSTEM_TIMER_MATCH_IRQ
        } else {
            0
        };
        let secondary_timers = (0..SECONDARY_TIMER_BASES.len()).fold(0, |lines, timer| {
            let active = (0..SECONDARY_TIMER_COMPARE_OFFSETS.len()).any(|channel| {
                self.secondary_timer_pending[timer] & (1 << channel) != 0
                    && self.secondary_timer_channel_enabled(timer, channel)
            });
            lines | if active {
                1 << SECONDARY_TIMER_IRQS[timer]
            } else {
                0
            }
        });
        let i2c = if self.i2c.irq_pending() { 1 << I2C_IRQ } else { 0 };
        let serial_control = self.registers.get(&(DSP_SERIAL_BASE + 0x0c)).copied().unwrap_or(0);
        let serial_status = self.registers.get(&(DSP_SERIAL_BASE + 0x14)).copied().unwrap_or(0);
        let dsp_serial = if serial_control & DSP_SERIAL_TX_ENABLE != 0
            && serial_status & DSP_SERIAL_TX_COMPLETE != 0
        {
            1 << DSP_SERIAL_IRQ
        } else {
            0
        };
        let dma = if self.dma_interrupt_status() != 0 { 1 << DMA_IRQ } else { 0 };
        let gpio = if self.i2c.sim_model_enabled() {
            let status = self.registers.get(&(GPIO_BASE + 0x4c)).copied().unwrap_or(0);
            (0..3).fold(0, |lines, group| {
                let mask = self.registers.get(&(GPIO_BASE + 0x40 + group * 4)).copied().unwrap_or(0);
                lines | if status & mask & PMIC_GPIO_PIN != 0 { 1 << group } else { 0 }
            })
        } else { 0 };
        let sim = if self.i2c.sim_model_enabled() && self.sim.irq_pending() {
            1 << 30
        } else { 0 };
        let keypad = if self.keypad.irq_pending() { 1 << 16 } else { 0 };
        diagnostic | system_timer | system_timer_match | secondary_timers
            | i2c | dsp_serial | dma | gpio | sim | keypad
    }

    fn sync_pmic_gpio(&mut self) {
        if !self.i2c.sim_model_enabled() { return; }
        // HECI init 0x21189b42 registers pin 3 / group 0 to 0x2013dcdc,
        // which wakes event bit 0. This is not the phone-open pin 9.
        let asserted = self.i2c.pmic_irq_pending();
        let input = self.registers.entry(SOC_BOOT_READY_INPUT_REGISTER).or_default();
        if asserted { *input &= !PMIC_GPIO_PIN; } else { *input |= PMIC_GPIO_PIN; }
        if asserted && !self.pmic_gpio_asserted {
            *self.registers.entry(GPIO_BASE + 0x4c).or_default() |= PMIC_GPIO_PIN;
        }
        self.pmic_gpio_asserted = asserted;
    }

    fn dma_interrupt_status(&self) -> u32 {
        let raw = self.registers.get(&(DMA_BASE + 0x14)).copied().unwrap_or(0);
        // No channel mask can turn an absent terminal count into an IRQ.
        // This is the usual case and avoids eight map lookups per instruction.
        if raw & 0xff == 0 {
            return 0;
        }
        let enabled = (0..8).fold(0, |mask, channel| {
            let config = self.registers.get(&(DMA_BASE + 0x110 + channel * 0x20))
                .copied().unwrap_or(0);
            mask | if config & 0x8000 != 0 { 1 << channel } else { 0 }
        });
        raw & enabled & 0xff
    }

    fn dma_uart0_tx(&self, channel: usize) -> Option<(u32, usize)> {
        if self.registers.get(&(DMA_BASE + 0x30)).copied().unwrap_or(0) & 1 == 0 {
            return None;
        }
        let base = DMA_BASE + 0x100 + channel as u32 * 0x20;
        let word = |offset| self.registers.get(&(base + offset)).copied().unwrap_or(0);
        let count = (word(0x0c) & 0xfff) as usize;
        let source = word(0);
        let end = u64::from(source) + count as u64;
        (word(0x10) == DMA_UART0_TX_CONFIG
            && word(0x0c) & !0xfff == DMA_UART0_TX_CONTROL
            && word(4) == UART_BASES[0] && word(8) == 0 && count != 0
            && source >= RAM_BASE && end <= u64::from(RAM_BASE) + u64::from(RAM_SIZE))
            .then_some((source, count))
    }

    fn dma_sim_tx(&self, channel: usize) -> Option<(u32, usize)> {
        if !self.i2c.sim_model_enabled() || !self.i2c.sim_powered()
            || self.registers.get(&(DMA_BASE + 0x30)).copied().unwrap_or(0) & 1 == 0
        { return None; }
        let base = DMA_BASE + 0x100 + channel as u32 * 0x20;
        let word = |offset| self.registers.get(&(base + offset)).copied().unwrap_or(0);
        let count = (word(0x0c) & 0xfff) as usize;
        let source = word(0);
        (word(0x10) == DMA_SIM_TX_CONFIG
            && word(0x0c) & !0xfff == DMA_UART0_TX_CONTROL
            && word(4) == 0x5040_6034 && word(8) == 0 && count != 0
            && source >= RAM_BASE
            && u64::from(source) + count as u64 <= u64::from(RAM_BASE) + u64::from(RAM_SIZE))
            .then_some((source, count))
    }

    fn dma_sim_rx(&self, channel: usize) -> Option<(u32, usize)> {
        if !self.i2c.sim_model_enabled() || !self.i2c.sim_powered()
            || self.registers.get(&(DMA_BASE + 0x30)).copied().unwrap_or(0) & 1 == 0
        { return None; }
        let base = DMA_BASE + 0x100 + channel as u32 * 0x20;
        let word = |offset| self.registers.get(&(base + offset)).copied().unwrap_or(0);
        let count = (word(0x0c) & 0xfff) as usize;
        let destination = word(4);
        (word(0x10) == DMA_SIM_RX_CONFIG && word(0x0c) & !0xfff == 0x8800_0000
            && word(0) == 0x5040_6034 && word(8) == 0 && count != 0
            && destination >= RAM_BASE
            && u64::from(destination) + count as u64 <= u64::from(RAM_BASE) + u64::from(RAM_SIZE))
            .then_some((destination, count))
    }

    fn advance_dsp_serial(&mut self, now: u64) {
        if let DspQueueHlePhase::CompletePending { deadline } = self.dsp_queue_hle_phase {
            if now >= deadline {
                // Publish the device-owned sample before the established
                // serial completion. ARM copies/converts it after W0C ACK.
                self.analog_hle.complete(self.dsp_boot.runtime_hle_active());
                // The exact-image HLE supplies only the observed transfer-done
                // status. IRQ9 and all callbacks/queue changes belong to the
                // original serial ISR (210d4fc0 -> 21122084 -> 215a7cb8).
                *self.registers.entry(DSP_SERIAL_BASE + 0x14).or_default()
                    |= DSP_SERIAL_TX_COMPLETE;
                self.dsp_queue_hle_phase = DspQueueHlePhase::CompleteSignaled;
            }
        }
    }

    fn next_device_deadline(&self, now: u64) -> u64 {
        // The system timer advances one raw tick per eight ARM instructions.
        // Other modeled peripherals expose their exact completion deadline.
        // Poll only the earliest one instead of advancing every device from
        // the global per-instruction hook when no state can possibly change.
        let until_timer_tick = SYSTEM_TIMER_CPU_CLOCK_RATIO - self.timer_clock_remainder;
        let mut deadline = now.saturating_add(until_timer_tick.max(1));
        let mut include = |candidate: Option<u64>| {
            if let Some(candidate) = candidate { deadline = deadline.min(candidate); }
        };
        include(self.dma_deadlines.iter().flatten().copied().min());
        include(match self.dsp_queue_hle_phase {
            DspQueueHlePhase::CompletePending { deadline } => Some(deadline),
            _ => None,
        });
        include(self.i2c.next_deadline());
        if self.i2c.sim_model_enabled() { include(self.sim.next_deadline()); }
        include(self.dsp_rf.next_deadline());
        include(self.secondary_timer_deadline(now));
        if self.timer_irq_period != 0 && !self.timer_irq_pending {
            include(Some(self.timer_irq_counter.saturating_add(self.timer_irq_period)));
        }
        deadline.max(now.saturating_add(1))
    }

    fn secondary_timer_channel_enabled(&self, timer: usize, channel: usize) -> bool {
        let base = SECONDARY_TIMER_BASES[timer];
        let control = self.registers.get(&base).copied().unwrap_or(0);
        control & SECONDARY_TIMER_CONTROL_BITS[channel] != 0
    }

    fn secondary_timer_status(&self, timer: usize) -> u32 {
        (0..SECONDARY_TIMER_STATUS_BITS.len()).fold(0, |status, channel| {
            status | if self.secondary_timer_pending[timer] & (1 << channel) != 0 {
                SECONDARY_TIMER_STATUS_BITS[channel]
            } else {
                0
            }
        })
    }

    fn secondary_timer_deadline(&self, now: u64) -> Option<u64> {
        let mut deadline = None;
        for timer in 0..SECONDARY_TIMER_BASES.len() {
            let counter = self.secondary_timer_counters[timer];
            for channel in 0..SECONDARY_TIMER_COMPARE_OFFSETS.len() {
                if self.secondary_timer_pending[timer] & (1 << channel) != 0
                    || !self.secondary_timer_channel_enabled(timer, channel)
                {
                    continue;
                }
                let compare = self.registers
                    .get(&(SECONDARY_TIMER_BASES[timer]
                        + SECONDARY_TIMER_COMPARE_OFFSETS[channel]))
                    .copied().unwrap_or(0) as u16;
                let mut ticks = u64::from(compare.wrapping_sub(counter));
                if ticks == 0 { ticks = u64::from(u16::MAX) + 1; }
                let instructions = ticks
                    .saturating_mul(SECONDARY_TIMER_CPU_CLOCK_RATIO)
                    .saturating_sub(self.secondary_timer_clock_remainder)
                    .max(1);
                let candidate = now.saturating_add(instructions);
                deadline = Some(deadline.map_or(candidate, |current: u64| current.min(candidate)));
            }
        }
        deadline
    }

    fn advance_secondary_timers(&mut self, now: u64) {
        let elapsed = now.saturating_sub(self.secondary_timer_last_update);
        self.secondary_timer_last_update = now;
        let clocks = self.secondary_timer_clock_remainder.saturating_add(elapsed);
        let ticks = clocks / SECONDARY_TIMER_CPU_CLOCK_RATIO;
        self.secondary_timer_clock_remainder = clocks % SECONDARY_TIMER_CPU_CLOCK_RATIO;
        if ticks == 0 { return; }

        let mut raised = Vec::new();
        for timer in 0..SECONDARY_TIMER_BASES.len() {
            let previous = self.secondary_timer_counters[timer];
            for channel in 0..SECONDARY_TIMER_COMPARE_OFFSETS.len() {
                if self.secondary_timer_pending[timer] & (1 << channel) != 0
                    || !self.secondary_timer_channel_enabled(timer, channel)
                {
                    continue;
                }
                let compare = self.registers
                    .get(&(SECONDARY_TIMER_BASES[timer]
                        + SECONDARY_TIMER_COMPARE_OFFSETS[channel]))
                    .copied().unwrap_or(0) as u16;
                if wrapping_u16_counter_crossed(previous, ticks, compare) {
                    self.secondary_timer_pending[timer] |= 1 << channel;
                    raised.push((timer, channel, previous, compare));
                }
            }
            self.secondary_timer_counters[timer] = previous.wrapping_add(ticks as u16);
        }
        for (timer, channel, previous, compare) in raised {
            let base = SECONDARY_TIMER_BASES[timer];
            let control = self.registers.get(&base).copied().unwrap_or(0);
            let mask = self.registers.get(&INTC_ENABLE_MASK_REGISTER)
                .copied().unwrap_or(u32::MAX);
            let irq = SECONDARY_TIMER_IRQS[timer];
            self.trace_timer_line(format!(
                "secondary match base=0x{base:08x} channel={channel} previous=0x{previous:04x} compare=0x{compare:04x} ticks={ticks} control=0x{control:08x} status=0x{:02x} irq={irq} masked={} insns={now}",
                self.secondary_timer_status(timer), mask & (1 << irq) != 0,
            ));
        }
    }

    // Non-mutating projection for the opt-in RF model. Only the observed
    // 5000-unit frame with its original 4700-unit collector is supported.
    // Status bits are independent of the mode; no timer/IRQ state is changed.
    fn rf_timer_phase_clocks(&self, now: u64) -> Option<u64> {
        let register = |offset| self.registers.get(&(SOC_TIMER + offset)).copied().unwrap_or(0);
        if register(8) & !SYSTEM_TIMER_STATUS_MASK != 0x0660
            || register(0x0c) & u32::from(SYSTEM_TIMER_COMPARE_MASK) != 5000
            || register(0x10) & u32::from(SYSTEM_TIMER_COMPARE_MASK) != 0
            || register(0x14) & u32::from(SYSTEM_TIMER_COMPARE_MASK) != 4700
            || self.timer_counter >= 5000 << SYSTEM_TIMER_FRACTION_BITS
            || self.timer_clock_remainder >= SYSTEM_TIMER_CPU_CLOCK_RATIO
        { return None; }
        let elapsed = now.checked_sub(self.timer_last_update)?;
        let clocks = self.timer_clock_remainder.checked_add(elapsed)?;
        let period_raw = 5000u64 << SYSTEM_TIMER_FRACTION_BITS;
        let raw = (u64::from(self.timer_counter)
            + clocks / SYSTEM_TIMER_CPU_CLOCK_RATIO) % period_raw;
        Some(raw * SYSTEM_TIMER_CPU_CLOCK_RATIO + clocks % SYSTEM_TIMER_CPU_CLOCK_RATIO)
    }

    fn advance_system_timer(&mut self, now: u64) {
        let elapsed = now.saturating_sub(self.timer_last_update);
        self.timer_last_update = now;
        let clocks = self.timer_clock_remainder.saturating_add(elapsed);
        let ticks = clocks / SYSTEM_TIMER_CPU_CLOCK_RATIO;
        self.timer_clock_remainder = clocks % SYSTEM_TIMER_CPU_CLOCK_RATIO;
        if ticks == 0 {
            return;
        }

        let control = self.registers.get(&(SOC_TIMER + 8)).copied().unwrap_or(0);
        let period = self
            .registers
            .get(&(SOC_TIMER + 0x0c))
            .copied()
            .unwrap_or(0) as u16 & SYSTEM_TIMER_COMPARE_MASK;
        let match0 = self
            .registers
            .get(&(SOC_TIMER + 0x10))
            .copied()
            .unwrap_or(0) as u16 & SYSTEM_TIMER_COMPARE_MASK;
        let match1 = self
            .registers
            .get(&(SOC_TIMER + 0x14))
            .copied()
            .unwrap_or(0) as u16 & SYSTEM_TIMER_COMPARE_MASK;
        if control & SYSTEM_TIMER_MATCH_CHANNELS == SYSTEM_TIMER_MATCH_CHANNELS && period != 0 {
            let period = period << SYSTEM_TIMER_FRACTION_BITS;
            let match0 = match0 << SYSTEM_TIMER_FRACTION_BITS;
            let match1 = match1 << SYSTEM_TIMER_FRACTION_BITS;
            let previous_counter = self.timer_counter;
            let match0_crossed = periodic_timer_crossed(previous_counter, ticks, period, match0);
            let match1_crossed = periodic_timer_crossed(previous_counter, ticks, period, match1);
            // Integer period 5000 wraps raw counter at 20000. Match0 is
            // zero, and integer match1=4700 compares at raw 18800.
            self.timer_counter = ((u64::from(previous_counter) + ticks) % u64::from(period)) as u16;
            if match0_crossed && control & SYSTEM_TIMER_IRQ20_ENABLE != 0 {
                *self.registers.entry(SOC_TIMER + 8).or_default() |=
                    SYSTEM_TIMER_IRQ20_STATUS;
                self.system_timer_irq_pending = true;
            }
            if match1_crossed && control & SYSTEM_TIMER_IRQ21_ENABLE != 0 {
                *self.registers.entry(SOC_TIMER + 8).or_default() |=
                    SYSTEM_TIMER_IRQ21_STATUS;
                self.system_timer_match_irq_pending = true;
            }
            return;
        }

        let previous_counter = self.timer_counter;
        let wraps = ticks > u64::from(u16::MAX - previous_counter);
        self.timer_counter = previous_counter.wrapping_add(ticks as u16);
        if wraps && control & SYSTEM_TIMER_IRQ20_ENABLE != 0 {
            *self.registers.entry(SOC_TIMER + 8).or_default() |=
                SYSTEM_TIMER_IRQ20_STATUS;
            self.system_timer_irq_pending = true;
        }
    }

    fn enabled_pending_lines(&mut self) -> u32 {
        if !self.enabled_pending_dirty {
            return self.enabled_pending_cache;
        }
        let mask = self
            .registers
            .get(&INTC_ENABLE_MASK_REGISTER)
            .copied()
            .unwrap_or(u32::MAX);
        self.enabled_pending_cache = if mask == u32::MAX {
            0
        } else {
            self.pending_lines() & !mask
        };
        self.enabled_pending_dirty = false;
        self.enabled_pending_cache
    }

    fn read_register(&mut self, address: u32, size: usize, pc: u32) -> u64 {
        // Some device reads consume FIFO/status state. Conservatively refresh
        // the cached interrupt projection after any MMIO read.
        self.enabled_pending_dirty = true;
        if (0x5030_8000..0x5030_8010).contains(&address) {
            return self.keypad.read(address - 0x5030_8000, size);
        }
        if u64::from(address) >= DSP_CONTROL_BASE
            && u64::from(address) < DSP_CONTROL_BASE + DSP_BOOT_MMIO_SIZE
        {
            return self.dsp_boot.read(address, size);
        }
        if u64::from(address) >= ONENAND_BASE && u64::from(address) < ONENAND_BASE + ONENAND_SIZE {
            return self.onenand.read(address - ONENAND_BASE as u32, size);
        }
        let aligned = address & !3;
        if self.i2c.sim_model_enabled() && (0x5040_6000..0x5040_7000).contains(&address) {
            return self.sim.read(address - 0x5040_6000, size);
        }
        let raw = if aligned == DSP_SERIAL_BASE + 0x18 {
            let addresses = self.registers.get(&(DSP_SERIAL_BASE + 4)).copied().unwrap_or(0);
            let extension = self.registers.get(&(DSP_SERIAL_BASE + 8)).copied().unwrap_or(0);
            // Transaction-local overlay: unsupported requests see exactly the
            // original register, never a stale synthetic battery sample.
            self.analog_hle.sample(self.dsp_boot.runtime_hle_active(), addresses, extension)
                .map(u32::from).unwrap_or_else(|| self.registers.get(&aligned).copied().unwrap_or(0))
        } else if let Some((channel, base)) = UART_BASES.iter().enumerate()
            .find(|(_, base)| aligned >= **base && aligned < **base + 0x20) {
            self.uarts[channel].read(aligned - base)
        } else if matches!(aligned, DMA_BASE | 0x5010_0004) {
            self.dma_interrupt_status()
        } else if aligned == DMA_BASE + 0x1c {
            (0..8).fold(0, |mask, channel| {
                let config = self.registers.get(&(DMA_BASE + 0x110 + channel * 0x20))
                    .copied().unwrap_or(0);
                mask | if config & 1 != 0 { 1 << channel } else { 0 }
            })
        } else if aligned >= I2C_BASE && aligned < I2C_BASE + 0x30 {
            self.i2c.read(aligned - I2C_BASE)
        } else if aligned >= SERIAL_BASE && aligned < SERIAL_BASE + 0x20 {
            return self.serial.read(address - SERIAL_BASE, size);
        } else if SOC_KEYPAD_SCAN_READ_PCS.contains(&pc)
            && SOC_KEYPAD_INPUT_REGISTERS.contains(&aligned)
        {
            u32::MAX
        } else if let Some(timer) = SECONDARY_TIMER_BASES.iter()
            .position(|base| aligned == *base + 8)
        {
            u32::from(self.secondary_timer_counters[timer])
        } else if let Some(timer) = SECONDARY_TIMER_BASES.iter()
            .position(|base| aligned == *base + 0x1c)
        {
            self.secondary_timer_status(timer)
        } else if address == SOC_TIMER {
            u32::from(self.timer_counter)
        } else if aligned == INTC_PENDING_NUMBER_REGISTER {
            // The IRQ vector handler reads this byte and uses it directly as the
            // pre-multiplied dispatch index, stable while a handler runs.
            if self.timer_irq_in_service {
                self.active_irq_line * 4
            } else {
                let pending = self.enabled_pending_lines();
                if pending != 0 { (31 - pending.leading_zeros()) * 4 }
                else { self.registers.get(&aligned).copied().unwrap_or(0) }
            }
        } else if aligned == INTC_PENDING_MASK_REGISTER {
            // Firmware uses +0x1c to recognize an accepted interrupt, not raw
            // asynchronous device levels. A level arriving between a task's
            // paired lock/unlock tests must not change its execution context.
            // Keep only the accepted source visible until EOI; new peripheral
            // requests remain available separately through +0x14 and IRQ entry.
            if self.timer_irq_in_service && !self.irq_eoi_seen {
                1u32 << (self.active_irq_line & 31)
            } else {
                0
            }
        } else if aligned == INTC_MASKED_PENDING_REGISTER {
            self.enabled_pending_lines()
        } else {
            self.registers.get(&aligned).copied().unwrap_or(0)
        };
        let shift = (address & 3) * 8;
        let mask = width_mask(size);
        u64::from((raw >> shift) & mask)
    }

    fn write_register(&mut self, address: u32, size: usize, value: u64) -> Option<DspEvent> {
        // Register writes may acknowledge an IRQ, change a mask, or schedule
        // peripheral work. Recompute the projection on the next CPU step.
        self.enabled_pending_dirty = true;
        if (0x5030_8000..0x5030_8010).contains(&address) {
            self.keypad.write(address - 0x5030_8000, value);
            return None;
        }
        if u64::from(address) >= DSP_CONTROL_BASE
            && u64::from(address) < DSP_CONTROL_BASE + DSP_BOOT_MMIO_SIZE
        {
            let event = self.dsp_boot.write(address, size, value);
            match event {
                Some(DspEvent::StartupReady) => {
                    *self.registers.entry(DSP_START_SIGNAL).or_default() |= 1;
                }
                Some(DspEvent::RuntimeReady) => {
                    *self.registers.entry(DSP_RUNTIME_SIGNAL).or_default() |= 1;
                }
                None => {}
            }
            return event;
        }
        if u64::from(address) >= ONENAND_BASE && u64::from(address) < ONENAND_BASE + ONENAND_SIZE {
            self.onenand.write(address - ONENAND_BASE as u32, size, value);
            return None;
        }
        let aligned = address & !3;
        if self.i2c.sim_model_enabled() && (0x5040_6000..0x5040_7000).contains(&address) {
            let now = self.instructions.saturating_add(self.fast_forwarded_instructions);
            self.sim.write(address - 0x5040_6000, size, value, now);
            return None;
        }
        if self.i2c.sim_model_enabled() && aligned == GPIO_BASE + 0x4c {
            // Firmware GPIO handler 0x200a80e4 acknowledges with W0C halfwords.
            let mask = width_mask(size).wrapping_shl((address & 3) * 8);
            let written = (value as u32).wrapping_shl((address & 3) * 8);
            *self.registers.entry(aligned).or_default() &= !mask | written;
            return None;
        }
        if self.i2c.sim_model_enabled() && aligned == SOC_BOOT_READY_INPUT_REGISTER {
            return None; // external input, not writable storage
        }
        if matches!(aligned, 0x5010_0008 | 0x5010_0010) {
            // Original DMA ISR clears terminal count and error independently
            // through W1C registers +8/+0x10. INTC EOI does not clear a device.
            let raw = if aligned == DMA_BASE + 8 { DMA_BASE + 0x14 } else { DMA_BASE + 0x18 };
            *self.registers.entry(raw).or_default() &= !((value as u32)
                .wrapping_shl((address & 3) * 8) & 0xff);
            return None;
        }
        if matches!(aligned, DMA_BASE | 0x5010_0004 | 0x5010_000c
            | 0x5010_0014 | 0x5010_0018 | 0x5010_001c)
        {
            return None; // Device-owned status, not writable scratch storage.
        }
        if aligned >= I2C_BASE && aligned < I2C_BASE + 0x30 {
            let now = self.instructions.saturating_add(self.fast_forwarded_instructions);
            self.i2c.write(aligned - I2C_BASE, value as u32, now);
            return None;
        }
        if aligned >= SERIAL_BASE && aligned < SERIAL_BASE + 0x20 {
            self.serial.write(address - SERIAL_BASE, size, value);
            return None;
        }
        if let Some((channel, base)) = UART_BASES.iter().enumerate()
            .find(|(_, base)| aligned >= **base && aligned < **base + 0x20) {
            self.uarts[channel].write(aligned - base, value as u32);
            return None;
        }
        if SECONDARY_TIMER_BASES.iter().any(|base| aligned == *base + 8) {
            // The free-running counter is device-owned. The E830 firmware only
            // reads it; compare and control registers remain normally writable.
            return None;
        }
        let shift = (address & 3) * 8;
        let value_mask = width_mask(size);
        let mask = value_mask.wrapping_shl(shift);
        if let Some(timer) = SECONDARY_TIMER_BASES.iter()
            .position(|base| aligned == *base + 0x1c)
        {
            // The compare flags are device-owned and W0C. Firmware writes the
            // complement of a sampled byte, so a zero acknowledges exactly the
            // status bits it observed. Writes cannot arm or raise a channel.
            let written = ((value as u32) & value_mask).wrapping_shl(shift);
            for (channel, status_bit) in SECONDARY_TIMER_STATUS_BITS.iter().enumerate() {
                if mask & status_bit != 0 && written & status_bit == 0 {
                    self.secondary_timer_pending[timer] &= !(1 << channel);
                }
            }
            return None;
        }
        if matches!(aligned, DSP_START_SIGNAL | DSP_RUNTIME_SIGNAL | DSP_RESULT_SIGNAL) {
            // Original result collector 20576a2a writes the complement of
            // sampled flags to +0x1c. Like +0x10/+0x18, acknowledgements are
            // W0C: ARM writes cannot create a DSP completion or its payload.
            let written = ((value as u32) & value_mask).wrapping_shl(shift);
            let register = self.registers.entry(aligned).or_default();
            *register = (*register & !mask) | ((*register & mask) & written);
            if aligned == DSP_RUNTIME_SIGNAL
                && *register & 1 == 0
                && self.dsp_boot.runtime_signal_acknowledged()
            {
                *register |= 1;
            }
            return None;
        }
        let written = ((value as u32).wrapping_shl(shift)) & mask;
        if [DSP_SERIAL_BASE + 4, DSP_SERIAL_BASE + 8, DSP_SERIAL_BASE + 0x100]
            .contains(&aligned)
        {
            self.analog_hle.reconfigure();
        }
        if aligned == DSP_SERIAL_BASE + 0x0c {
            let addresses = self.registers.get(&(DSP_SERIAL_BASE + 4)).copied().unwrap_or(0);
            let extension = self.registers.get(&(DSP_SERIAL_BASE + 8)).copied().unwrap_or(0);
            let fifo = self.registers.get(&(DSP_SERIAL_BASE + 0x100)).copied().unwrap_or(0);
            let control_size = if address == aligned { size } else { 0 };
            self.analog_hle.control(self.dsp_boot.runtime_hle_active(), control_size, value,
                addresses, extension, fifo);
        }
        if aligned == DSP_SERIAL_BASE + 0x14 {
            // The original ISR XORs 0x3fc with the sampled status and writes
            // it back: device-owned status bits 2..9 are write-zero-to-clear.
            let previous = self.registers.get(&aligned).copied().unwrap_or(0);
            let status_mask = mask & DSP_SERIAL_STATUS_MASK;
            let ordinary_mask = mask & !DSP_SERIAL_STATUS_MASK;
            let ordinary = (previous & !ordinary_mask) | (written & ordinary_mask);
            let status = (previous & DSP_SERIAL_STATUS_MASK) & (!status_mask | written);
            self.registers.insert(aligned, (ordinary & !DSP_SERIAL_STATUS_MASK) | status);
            if status & DSP_SERIAL_TX_COMPLETE == 0
                && self.dsp_queue_hle_phase == DspQueueHlePhase::CompleteSignaled
            {
                self.dsp_queue_hle_phase = DspQueueHlePhase::Idle;
            }
            return None;
        }
        let updated = if aligned == SOC_TIMER + 8 {
            // Bits 7 and 8 are device-owned interrupt status with write-zero-
            // to-clear semantics. The remaining bits are ordinary writable
            // timer mode/enable fields.
            let previous = self.registers.get(&aligned).copied().unwrap_or(0);
            let ordinary_mask = mask & !SYSTEM_TIMER_STATUS_MASK;
            let status_mask = mask & SYSTEM_TIMER_STATUS_MASK;
            let ordinary = (previous & !ordinary_mask) | (written & ordinary_mask);
            let previous_status = previous & SYSTEM_TIMER_STATUS_MASK;
            let status = previous_status & (!status_mask | written);
            let updated = (ordinary & !SYSTEM_TIMER_STATUS_MASK) | status;
            self.registers.insert(aligned, updated);
            updated
        } else {
            let register = self.registers.entry(aligned).or_default();
            *register = (*register & !mask) | written;
            *register
        };
        if aligned == DMA_BASE + 0x30
            || ((DMA_BASE + 0x110..=DMA_BASE + 0x1f0).contains(&aligned)
                && (aligned - DMA_BASE - 0x110) % 0x20 == 0)
        {
            let now = self.instructions.saturating_add(self.fast_forwarded_instructions);
            for channel in 0..8 {
                if let Some((_, count)) = self.dma_uart0_tx(channel)
                    .or_else(|| self.dma_sim_tx(channel)).or_else(|| self.dma_sim_rx(channel)) {
                    self.dma_deadlines[channel].get_or_insert(
                        now.saturating_add(count as u64 * DMA_BYTE_INSTRUCTIONS));
                } else {
                    self.dma_deadlines[channel] = None;
                }
            }
        }
        if aligned == SOC_TIMER + 8 && updated & SYSTEM_TIMER_IRQ20_STATUS == 0 {
            // IRQ20 is a peripheral level. The firmware's handler clears the
            // timer's 0x80 status bit before the interrupt controller EOI.
            self.system_timer_irq_pending = false;
        }
        if aligned == SOC_TIMER + 8 && updated & SYSTEM_TIMER_IRQ21_STATUS == 0 {
            // IRQ21 is the early compare in the observed 0x600 periodic mode.
            // The firmware acknowledges it independently with bit 8 clear.
            self.system_timer_match_irq_pending = false;
        }
        if aligned == SOC_INIT_CONTROL_REGISTER && updated & 0x80 != 0 {
            *self.registers.entry(SOC_INIT_STATUS_REGISTER).or_default() |= 0x07;
            if self.active_irq_line == self.timer_irq_index {
                self.timer_irq_pending = false;
            }
            // The per-line dispatcher acknowledges before the IRQ wrapper has
            // necessarily restored an interruptible task context. Remember the
            // EOI, but keep the source in service until CPSR.I is actually clear.
            self.irq_eoi_seen = true;
        }
        self.write_lcd_register(address, size, value);
        None
    }

    fn write_lcd_register(&mut self, address: u32, size: usize, value: u64) {
        if size != 2 {
            return;
        }
        let address = u64::from(address);
        if address == SOC_EXTERNAL_CONTROL_BASE {
            self.lcd_bus_mode = value as u16;
            return;
        }
        if address != SOC_EXTERNAL_CONTROL_BASE + 2 {
            return;
        }
        let value = value as u16;
        if self.lcd_bus_mode == 0 {
            self.lcd_register_index = value;
        } else if self.lcd_bus_mode == 1 && self.lcd_register_index != LCD_GRAM_REGISTER {
            if let Some(register) = self.lcd_registers.get_mut(self.lcd_register_index as usize) {
                *register = value;
            }
            if self.lcd_register_index == 0x21 {
                // Original window routine 2119f1d0 writes x | (y << 8).
                let x = usize::from(value & 0xff);
                let y = usize::from(value >> 8);
                self.lcd_pixel_cursor = if x < LCD_WIDTH && y < LCD_HEIGHT {
                    y * LCD_WIDTH + x
                } else {
                    LCD_PIXEL_COUNT
                };
            }
        } else if self.lcd_bus_mode == 1 && self.lcd_register_index == LCD_GRAM_REGISTER {
            // Original 2119f1d0 packs inclusive x/y bounds into 44/45.
            // Initialization uses 44=af00, 45=db00 and entry mode 3=1030.
            // Cover that x-increasing, then y-increasing mode; other scan
            // directions are not inferred from this firmware path.
            let x0 = usize::from(self.lcd_registers[0x44] & 0xff);
            let x1 = usize::from(self.lcd_registers[0x44] >> 8);
            let y0 = usize::from(self.lcd_registers[0x45] & 0xff);
            let y1 = usize::from(self.lcd_registers[0x45] >> 8);
            let x = self.lcd_pixel_cursor % LCD_WIDTH;
            let y = self.lcd_pixel_cursor / LCD_WIDTH;
            if self.lcd_registers[3] & 0x38 != 0x30
                || x0 > x1 || x1 >= LCD_WIDTH || y0 > y1 || y1 >= LCD_HEIGHT
                || x < x0 || x > x1 || y < y0 || y > y1
            {
                return;
            }
            self.lcd_framebuffer[self.lcd_pixel_cursor] = value;
            self.lcd_pixel_cursor = if x < x1 {
                self.lcd_pixel_cursor + 1
            } else {
                (if y < y1 { y + 1 } else { y0 }) * LCD_WIDTH + x0
            };
            let next_x = self.lcd_pixel_cursor % LCD_WIDTH;
            let next_y = self.lcd_pixel_cursor / LCD_WIDTH;
            self.lcd_registers[0x21] = (next_x | (next_y << 8)) as u16;
            // Retain the report's full-screen-equivalent write counter. Partial
            // updates do not imply that every pixel was refreshed this cycle.
            self.lcd_pixels_since_frame += 1;
            if self.lcd_pixels_since_frame == LCD_PIXEL_COUNT {
                self.lcd_frames = self.lcd_frames.saturating_add(1);
                self.lcd_pixels_since_frame = 0;
            }
        }
    }

}

pub struct E830Machine {
    cpu: Arm946Cpu<E830State>,
    firmware: FirmwareSet,
}

impl E830Machine {
    pub fn new(firmware: FirmwareSet, trace_mmio: bool) -> Result<Self, MachineError> {
        if firmware.cla().len() <= FLASH_ALIAS_OFFSET {
            return Err(MachineError::ClaTooShortForBootMap);
        }
        let delay_loop_pcs = find_counting_delay_loop_pcs(firmware.cla());
        let nested_delay_loop_pcs = find_nested_delay_loop_pcs(firmware.cla());
        let memset_loop_pcs = find_memset_loop_pcs(firmware.cla());
        let fade_delay_loop_pcs = find_fade_delay_loop_pcs(firmware.cla());
        let tfs_source = firmware.tfs_image().or(firmware.tfs_package());
        let tfs_source_size = tfs_source
            .map(|tfs| align_page(tfs.len() as u64).max(E830_TFS_SOURCE_MIN_SIZE))
            .unwrap_or(E830_TFS_SOURCE_MIN_SIZE);
        let eeprom = firmware.eeprom_payload();
        if eeprom.len() > EXTERNAL_FLASH_WINDOW_SIZE as usize - NSF_NOR_OFFSET {
            return Err(MachineError::NsfTooLarge);
        }
        let mut flash = vec![0xff; EXTERNAL_FLASH_WINDOW_SIZE as usize];
        flash[NSF_NOR_OFFSET..NSF_NOR_OFFSET + eeprom.len()].copy_from_slice(eeprom);
        let mut cpu_backend = Arm946Cpu::new_for_arm946_with_data(E830State::new(
            trace_mmio,
            flash,
            delay_loop_pcs,
            nested_delay_loop_pcs,
            memset_loop_pcs,
            fade_delay_loop_pcs,
        ))
        .map_err(|error| unicorn_error("creating the ARM946 execution profile", error))?;
        let cpu = cpu_backend.raw_engine();
        if let Some(image) = firmware.onenand_image() {
            cpu.get_data_mut().onenand.load_image(image.to_vec())
                .map_err(|size| MachineError::InvalidOneNandImage { size })?;
        }

        let flash_size = align_page(firmware.cla().len() as u64);
        map_memory(
            cpu,
            u64::from(FLASH_BASE),
            flash_size,
            Prot::ALL,
            "mapping the CLA working image",
        )?;
        cpu.mem_write(u64::from(FLASH_BASE), firmware.cla())
            .map_err(|error| unicorn_error("loading the CLA working image", error))?;
        // The running image contains writable tables (for example the
        // hau3 sequencer table at 0x2023e064). Keep this execution window
        // writable while the separate 0xea000000 backing copy below remains
        // read-only, so guest updates never modify the supplied CLA file.
        map_memory(
            cpu,
            E830_BOOT_ROM_MIRROR_BASE,
            flash_size,
            Prot::ALL,
            "mapping the E830 boot ROM mirror",
        )?;
        cpu.mem_write(E830_BOOT_ROM_MIRROR_BASE, firmware.cla())
            .map_err(|error| unicorn_error("loading the E830 boot ROM mirror", error))?;
        cpu.mem_protect(
            E830_BOOT_ROM_MIRROR_BASE,
            flash_size,
            Prot::READ | Prot::EXEC,
        )
        .map_err(|error| unicorn_error("protecting the E830 boot ROM mirror", error))?;

        map_memory(cpu, DSP_SHARED_RAM_BASE, DSP_SHARED_RAM_SIZE,
            Prot::READ | Prot::WRITE, "mapping shared DSP data")?;
        map_mmio(cpu, DSP_CONTROL_BASE, DSP_BOOT_MMIO_SIZE)?;
        map_memory(cpu, DSP_AUX_RAM_BASE + DSP_BOOT_MMIO_SIZE,
            DSP_AUX_RAM_SIZE - DSP_BOOT_MMIO_SIZE,
            Prot::READ | Prot::WRITE, "mapping auxiliary DSP data")?;
        map_memory(
            cpu,
            E830_TFS_SOURCE_BASE,
            tfs_source_size,
            Prot::READ | Prot::WRITE,
            "mapping the TFS loader image",
        )?;
        if let Some(tfs_source) = tfs_source {
            cpu.mem_write(E830_TFS_SOURCE_BASE, tfs_source)
                .map_err(|error| unicorn_error("loading the TFS source image", error))?;
        } else {
            let erased_window = vec![0xff; tfs_source_size as usize];
            cpu.mem_write(E830_TFS_SOURCE_BASE, &erased_window)
                .map_err(|error| unicorn_error("initializing the empty TFS window", error))?;
        }
        map_flash_window(cpu)?;

        let low_vector_image = &firmware.cla()[..LOW_VECTOR_SIZE as usize];
        map_memory(
            cpu,
            LOW_VECTOR_BASE,
            LOW_VECTOR_SIZE,
            Prot::ALL,
            "mapping the low vector overlay",
        )?;
        cpu.mem_write(LOW_VECTOR_BASE, low_vector_image)
            .map_err(|error| unicorn_error("loading the low vector overlay", error))?;
        map_memory(cpu, E830_DTCM_BASE, E830_TCM_SIZE, Prot::ALL, "mapping E830 data TCM")?;
        map_memory(
            cpu,
            u64::from(RAM_BASE),
            u64::from(RAM_SIZE),
            Prot::ALL,
            "mapping SDRAM",
        )?;
        let flash_alias = &firmware.cla()[FLASH_ALIAS_OFFSET..];
        let flash_alias_size = align_page(flash_alias.len() as u64);
        map_memory(
            cpu,
            FLASH_ALIAS_BASE,
            flash_alias_size,
            Prot::ALL,
            "mapping the low flash alias",
        )?;
        cpu.mem_write(FLASH_ALIAS_BASE, flash_alias)
            .map_err(|error| unicorn_error("loading the low flash alias", error))?;
        cpu.mem_protect(FLASH_ALIAS_BASE, flash_alias_size, Prot::READ | Prot::EXEC)
            .map_err(|error| unicorn_error("protecting the low flash alias", error))?;
        map_mmio(cpu, SOC_BOOT_REGISTER_BASE, SOC_BOOT_REGISTER_SIZE)?;
        map_mmio(cpu, ONENAND_BASE, ONENAND_SIZE)?;

        map_mmio(cpu, SOC_CONFIG_REGISTER_BASE, SOC_CONFIG_REGISTER_SIZE)?;
        map_mmio(
            cpu,
            SOC_SECONDARY_REGISTER_BASE,
            SOC_SECONDARY_REGISTER_SIZE,
        )?;
        map_mmio(cpu, SOC_POWER_CONTROL_BASE, SOC_POWER_CONTROL_SIZE)?;
        map_mmio(cpu, SOC_MEMORY_CONTROL_BASE, SOC_MEMORY_CONTROL_SIZE)?;
        map_mmio(cpu, SOC_REGISTER_BASE, SOC_REGISTER_SIZE)?;
        map_mmio(cpu, SOC_EXTERNAL_CONTROL_BASE, SOC_EXTERNAL_CONTROL_SIZE)?;
        install_hooks(cpu)?;
        install_tcm_hooks(cpu, firmware.cla())?;

        cpu.reg_write(RegisterARM::CPSR, RESET_CPSR)
            .map_err(|error| unicorn_error("setting reset status", error))?;
        cpu.reg_write(RegisterARM::SP, INITIAL_STACK)
            .map_err(|error| unicorn_error("setting the initial stack", error))?;
        cpu.reg_write(RegisterARM::PC, u64::from(FLASH_BASE))
            .map_err(|error| unicorn_error("setting the reset PC", error))?;

        Ok(Self {
            cpu: cpu_backend,
            firmware,
        })
    }

    pub fn set_breakpoint(&mut self, pc: u32, after_instructions: u64) {
        let state = self.cpu.raw_engine().get_data_mut();
        state.breakpoint = Some((u64::from(pc & !1), after_instructions));
        state.breakpoint_hit = false;
    }

    pub fn sdram_snapshot(&mut self) -> Result<Vec<u8>, MachineError> {
        let mut bytes = vec![0; RAM_SIZE as usize];
        self.cpu.raw_engine().mem_read(u64::from(RAM_BASE), &mut bytes)
            .map_err(|error| unicorn_error("reading SDRAM for a diagnostic capture", error))?;
        Ok(bytes)
    }

    pub fn onenand_snapshot(&mut self) -> Vec<u8> {
        self.cpu.raw_engine().get_data().onenand.snapshot()
    }

    pub fn dsp_memory_snapshot(&mut self, memory_space: usize) -> Option<Vec<u8>> {
        self.cpu.raw_engine().get_data().dsp_boot.memory_snapshot(memory_space)
    }

    /// Copy installer arguments into mapped SDRAM; never modify firmware code.
    pub fn write_installer_buffer(&mut self, address: u32, bytes: &[u8]) -> Result<(), MachineError> {
        if u64::from(address) < u64::from(RAM_BASE)
            || u64::from(address) + bytes.len() as u64 > u64::from(RAM_BASE) + u64::from(RAM_SIZE) {
            return Err(MachineError::InvalidInstallerBuffer);
        }
        self.cpu.raw_engine().mem_write(u64::from(address), bytes)
            .map_err(|error| unicorn_error("writing installer arguments", error))
    }

    /// Invoke an original firmware function from a stopped machine for provisioning.
    /// CPU context and caller stack arguments are restored afterwards. Guest writes
    /// to devices/allocated buffers remain; normal boot never invokes this method.
    pub fn call_firmware(&mut self, function: u32, args: &[u32], budget: u64) -> Result<BootReport, MachineError> {
        let address = function & !1;
        if address < FLASH_BASE || address as usize - FLASH_BASE as usize >= self.firmware.cla().len()
            || args.len() > 16 {
            return Err(MachineError::InvalidGuestCall);
        }
        if budget == 0 { return Err(MachineError::ZeroInstructionBudget); }
        let cpu = self.cpu.raw_engine();
        let context = cpu.context_init().map_err(|error| unicorn_error("saving caller CPU context", error))?;
        let pc = cpu.reg_read(RegisterARM::PC).map_err(|error| unicorn_error("reading caller PC", error))?;
        let cpsr = cpu.reg_read(RegisterARM::CPSR).map_err(|error| unicorn_error("reading caller CPSR", error))?;
        let sp = cpu.reg_read(RegisterARM::SP).map_err(|error| unicorn_error("reading caller SP", error))?;
        let stack_args: Vec<u8> = args.iter().skip(4).flat_map(|arg| arg.to_le_bytes()).collect();
        let mut saved_stack = vec![0; stack_args.len()];
        if !saved_stack.is_empty() {
            cpu.mem_read(sp, &mut saved_stack).map_err(|error| unicorn_error("saving caller stack arguments", error))?;
        }
        let breakpoint = cpu.get_data().breakpoint;
        let hit = cpu.get_data().breakpoint_hit;
        let setup = (|| {
            for (register, value) in [RegisterARM::R0, RegisterARM::R1, RegisterARM::R2, RegisterARM::R3]
                .into_iter().zip(args.iter().copied().chain(std::iter::repeat(0))) {
                cpu.reg_write(register, u64::from(value)).map_err(|error| unicorn_error("setting guest call arguments", error))?;
            }
            if !stack_args.is_empty() {
                cpu.mem_write(sp, &stack_args).map_err(|error| unicorn_error("setting guest stack arguments", error))?;
            }
            cpu.reg_write(RegisterARM::LR, pc | ((cpsr >> 5) & 1))
                .map_err(|error| unicorn_error("setting guest call return", error))?;
            cpu.reg_write(RegisterARM::CPSR, (cpsr & !u64::from(CPSR_THUMB)) | (u64::from(function & 1) << 5))
                .map_err(|error| unicorn_error("setting guest call mode", error))?;
            // Unicorn's PC write derives Thumb state from bit 0.
            cpu.reg_write(RegisterARM::PC, u64::from(function))
                .map_err(|error| unicorn_error("setting guest call entry", error))?;
            cpu.get_data_mut().breakpoint = Some((pc & !1, 0));
            cpu.get_data_mut().breakpoint_hit = false;
            Ok::<(), MachineError>(())
        })();
        let result = setup.and_then(|()| self.run(budget));
        let cpu = self.cpu.raw_engine();
        cpu.context_restore(&context).map_err(|error| unicorn_error("restoring caller CPU context", error))?;
        if !saved_stack.is_empty() {
            cpu.mem_write(sp, &saved_stack).map_err(|error| unicorn_error("restoring caller stack arguments", error))?;
        }
        cpu.get_data_mut().breakpoint = breakpoint;
        cpu.get_data_mut().breakpoint_hit = hit;
        result
    }

    pub fn uart_output(&mut self, channel: usize) -> Option<Vec<u8>> {
        self.cpu.raw_engine().get_data().uarts.get(channel).map(crate::uart::Uart::output)
    }

    pub fn peek_u32(&mut self, address: u32) -> Option<u32> {
        let mut bytes = [0u8; 4];
        self.cpu
            .raw_engine()
            .mem_read(u64::from(address), &mut bytes)
            .ok()?;
        Some(u32::from_le_bytes(bytes))
    }

    pub fn lcd_snapshot_rgb(&mut self) -> Option<Vec<u8>> {
        let state = self.cpu.raw_engine().get_data();
        if state.lcd_frames == 0 {
            return None;
        }
        let mut rgb = Vec::with_capacity(LCD_PIXEL_COUNT * 3);
        for &pixel in &state.lcd_framebuffer {
            rgb.push((((pixel >> 11) & 0x1f) * 255 / 31) as u8);
            rgb.push((((pixel >> 5) & 0x3f) * 255 / 63) as u8);
            rgb.push(((pixel & 0x1f) * 255 / 31) as u8);
        }
        Some(rgb)
    }

    /// Pixel-write revision includes partial UI updates, not just whole screens.
    pub fn lcd_write_revision(&mut self) -> u64 {
        let state = self.cpu.raw_engine().get_data();
        state.lcd_frames.saturating_mul(LCD_PIXEL_COUNT as u64)
            .saturating_add(state.lcd_pixels_since_frame as u64)
    }

    /// Change external phone contacts; the original controller ISR owns events.
    pub fn set_phone_keys(&mut self, keys: u32) {
        let state = self.cpu.raw_engine().get_data_mut();
        state.keypad.set_keys(keys);
        state.enabled_pending_dirty = true;
    }

    pub fn run(&mut self, max_instructions: u64) -> Result<BootReport, MachineError> {
        if max_instructions == 0 {
            return Err(MachineError::ZeroInstructionBudget);
        }
        let cpu = self.cpu.raw_engine();
        let state = cpu.get_data_mut();
        let instructions_so_far = state
            .instructions
            .saturating_add(state.fast_forwarded_instructions);
        state.instruction_budget = instructions_so_far.saturating_add(max_instructions);
        let start_pc = cpu
            .reg_read(RegisterARM::PC)
            .map_err(|error| unicorn_error("reading the resume PC", error))?;
        let start_cpsr = cpu
            .reg_read(RegisterARM::CPSR)
            .map_err(|error| unicorn_error("reading the resume CPSR", error))?;
        let start_address = if start_cpsr & (1 << 5) != 0 {
            start_pc | 1
        } else {
            start_pc
        };
        let execution = cpu.emu_start(start_address, u64::MAX, 0, 0);
        let pc = cpu
            .reg_read(RegisterARM::PC)
            .map_err(|error| unicorn_error("reading PC", error))? as u32;
        let cpsr = cpu
            .reg_read(RegisterARM::CPSR)
            .map_err(|error| unicorn_error("reading CPSR", error))? as u32;
        let r0 = cpu
            .reg_read(RegisterARM::R0)
            .map_err(|error| unicorn_error("reading R0", error))? as u32;
        let r1 = cpu
            .reg_read(RegisterARM::R1)
            .map_err(|error| unicorn_error("reading R1", error))? as u32;
        let r2 = cpu
            .reg_read(RegisterARM::R2)
            .map_err(|error| unicorn_error("reading R2", error))? as u32;
        let r3 = cpu
            .reg_read(RegisterARM::R3)
            .map_err(|error| unicorn_error("reading R3", error))? as u32;
        let r4 = cpu
            .reg_read(RegisterARM::R4)
            .map_err(|error| unicorn_error("reading R4", error))? as u32;
        let r5 = cpu
            .reg_read(RegisterARM::R5)
            .map_err(|error| unicorn_error("reading R5", error))? as u32;
        let r6 = cpu
            .reg_read(RegisterARM::R6)
            .map_err(|error| unicorn_error("reading R6", error))? as u32;
        let r7 = cpu
            .reg_read(RegisterARM::R7)
            .map_err(|error| unicorn_error("reading R7", error))? as u32;
        let sp = cpu
            .reg_read(RegisterARM::SP)
            .map_err(|error| unicorn_error("reading SP", error))? as u32;
        let lr = cpu
            .reg_read(RegisterARM::LR)
            .map_err(|error| unicorn_error("reading LR", error))? as u32;
        let mut stack_bytes = [0u8; 64];
        let stack_words = if cpu.mem_read(u64::from(sp), &mut stack_bytes).is_ok() {
            stack_bytes
                .chunks(4)
                .map(|bytes| u32::from_le_bytes(bytes.try_into().expect("four-byte stack word")))
                .collect()
        } else {
            Vec::new()
        };

        let state = cpu.get_data();
        let stop_reason = match execution {
            Ok(()) if state.breakpoint_hit => StopReason::Breakpoint,
            Ok(()) => StopReason::InstructionBudget,
            Err(error) => StopReason::CpuError(format!("{error:?}")),
        };

        Ok(BootReport {
            instructions: state
                .instructions
                .saturating_add(state.fast_forwarded_instructions),
            fast_forwarded_instructions: state.fast_forwarded_instructions,
            pc,
            cpsr,
            r0,
            r1,
            r2,
            r3,
            sp,
            r4,
            r5,
            r6,
            r7,
            lr,
            stop_reason,
            stack_words,
            memory_fault: state.memory_fault.clone(),
            recent_mmio: state.recent_mmio.iter().cloned().collect(),
            eeprom_size: self.firmware.eeprom_payload().len(),
            tfs_size: self.firmware.tfs_image().or(self.firmware.tfs_package()).map(<[u8]>::len),
            nor_size: state.nor.len(),
            onenand_image_size: self.firmware.onenand_image().map(<[u8]>::len),
            lcd_frames: state.lcd_frames,
            keyboard_init_dispatches: state.keyboard_init_dispatches,
            standby_look_dispatches: state.standby_look_dispatches,
            irqs_taken: state.irqs_taken,
        })
    }
}

fn align_page(size: u64) -> u64 {
    (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

fn map_memory(
    cpu: &mut Unicorn<'static, E830State>,
    address: u64,
    size: u64,
    permissions: Prot,
    operation: &'static str,
) -> Result<(), MachineError> {
    cpu.mem_map(address, size, permissions)
        .map_err(|error| unicorn_error(operation, error))
}

fn map_mmio(
    cpu: &mut Unicorn<'static, E830State>,
    base: u64,
    size: u64,
) -> Result<(), MachineError> {
    cpu.mmio_map(
        base,
        size,
        Some(
            move |uc: &mut Unicorn<'_, E830State>, offset, access_size| {
                let address = (base + offset) as u32;
                let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                let value = uc.get_data_mut().read_register(address, access_size, pc);
                uc.get_data_mut().record_mmio(MmioAccess {
                    write: false,
                    address,
                    size: access_size,
                    value,
                    pc,
                });
                value
            },
        ),
        Some(
            move |uc: &mut Unicorn<'_, E830State>, offset, access_size, value| {
                let address = (base + offset) as u32;
                let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                let event = {
                    let state = uc.get_data_mut();
                    let event = state.write_register(address, access_size, value);
                    state.record_mmio(MmioAccess {
                        write: true,
                        address,
                        size: access_size,
                        value,
                        pc,
                    });
                    event
                };
                if address == DSP_SIGNAL_BASE && access_size == 2 && value == 0x40 {
                    let (enabled, runtime_active, now, phase) = {
                        let state = uc.get_data();
                        let now = state.instructions.saturating_add(state.fast_forwarded_instructions);
                        let phase = state.rf_timer_phase_clocks(now);
                        if std::env::var_os("E830_TRACE_RF_REJECT").is_some() {
                            let register = |offset| state.registers.get(&(SOC_TIMER + offset))
                                .copied().unwrap_or(0);
                            eprintln!("[e830-dsp-rf-gate] time={now} pc=0x{pc:08x} enabled={} runtime={} hle_phase={:?} phase_clocks={phase:?} timer_status=0x{:08x} timer_compare0=0x{:08x} timer_compare1=0x{:08x} timer_compare2=0x{:08x} timer_raw={} timer_remainder={} timer_last={}",
                                state.dsp_rf.enabled,
                                state.dsp_boot.runtime_hle_active(),
                                state.dsp_boot.hle_phase,
                                register(8), register(0x0c), register(0x10), register(0x14),
                                state.timer_counter, state.timer_clock_remainder,
                                state.timer_last_update);
                        }
                        (state.dsp_rf.enabled, state.dsp_boot.runtime_hle_active(), now, phase)
                    };
                    if enabled && runtime_active {
                        if let Ok(bytes) = uc.mem_read_as_vec(crate::dsp_rf::SNAPSHOT_BASE,
                            crate::dsp_rf::SNAPSHOT_SIZE)
                        {
                            if let Some(phase) = phase {
                                let state = uc.get_data_mut();
                                let count = state.dsp_rf.submit(bytes, now, phase,
                                    SYSTEM_TIMER_CPU_CLOCK_RATIO << SYSTEM_TIMER_FRACTION_BITS);
                                if count != 0 {
                                    eprintln!("[e830-dsp-rf-hle] submit empty-air slots={count} time={now} phase_clocks={phase} timer_raw={} timer_remainder={} timer_last={}",
                                        state.timer_counter, state.timer_clock_remainder, state.timer_last_update);
                                }
                            }
                        }
                    }
                }
                if std::env::var_os("E830_TRACE_DSP_TX").is_some()
                    && address == DSP_CONTROL_BASE as u32 + 8
                    && matches!(value as u16, 0x1001 | 0x1002 | 0x2001 | 0x4001)
                {
                    let mailbox = uc.mem_read_as_vec(DSP_MAILBOX_BASE, 0x90).unwrap_or_default();
                    let halfword = |offset: usize| -> u16 {
                        mailbox.get(offset..offset + 2)
                            .and_then(|slice| slice.try_into().ok())
                            .map(u16::from_le_bytes)
                            .unwrap_or(0)
                    };
                    eprintln!(
                        "[e830-dsp-mailbox] pc=0x{pc:08x} tx=0x{:04x} m14=0x{:04x} m7c=0x{:04x} m7e=0x{:04x} m80=0x{:04x} m82=0x{:04x}",
                        value as u16,
                        halfword(0x14),
                        halfword(0x7c),
                        halfword(0x7e),
                        halfword(0x80),
                        halfword(0x82),
                    );
                }
                if event == Some(DspEvent::StartupReady) {
                    let _ = uc.mem_write(
                        DSP_SHARED_RAM_BASE,
                        &E830_DSP_STARTUP_MAGIC.to_le_bytes(),
                    );
                }
            },
        ),
    )
    .map_err(|error| unicorn_error("mapping a device register bank", error))
}

fn map_flash_window(cpu: &mut Unicorn<'static, E830State>) -> Result<(), MachineError> {
    let base = EXTERNAL_FLASH_WINDOW_BASE;
    cpu.mmio_map(
        base,
        EXTERNAL_FLASH_WINDOW_SIZE,
        Some(move |uc: &mut Unicorn<'_, E830State>, offset, size| {
            let address = (base + offset) as u32;
            let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
            let state = uc.get_data_mut();
            let value = state.nor.read(offset, size);
            state.record_mmio(MmioAccess {
                write: false,
                address,
                size,
                value,
                pc,
            });
            value
        }),
        Some(
            move |uc: &mut Unicorn<'_, E830State>, offset, size, value| {
                let address = (base + offset) as u32;
                let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                let state = uc.get_data_mut();
                state.nor.write(offset, size, value);
                state.record_mmio(MmioAccess {
                    write: true,
                    address,
                    size,
                    value,
                    pc,
                });
            },
        ),
    )
    .map_err(|error| unicorn_error("mapping the external flash interface", error))
}
const COUNTING_DELAY_LOOP: [u8; 10] = [0x01, 0x32, 0x01, 0x30, 0x88, 0x42, 0xfb, 0xd3, 0x70, 0x47];

const LOOP_COUNTING: u8 = 1;
const LOOP_NESTED: u8 = 2;
const LOOP_MEMSET: u8 = 4;
const LOOP_FADE: u8 = 8;

fn collect_loop_kinds(sets: [HashSet<u64>; 4]) -> Vec<(u64, u8)> {
    let mut kinds = HashMap::new();
    for (index, pcs) in sets.into_iter().enumerate() {
        for pc in pcs {
            // Signatures can overlap (notably nested/fade delays). Preserve
            // every former membership result, including handler priority.
            *kinds.entry(pc).or_insert(0) |= 1 << index;
        }
    }
    let mut kinds: Vec<_> = kinds.into_iter().collect();
    kinds.sort_unstable_by_key(|(pc, _)| *pc);
    kinds
}

fn loop_kinds_at(kinds: &[(u64, u8)], address: u64) -> u8 {
    kinds.binary_search_by_key(&address, |(pc, _)| *pc)
        .map(|index| kinds[index].1)
        .unwrap_or(0)
}

fn find_counting_delay_loop_pcs(cla: &[u8]) -> HashSet<u64> {
    cla.windows(COUNTING_DELAY_LOOP.len())
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes == COUNTING_DELAY_LOOP.as_slice())
                .then_some(u64::from(FLASH_BASE) + offset as u64 + 2)
        })
        .collect()
}

const NESTED_DELAY_LOOP: [u8; 16] = [
    0x10, 0x1c, 0xc0, 0x46, 0x01, 0x38, 0xfc, 0xd1, 0x01, 0x39, 0x00, 0x29, 0xf8, 0xd1, 0x04, 0xe0,
];

fn find_nested_delay_loop_pcs(cla: &[u8]) -> HashSet<u64> {
    cla.windows(NESTED_DELAY_LOOP.len())
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes == NESTED_DELAY_LOOP.as_slice())
                .then_some(u64::from(FLASH_BASE) + offset as u64 + 2)
        })
        .collect()
}

fn fast_forward_nested_delay(r0: u32, r1: u32, remaining_instructions: u64) -> Option<u64> {
    if r0 == 0 || r1 == 0 {
        return None;
    }
    let loop_instructions = u64::from(r0).checked_mul(3)?;
    (loop_instructions <= remaining_instructions).then_some(loop_instructions - 1)
}

const ZERO_FILL_LOOP: [u8; 16] = [
    0x0c, 0x50, 0xa0, 0x28, 0x0c, 0x50, 0xa0, 0x28, 0x20, 0x10, 0x51, 0x22, 0xfb, 0xff, 0xff, 0x2a,
];

const FADE_DELAY_LOOP: [u8; 6] = [0xc0, 0x46, 0x01, 0x38, 0xfc, 0xd1];

// Splash LCD-init table delay: `mov r8,r8; subs r0,#1; bne .-2`. The inner
// countdown (r0, reloaded from r2 each outer pass) is a pure software delay the
// firmware runs between LCD register writes while holding the boot logo. The PC
// registered is the `mov r8,r8` at the top of the inner loop; the fast-forward
// handler collapses the r0 countdown and resumes at the `subs r1,#1` six bytes
// on. Distinct byte shape from the earlier delay loops, so it needs its own
// detector.
fn find_fade_delay_loop_pcs(cla: &[u8]) -> HashSet<u64> {
    cla.windows(FADE_DELAY_LOOP.len())
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes == FADE_DELAY_LOOP.as_slice()).then_some(u64::from(FLASH_BASE) + offset as u64)
        })
        .collect()
}

fn find_memset_loop_pcs(cla: &[u8]) -> HashSet<u64> {
    cla.windows(ZERO_FILL_LOOP.len())
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes == ZERO_FILL_LOOP.as_slice()).then_some(u64::from(FLASH_BASE) + offset as u64)
        })
        .collect()
}

struct ZeroFillPlan {
    bytes: usize,
    new_r0: u32,
    new_r1: u32,
    new_cpsr: u32,
    skipped_instructions: u64,
}

fn plan_zero_fill(
    r0: u32,
    r1: u32,
    cpsr: u32,
    r2: u32,
    r3: u32,
    r12: u32,
    lr: u32,
) -> Option<ZeroFillPlan> {
    if r2 != 0 || r3 != 0 || r12 != 0 || lr != 0 {
        return None;
    }

    let carry_set = cpsr & (1 << 29) != 0;
    if !carry_set {
        return Some(ZeroFillPlan {
            bytes: 0,
            new_r0: r0,
            new_r1: r1,
            new_cpsr: cpsr,
            skipped_instructions: 4,
        });
    }

    let iterations = u64::from(r1 / 32) + 1;
    let byte_count = iterations.checked_mul(32)?;
    if byte_count > u64::from(RAM_SIZE) {
        return None;
    }
    let bytes = usize::try_from(byte_count).ok()?;
    let byte_count_u32 = u32::try_from(byte_count).ok()?;
    let new_r0 = r0.checked_add(byte_count_u32)?;
    let new_r1 = r1.wrapping_sub(byte_count_u32);
    let new_cpsr = (cpsr & !0xf000_0000) | 0x8000_0000;

    Some(ZeroFillPlan {
        bytes,
        new_r0,
        new_r1,
        new_cpsr,
        skipped_instructions: iterations.checked_mul(4)?,
    })
}

fn fast_forward_counting_delay(
    r0: u32,
    r1: u32,
    r2: u32,
    max_skipped_instructions: u64,
) -> Option<(u32, u32, u64)> {
    if r0 >= r1 {
        return None;
    }
    let skipped_iterations = u64::from(r1 - r0 - 1).min(max_skipped_instructions / 4);
    if skipped_iterations == 0 {
        return None;
    }
    let skipped_iterations = u32::try_from(skipped_iterations).ok()?;
    Some((
        r0 + skipped_iterations,
        r2.wrapping_add(skipped_iterations),
        u64::from(skipped_iterations) * 4,
    ))
}

fn install_tcm_hooks(cpu: &mut Unicorn<'static, E830State>, image: &[u8]) -> Result<(), MachineError> {
    for (index, bytes) in image.chunks_exact(4).enumerate() {
        let instruction = u32::from_le_bytes(bytes.try_into().expect("ARM instruction word"));
        if !TcmRegisters::recognizes(instruction) { continue; }
        let offset = index as u64 * 4;
        for address in [u64::from(FLASH_BASE) + offset, offset, E830_BOOT_ROM_MIRROR_BASE + offset] {
            cpu.add_code_hook(address, address, |uc, pc, size| {
                let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
                if size != 4 || cpsr & CPSR_THUMB != 0 || uc.get_data().breakpoint_hit {
                    return;
                }
                let mut bytes = [0; 4];
                if uc.mem_read(pc, &mut bytes).is_err() { return; }
                let instruction = u32::from_le_bytes(bytes);
                if !TcmRegisters::recognizes(instruction)
                    || !arm_condition_passed(instruction >> 28, cpsr) { return; }
                let rd = (instruction >> 12) & 15;
                let register = [RegisterARM::R0, RegisterARM::R1, RegisterARM::R2, RegisterARM::R3,
                    RegisterARM::R4, RegisterARM::R5, RegisterARM::R6, RegisterARM::R7,
                    RegisterARM::R8, RegisterARM::R9, RegisterARM::R10, RegisterARM::R11,
                    RegisterARM::R12, RegisterARM::SP, RegisterARM::LR, RegisterARM::PC][rd as usize];
                let input = if rd == 15 { pc as u32 + 8 } else {
                    uc.reg_read(register).unwrap_or(0) as u32
                };
                if let Some(output) = uc.get_data_mut().tcm_registers.execute(instruction, input) {
                    if rd == 15 {
                        let _ = uc.reg_write(RegisterARM::CPSR,
                            u64::from((cpsr & 0x0fff_ffff) | (output & 0xf000_0000)));
                    } else {
                        let _ = uc.reg_write(register, u64::from(output));
                    }
                }
                // Keep Unicorn's architectural execution for all other ARM
                // instructions. These TCM registers are a board extension.
                let _ = uc.reg_write(RegisterARM::PC, pc + 4);
            }).map_err(|error| unicorn_error("installing E830 TCM register support", error))?;
        }
    }
    Ok(())
}

fn handle_internal_flash_command_write(
    uc: &mut Unicorn<'_, E830State>,
    pc: u64,
) -> bool {
    let (address_register, value_register, expected_address, expected_value) = match pc {
        0x31f0_0034 | 0x2189_a528 | 0x0189_a528 | 0xeb89_a528 => {
            (RegisterARM::R3, RegisterARM::R1, 0x2000_0aaau32, 0x00aau16)
        }
        0x31f0_0038 | 0x2189_a52c | 0x0189_a52c | 0xeb89_a52c => {
            (RegisterARM::R1, RegisterARM::R2, 0x2000_0554u32, 0x0055u16)
        }
        0x31f0_003c | 0x2189_a530 | 0x0189_a530 | 0xeb89_a530 => {
            (RegisterARM::R1, RegisterARM::R0, 0x2008_2aaau32, 0x00c0u16)
        }
        _ => return false,
    };
    let target = uc.reg_read(address_register).unwrap_or(u64::MAX) as u32;
    let value = uc.reg_read(value_register).unwrap_or(u64::MAX) as u16;
    if target != expected_address || value != expected_value {
        return false;
    }
    if std::env::var_os("E830_TRACE_FLASH_COMMANDS").is_some() {
        eprintln!(
            "[e830-flash-command] pc=0x{pc:08x} address=0x{target:08x} value=0x{value:04x}"
        );
    }
    // These are AMD-style command cycles to the internal firmware flash, not
    // array programming.  Keep the immutable CLA bytes intact and consume the
    // store just as the external NOR command model consumes unlock commands.
    // Unicorn derives Thumb state from bit 0 on a PC write.  Resume at the
    // following halfword while keeping the copied RAM routine in Thumb mode.
    uc.reg_write(RegisterARM::PC, pc + 3).is_ok()
}

fn install_hooks(cpu: &mut Unicorn<'static, E830State>) -> Result<(), MachineError> {
    cpu.add_code_hook(0, u64::MAX, |uc, address, _| {

        let (
            is_delay_loop,
            is_nested_delay_loop,
            is_memset_loop,
            is_fade_delay_loop,
            remaining_before,
        ) = {
            let state = uc.get_data_mut();
            let total = state
                .instructions
                .saturating_add(state.fast_forwarded_instructions);
            if state.breakpoint.is_some_and(|(pc, after)| address == pc && total >= after) {
                state.breakpoint_hit = true;
                (false, false, false, false, 0)
            } else if total >= state.instruction_budget {
                (false, false, false, false, 0)
            } else {
                let remaining = state.instruction_budget - total;
                state.instructions = state.instructions.saturating_add(1);
                if state.pc_log_remaining > 0 && total >= state.pc_log_after {
                    state.pc_log_remaining -= 1;
                    eprintln!("[e830-pc] 0x{address:08x}");
                }
                let kinds = loop_kinds_at(&state.loop_kinds, address);
                (
                    kinds & LOOP_COUNTING != 0,
                    kinds & LOOP_NESTED != 0,
                    kinds & LOOP_MEMSET != 0,
                    kinds & LOOP_FADE != 0,
                    remaining,
                )
            }
        };
        if remaining_before == 0 {
            let _ = uc.emu_stop();
            return;
        }

        if handle_internal_flash_command_write(uc, address) {
            return;
        }

        let total = uc.get_data().instructions.saturating_add(uc.get_data().fast_forwarded_instructions);
        if total >= uc.get_data().task_pc_log_after
            && uc.get_data().task_pc_log_remaining > 0
            && uc.get_data().task_pc_log_target.is_some()
        {
            let current = diagnostic_ram_read(uc, 0x3090_0194, 4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_le_bytes)
                .unwrap_or(0);
            let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0);
            if uc.get_data().task_pc_log_target == Some(current) && cpsr & 0x1f == 0x1f {
                eprintln!(
                    "[e830-task-pc] task=0x{current:08x} pc=0x{address:08x} insns={total} cpsr=0x{cpsr:08x}"
                );
                uc.get_data_mut().task_pc_log_remaining -= 1;
            }
        }
        if address == STATE_TABLE_DISPATCH_PC {
            let table = dispatch_table(uc).unwrap_or(0);
            let (keyboard, standby) = state_dispatch_kind(address, table);
            let state = uc.get_data_mut();
            state.keyboard_init_dispatches = state.keyboard_init_dispatches.saturating_add(u64::from(keyboard));
            state.standby_look_dispatches = state.standby_look_dispatches.saturating_add(u64::from(standby));
        }
        if uc.get_data().trace_tasks
            && total >= uc.get_data().trace_tasks_after
            && uc.get_data().trace_tasks_remaining > 0
            && uc.get_data().trace_tasks_pcs.contains(&address)
        {
            let word = |at: u64| -> Option<u32> {
                diagnostic_ram_read(uc, at, 4)
                    .and_then(|bytes| bytes.try_into().ok()).map(u32::from_le_bytes)
            };
            let current = word(0x3090_0194).unwrap_or(0);
            let next = if address == 0x204b_01d8 {
                uc.reg_read(RegisterARM::R1).unwrap_or(0) as u32
            } else {
                0
            };
            let target = uc.get_data().trace_tasks_target;
            let send_target = if address == 0x2084_7a02 {
                uc.reg_read(RegisterARM::R5).unwrap_or(0) as u32
            } else {
                0
            };
            if target.is_none_or(|target| current == target || next == target || send_target == target) {
                let task = diagnostic_ram_read(uc, u64::from(current), 0x38).unwrap_or_default();
                let task_words: Vec<String> = task.chunks_exact(4)
                    .map(|bytes| format!("{:08x}", u32::from_le_bytes(bytes.try_into().unwrap())))
                    .collect();
                let registers = [RegisterARM::R0, RegisterARM::R1, RegisterARM::R4,
                    RegisterARM::R5, RegisterARM::R6, RegisterARM::R7, RegisterARM::SP,
                    RegisterARM::LR, RegisterARM::CPSR];
                let values: Vec<String> = registers.into_iter().map(|register|
                    format!("{register:?}=0x{:08x}", uc.reg_read(register).unwrap_or(0))).collect();
                let object = uc.reg_read(if address == 0x2084_7d14 {
                    RegisterARM::R4
                } else { RegisterARM::R7 }).unwrap_or(0);
                let object_words: Vec<String> = (0..4).map(|index|
                    word(object.saturating_add(index * 4)).map(|value| format!("{value:08x}"))
                        .unwrap_or_else(|| "????????".into())).collect();
                let node = uc.reg_read(RegisterARM::R5).unwrap_or(0);
                let node_words: Vec<String> = (0..5).map(|index|
                    word(node.saturating_add(index * 4)).map(|value| format!("{value:08x}"))
                        .unwrap_or_else(|| "????????".into())).collect();
                let argument = uc.reg_read(RegisterARM::R0).unwrap_or(0);
                let argument_words: Vec<String> = (0..3).map(|index|
                    word(argument.saturating_add(index * 4)).map(|value| format!("{value:08x}"))
                        .unwrap_or_else(|| "????????".into())).collect();
                let saved_pc = |task: u32| -> Option<u32> {
                    let saved_sp = word(u64::from(task) + 4)?;
                    word(u64::from(saved_sp) + 56)
                };
                eprintln!(
                    "[e830-task] pc=0x{address:08x} insns={total} current=0x{current:08x} next=0x{next:08x} saved_pc={:?} next_saved_pc={:?} task=[{}] object=[{}] node=[{}] arg=[{}] {}",
                    saved_pc(current).map(|pc| format!("0x{pc:08x}")),
                    saved_pc(next).map(|pc| format!("0x{pc:08x}")),
                    task_words.join(","), object_words.join(","), node_words.join(","),
                    argument_words.join(","), values.join(" ")
                );
                uc.get_data_mut().trace_tasks_remaining -= 1;
            }
        }
        if address == DSP_ASYNC_EVENT_HANDLER_PC
            && uc.get_data().dsp_boot.runtime_hle_active()
            && uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32 == DSP_ASYNC_QUEUE
            && matches!(uc.reg_read(RegisterARM::R1).unwrap_or(0), 1 | 2)
        {
            let head = diagnostic_ram_read(uc, u64::from(DSP_ASYNC_QUEUE + 4), 4)
                .and_then(|bytes| bytes.try_into().ok())
                .map(u32::from_le_bytes).unwrap_or(0);
            let callback = if head != 0 {
                diagnostic_ram_read(uc, u64::from(head) + 0x10, 4)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u32::from_le_bytes).unwrap_or(0)
            } else { 0 };
            if head != 0 && callback == DSP_ASYNC_CALLBACK {
                // Let the original start handler fill the transmit registers
                // and FIFO; signal completion later through its real IRQ9.
                let state = uc.get_data_mut();
                state.dsp_queue_hle_phase = DspQueueHlePhase::CompletePending {
                    deadline: total.saturating_add(DSP_ASYNC_COMPLETE_DELAY),
                };
                if std::env::var_os("E830_TRACE_DSP_TX").is_some() {
                    eprintln!("[e830-dsp-serial] submit head=0x{head:08x} insns={total}");
                }
            }
        }

        trace_menu_input_stage(uc, address, total);

        if address == 0x20d9_225a {
            hkbd_hle_unblock_up(uc);
        }

        if let Some(limit) = uc.get_data().trace_sp_below {
            let sp = uc.reg_read(RegisterARM::SP).unwrap_or(0) as u32;
            let (last_sp, last_pc, after, remaining) = {
                let state = uc.get_data();
                (state.trace_sp_last, state.trace_sp_last_pc, state.trace_sp_after,
                    state.trace_sp_remaining)
            };
            if total >= after && remaining > 0 && sp < limit && last_sp != Some(sp) {
                let lr = uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;
                let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
                eprintln!(
                    "[e830-sp] pc=0x{address:08x} prev_pc=0x{last_pc:08x} sp=0x{sp:08x} prev_sp=0x{:08x} lr=0x{lr:08x} cpsr=0x{cpsr:08x} insns={total}",
                    last_sp.unwrap_or(0)
                );
                uc.get_data_mut().trace_sp_remaining -= 1;
            }
            let state = uc.get_data_mut();
            state.trace_sp_last = Some(sp);
            state.trace_sp_last_pc = address;
        }
        if !uc.get_data().watch_pcs.is_empty()
            && total >= uc.get_data().watch_after
            && uc.get_data().watch_remaining > 0
            && uc.get_data().watch_pcs.contains(&address)
        {
            let registers = [RegisterARM::R0, RegisterARM::R1, RegisterARM::R2, RegisterARM::R3,
                RegisterARM::R4, RegisterARM::R5, RegisterARM::R6, RegisterARM::R7,
                RegisterARM::SP, RegisterARM::LR, RegisterARM::CPSR];
            let values: Vec<(RegisterARM, u64)> = registers.into_iter().map(|register|
                (register, uc.reg_read(register).unwrap_or(0))).collect();
            let formatted: Vec<String> = values.iter().map(|(register, value)|
                format!("{register:?}=0x{value:08x}")).collect();
            let pointers: Vec<String> = values.iter().filter_map(|(register, value)| {
                if matches!(register, RegisterARM::SP | RegisterARM::LR | RegisterARM::CPSR) {
                    return None;
                }
                let words: Vec<String> = (0..4).map(|index| {
                    diagnostic_ram_read(uc, value.saturating_add(index * 4), 4)
                        .and_then(|bytes| bytes.try_into().ok())
                        .map(u32::from_le_bytes)
                        .map(|word| format!("{word:08x}"))
                        .unwrap_or_else(|| "????????".into())
                }).collect();
                (words.iter().any(|word| word != "????????"))
                    .then(|| format!("{register:?}->[{}]", words.join(",")))
            }).collect();
            eprintln!(
                "[e830-watch] pc=0x{address:08x} insns={total} {} ptrs={}",
                formatted.join(" "), pointers.join(" ")
            );
            uc.get_data_mut().watch_remaining -= 1;
        }
        if uc.get_data().trace_dsp && DSP_TRACE_PCS.contains(&address) {
            let should_log = {
                let state = uc.get_data();
                state.trace_dsp
                    && total >= state.trace_dsp_after
                    && state.trace_dsp_remaining > 0
            };
            if should_log {
                let mailbox = uc.mem_read_as_vec(DSP_MAILBOX_BASE, 64).unwrap_or_default();
                let word = |bytes: &[u8], offset: usize| -> u32 {
                    bytes.get(offset..offset + 4)
                        .and_then(|slice| slice.try_into().ok())
                        .map(u32::from_le_bytes)
                        .unwrap_or(0)
                };
                let r0 = uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32;
                let r1 = uc.reg_read(RegisterARM::R1).unwrap_or(0) as u32;
                let r4 = uc.reg_read(RegisterARM::R4).unwrap_or(0) as u32;
                let r6 = uc.reg_read(RegisterARM::R6).unwrap_or(0) as u32;
                let sp = uc.reg_read(RegisterARM::SP).unwrap_or(0) as u32;
                let lr = uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;
                let control = uc.get_data().dsp_boot.snapshot();
                let values = [
                    control[0], control[1], control[2], control[3],
                    word(&mailbox, 0), word(&mailbox, 0x10), word(&mailbox, 0x14),
                    word(&mailbox, 0x18),
                ];
                let changed = uc.get_data().trace_dsp_last
                    .map(|(last_pc, last_values)| last_pc != address || last_values != values)
                    .unwrap_or(true);
                if changed {
                    eprintln!(
                        "[e830-dsp] pc=0x{address:08x} ctl0=0x{:08x} ctl4=0x{:08x} ctl8=0x{:08x} ctlc=0x{:08x} mbx640=0x{:08x} mbx650=0x{:08x} mbx654=0x{:08x} mbx658=0x{:08x} r0=0x{r0:08x} r1=0x{r1:08x} r4=0x{r4:08x} r6=0x{r6:08x} sp=0x{sp:08x} lr=0x{lr:08x} insns={total}",
                        values[0], values[1], values[2], values[3], values[4], values[5],
                        values[6], values[7]
                    );
                    let state = uc.get_data_mut();
                    state.trace_dsp_last = Some((address, values));
                    state.trace_dsp_remaining -= 1;
                }
            }
        }

        if uc.get_data().trace_wake
            && ((WAKE_WAIT_TRACE_START..=WAKE_WAIT_TRACE_END).contains(&address)
                || WAKE_IRQ_TRACE_PCS.contains(&address))
        {
            let total = uc.get_data().instructions.saturating_add(uc.get_data().fast_forwarded_instructions);
            let after = uc.get_data().trace_wake_after;
            if total >= after && uc.get_data().trace_wake_remaining > 0 {
                let r0 = uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32;
                let r1 = uc.reg_read(RegisterARM::R1).unwrap_or(0) as u32;
                let r4 = uc.reg_read(RegisterARM::R4).unwrap_or(0) as u32;
                let r5 = uc.reg_read(RegisterARM::R5).unwrap_or(0) as u32;
                let sp = uc.reg_read(RegisterARM::SP).unwrap_or(0) as u32;
                let lr = uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;
                let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
                uc.get_data_mut().trace_wake_line(format!(
                    "pc=0x{address:08x} r0=0x{r0:08x} r1=0x{r1:08x} r4=0x{r4:08x} r5=0x{r5:08x} sp=0x{sp:08x} lr=0x{lr:08x} cpsr=0x{cpsr:08x} insns={total}"
                ));
            }
        }

        if uc.get_data().trace_timer && TIMER_TRACE_PCS.contains(&address) {
            let total = uc.get_data().instructions.saturating_add(uc.get_data().fast_forwarded_instructions);
            let after = uc.get_data().trace_timer_after;
            if total >= after && uc.get_data().trace_timer_remaining > 0 {
                let r0 = uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32;
                let r1 = uc.reg_read(RegisterARM::R1).unwrap_or(0) as u32;
                let sp = uc.reg_read(RegisterARM::SP).unwrap_or(0) as u32;
                let lr = uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;
                let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
                let state = uc.get_data();
                let mask = state.registers.get(&INTC_ENABLE_MASK_REGISTER).copied().unwrap_or(u32::MAX);
                let control = state.registers.get(&(SOC_TIMER + 8)).copied().unwrap_or(0);
                let period = state.registers.get(&(SOC_TIMER + 0x0c)).copied().unwrap_or(0);
                let match0 = state.registers.get(&(SOC_TIMER + 0x10)).copied().unwrap_or(0);
                let match1 = state.registers.get(&(SOC_TIMER + 0x14)).copied().unwrap_or(0);
                uc.get_data_mut().trace_timer_line(format!(
                    "pc=0x{address:08x} r0=0x{r0:08x} r1=0x{r1:08x} sp=0x{sp:08x} lr=0x{lr:08x} cpsr=0x{cpsr:08x} mask=0x{mask:08x} ctrl=0x{control:08x} period=0x{period:08x} match0=0x{match0:08x} match1=0x{match1:08x} insns={total}"
                ));
            }
        }

        // Diagnostic: pin down whether/where early boot unmasks interrupts.
        {
            let trace = uc.get_data().trace_irqen;
            if trace {
                let (logged_i, logged_f) = {
                    let s = uc.get_data();
                    (s.irqen_logged_i, s.irqen_logged_f)
                };
                if !logged_i || !logged_f {
                    let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
                    if !logged_i && cpsr & CPSR_IRQ_DISABLE == 0 {
                        let total = {
                            let s = uc.get_data();
                            s.instructions.saturating_add(s.fast_forwarded_instructions)
                        };
                        eprintln!(
                            "[e830-irqen] IRQ unmasked first at pc=0x{address:08x} cpsr=0x{cpsr:08x} after {total} insns"
                        );
                        uc.get_data_mut().irqen_logged_i = true;
                    }
                    if !logged_f && cpsr & (1 << 6) == 0 {
                        let total = {
                            let s = uc.get_data();
                            s.instructions.saturating_add(s.fast_forwarded_instructions)
                        };
                        eprintln!(
                            "[e830-irqen] FIQ unmasked first at pc=0x{address:08x} cpsr=0x{cpsr:08x} after {total} insns"
                        );
                        uc.get_data_mut().irqen_logged_f = true;
                    }
                }
            }
        }

        // Advance device time even while the CPU masks interrupts, but only at
        // the next exact point where a modeled device can change state.
        let update_devices = total >= uc.get_data().next_device_update;
        if update_devices { advance_dma(uc); }
        let trace_service = {
            let state = uc.get_data();
            state.trace_irq_service
                && total >= state.trace_irq_service_after
                && state.trace_irq_service_remaining > 0
        };
        // CPSR may change on ANY guest instruction: never cache it across
        // instructions. But no service can return before EOI, and the RTOS
        // nesting byte is diagnostic only, not part of IRQ eligibility.
        let service_cpsr = if trace_service
            || (uc.get_data().timer_irq_in_service && uc.get_data().irq_eoi_seen)
        {
            Some(uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32)
        } else {
            None
        };
        let service_returned = service_cpsr.is_some_and(|cpsr| interrupt_service_returned(
            cpsr, uc.get_data().timer_irq_in_service, uc.get_data().irq_eoi_seen,
        ));
        if trace_service {
            let service_cpsr = service_cpsr.unwrap();
            let service_nesting = read_irq_nesting(uc);
            let state = uc.get_data();
            let pending = state.pending_lines();
            let mask = state.registers.get(&INTC_ENABLE_MASK_REGISTER).copied().unwrap_or(u32::MAX);
            let key = (
                state.timer_irq_in_service,
                state.irq_eoi_seen,
                service_nesting,
                state.irq_entry_nesting,
                service_cpsr & (CPSR_MODE_MASK | CPSR_IRQ_DISABLE | CPSR_THUMB),
                pending,
                pending & !mask,
            );
            if (state.timer_irq_in_service || state.irq_eoi_seen || key.6 != 0)
                && state.trace_irq_service_last != Some(key)
            {
                eprintln!(
                    "[e830-irq-service] pc=0x{address:08x} cpsr=0x{service_cpsr:08x} in_service={} eoi={} nesting={} entry_nesting={} pending=0x{:08x} enabled=0x{:08x} release={} insns={total}",
                    key.0, key.1, key.2, key.3, key.5, key.6, service_returned
                );
                let state = uc.get_data_mut();
                state.trace_irq_service_last = Some(key);
                state.trace_irq_service_remaining -= 1;
            }
        }
        if service_returned {
            let state = uc.get_data_mut();
            state.timer_irq_in_service = false;
            state.irq_eoi_seen = false;
        }
        if update_devices { advance_dsp_rf(uc); }
        let pending = {
            let state = uc.get_data_mut();
            let total = state.instructions.saturating_add(state.fast_forwarded_instructions);
            if update_devices {
                state.advance_dsp_serial(total);
                state.i2c.advance(total);
                state.sync_pmic_gpio();
                if state.i2c.sim_model_enabled() {
                    state.sim.advance(total, state.i2c.sim_powered());
                }
                state.advance_system_timer(total);
                state.advance_secondary_timers(total);
                state.next_device_update = state.next_device_deadline(total);
                state.enabled_pending_dirty = true;
            }
            if state.timer_irq_period == 0 {
                state.enabled_pending_lines()
            } else {
                if !state.timer_irq_pending {
                    // Drive the tick cadence off *total* elapsed guest
                    // instructions, counting the delay-loop runs we
                    // fast-forward. Those skipped iterations are real elapsed
                    // device time, so the system clock must advance across them
                    // or a splash that spends almost all its instructions in
                    // fast-forwarded busy-waits would see only a handful of
                    // ticks and never reach its menu timeout. `timer_irq_counter`
                    // holds the total at the last raised tick.
                    let total = state
                        .instructions
                        .saturating_add(state.fast_forwarded_instructions);
                    if total.saturating_sub(state.timer_irq_counter) >= state.timer_irq_period {
                        state.timer_irq_counter = total;
                        state.timer_irq_pending = true;
                        state.enabled_pending_dirty = true;
                    }
                }
                state.enabled_pending_lines()
            }
        };
        if pending != 0 {
            let cpsr = service_cpsr.unwrap_or_else(|| uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32);
            let in_service = uc.get_data().timer_irq_in_service;
            let line = 31 - pending.leading_zeros();
            if interrupt_delivery_allowed(cpsr, in_service) {
                // ARM IRQ entry: the return address is the instruction we were
                // about to run, recovered by the handler's `sub lr,lr,#4` from
                // lr = address + 4. Switch to IRQ mode first so the banked LR
                // write lands in lr_irq, stash the interrupted PSR in spsr_irq,
                // and vector to the handler in ARM state with IRQs masked.
                let return_addr = u64::from(address as u32).wrapping_add(4);
                let new_cpsr = (cpsr & !CPSR_MODE_MASK & !CPSR_THUMB)
                    | CPSR_MODE_IRQ
                    | CPSR_IRQ_DISABLE;
                if uc.reg_write(RegisterARM::CPSR, u64::from(new_cpsr)).is_ok()
                    && uc.reg_write(RegisterARM::SPSR, u64::from(cpsr)).is_ok()
                    && uc.reg_write(RegisterARM::LR, return_addr).is_ok()
                    && uc.reg_write(RegisterARM::PC, IRQ_VECTOR_TARGET).is_ok()
                {
                    let service_nesting = read_irq_nesting(uc);
                    let state = uc.get_data_mut();
                    state.irqs_taken = state.irqs_taken.saturating_add(1);
                    state.active_irq_line = line;
                    state.timer_irq_in_service = true;
                    state.irq_eoi_seen = false;
                    state.irq_entry_nesting = service_nesting;
                    if state.trace_irq_service
                        && total >= state.trace_irq_service_after
                        && state.trace_irq_service_remaining > 0
                    {
                        eprintln!(
                            "[e830-irq-service] enter line={line} from=0x{address:08x} cpsr=0x{cpsr:08x} nesting={service_nesting} pending=0x{pending:08x} insns={total}"
                        );
                        state.trace_irq_service_last = None;
                        state.trace_irq_service_remaining -= 1;
                    }
                    return;
                }
            }
        }

        // A register-equivalent loop collapse is not event-equivalent while
        // RF measurements are in flight. The 356,038,371 zero-fill skips
        // 200,959 clocks with IRQs enabled, crossing multiple result deadlines
        // and timer matches. Execute the original instructions instead, so the
        // normal payload publication and IRQ collector retain their ordering.
        // This affects only pending opt-in RF work, not default boot speed.
        if uc.get_data().dsp_rf.busy() {
            return;
        }

        if is_delay_loop {
            let r0 = uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32;
            let r1 = uc.reg_read(RegisterARM::R1).unwrap_or(0) as u32;
            let r2 = uc.reg_read(RegisterARM::R2).unwrap_or(0) as u32;
            let Some((fast_forwarded_r0, fast_forwarded_r2, skipped_instructions)) =
                fast_forward_counting_delay(r0, r1, r2, remaining_before.saturating_sub(1))
            else {
                return;
            };
            if uc
                .reg_write(RegisterARM::R0, u64::from(fast_forwarded_r0))
                .is_err()
                || uc
                    .reg_write(RegisterARM::R2, u64::from(fast_forwarded_r2))
                    .is_err()
            {
                return;
            }
            let state = uc.get_data_mut();
            state.fast_forwarded_instructions = state.fast_forwarded_instructions
                .saturating_add(skipped_instructions);
            return;
        }
        if is_nested_delay_loop {
            let r0 = uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32;
            let r1 = uc.reg_read(RegisterARM::R1).unwrap_or(0) as u32;
            let r2 = uc.reg_read(RegisterARM::R2).unwrap_or(0) as u32;
            if r0 != r2 {
                return;
            }
            let Some(skipped_instructions) = fast_forward_nested_delay(r0, r1, remaining_before)
            else {
                return;
            };
            let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32 | (1 << 5);
            if uc.reg_write(RegisterARM::R0, 0).is_err()
                || uc.reg_write(RegisterARM::PC, address + 6).is_err()
                || uc.reg_write(RegisterARM::CPSR, u64::from(cpsr)).is_err()
            {
                return;
            }
            let state = uc.get_data_mut();
            state.fast_forwarded_instructions = state.fast_forwarded_instructions
                .saturating_add(skipped_instructions);
            return;
        }
        if is_fade_delay_loop {
            // The splash LCD-init table interpreter (around 0x20155490) runs a
            // `mov r8,r8; subs r0,#1; bne .-2` software delay between register
            // writes, with the inner count r0 reloaded from r2 each outer pass.
            // It is not one of the earlier delay shapes and so executed in real
            // time, spending billions of instructions holding the logo. Fast-
            // forward the inner countdown to zero exactly like the other delay
            // loops and count the skipped guest instructions.
            let r0 = uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32;
            if r0 == 0 {
                return;
            }
            let Some(loop_instructions) = u64::from(r0).checked_mul(3) else {
                return;
            };
            if loop_instructions > remaining_before {
                return;
            }
            let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32 | CPSR_THUMB;
            if uc.reg_write(RegisterARM::R0, 0).is_err()
                // Unicorn selects ARM/Thumb state from bit 0 on a PC write.
                // Keep this synthetic skip in Thumb just like the original
                // branch at the end of the firmware delay loop.
                || uc.reg_write(RegisterARM::PC, address + 7).is_err()
                || uc.reg_write(RegisterARM::CPSR, u64::from(cpsr)).is_err()
            {
                return;
            }
            let skipped_instructions = loop_instructions - 1;
            let state = uc.get_data_mut();
            state.fast_forwarded_instructions = state.fast_forwarded_instructions
                .saturating_add(skipped_instructions);
            return;
        }
        if !is_memset_loop {
            return;
        }

        let r0 = uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32;
        let r1 = uc.reg_read(RegisterARM::R1).unwrap_or(0) as u32;
        let r2 = uc.reg_read(RegisterARM::R2).unwrap_or(0) as u32;
        let r3 = uc.reg_read(RegisterARM::R3).unwrap_or(0) as u32;
        let r12 = uc.reg_read(RegisterARM::R12).unwrap_or(0) as u32;
        let lr = uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;
        let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
        let Some(plan) = plan_zero_fill(r0, r1, cpsr, r2, r3, r12, lr) else {
            return;
        };
        if plan.skipped_instructions > remaining_before {
            return;
        }
        if plan.bytes > 0 {
            let zeros = vec![0; plan.bytes];
            if uc.mem_write(u64::from(r0), &zeros).is_err() {
                return;
            }
        }
        if uc
            .reg_write(RegisterARM::R0, u64::from(plan.new_r0))
            .is_err()
            || uc
                .reg_write(RegisterARM::R1, u64::from(plan.new_r1))
                .is_err()
            || uc
                .reg_write(RegisterARM::CPSR, u64::from(plan.new_cpsr))
                .is_err()
            || uc
                .reg_write(RegisterARM::PC, address + ZERO_FILL_LOOP.len() as u64)
                .is_err()
        {
            return;
        }
        let state = uc.get_data_mut();
        state.instructions = state.instructions.saturating_sub(1);
        state.fast_forwarded_instructions = state
            .fast_forwarded_instructions
            .saturating_add(plan.skipped_instructions);
    })
    .map_err(|error| unicorn_error("installing the instruction counter", error))?;

    cpu.add_mem_hook(HookType::MEM_WRITE, crate::dsp_rf::SNAPSHOT_BASE,
        crate::dsp_rf::SNAPSHOT_BASE + 3, |uc, _, address, size, value| {
            if uc.get_data().dsp_rf.enabled && address == crate::dsp_rf::SNAPSHOT_BASE
                && size >= 2 && value == 0
            {
                uc.get_data_mut().dsp_rf.clear();
            }
            false
        })
        .map_err(|error| unicorn_error("installing RF HLE schedule lifetime", error))?;

    cpu.add_mem_hook(
        HookType::MEM_WRITE,
        WAKE_STATE_START,
        WAKE_STATE_END,
        |uc, _, address, size, value| {
            let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
            let total = uc.get_data().instructions.saturating_add(uc.get_data().fast_forwarded_instructions);
            uc.get_data_mut().trace_wake_line(format!(
                "ram write addr=0x{address:08x} size={size} value=0x{value:08x} pc=0x{pc:08x} insns={total}"
            ));
            false
        },
    )
    .map_err(|error| unicorn_error("installing the wake-state trace", error))?;

    cpu.add_mem_hook(
        HookType::MEM_WRITE,
        WAKE_STUB_START,
        WAKE_STUB_END,
        |uc, _, address, size, value| {
            let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
            let total = uc.get_data().instructions.saturating_add(uc.get_data().fast_forwarded_instructions);
            uc.get_data_mut().trace_wake_line(format!(
                "stub write addr=0x{address:08x} size={size} value=0x{value:08x} pc=0x{pc:08x} insns={total}"
            ));
            false
        },
    )
    .map_err(|error| unicorn_error("installing the wake-stub trace", error))?;

    cpu.add_mem_hook(
        HookType::MEM_WRITE,
        SYSTEM_TIMER_BOOKKEEPING_START,
        SYSTEM_TIMER_BOOKKEEPING_END,
        |uc, _, address, size, value| {
            let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
            let total = uc.get_data().instructions.saturating_add(uc.get_data().fast_forwarded_instructions);
            let state = uc.get_data();
            let counter = state.timer_counter;
            let control = state.registers.get(&(SOC_TIMER + 8)).copied().unwrap_or(0);
            let period = state.registers.get(&(SOC_TIMER + 0x0c)).copied().unwrap_or(0);
            let match0 = state.registers.get(&(SOC_TIMER + 0x10)).copied().unwrap_or(0);
            let match1 = state.registers.get(&(SOC_TIMER + 0x14)).copied().unwrap_or(0);
            uc.get_data_mut().trace_timer_line(format!(
                "ram write addr=0x{address:08x} size={size} value=0x{value:08x} pc=0x{pc:08x} counter=0x{counter:04x} ctrl=0x{control:08x} period=0x{period:08x} match0=0x{match0:08x} match1=0x{match1:08x} insns={total}"
            ));
            false
        },
    )
    .map_err(|error| unicorn_error("installing the system-timer bookkeeping trace", error))?;

    cpu.add_mem_hook(
        HookType::MEM_WRITE,
        RTOS_GLOBALS_TRACE_START,
        RTOS_GLOBALS_TRACE_END,
        |uc, _, address, size, value| {
            let total = uc.get_data().instructions
                .saturating_add(uc.get_data().fast_forwarded_instructions);
            let write_end = address.saturating_add(size as u64).saturating_sub(1);
            let hits_target = RTOS_GLOBALS_TRACE_TARGETS.iter().any(|&(start, end)| {
                address <= end && write_end >= start
            });
            let should_log = {
                let state = uc.get_data();
                hits_target
                    && state.trace_rtos_globals
                    && total >= state.trace_rtos_globals_after
                    && state.trace_rtos_globals_remaining > 0
            };
            if should_log {
                let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                eprintln!(
                    "[e830-rtos] write addr=0x{address:08x} size={size} value=0x{value:08x} pc=0x{pc:08x} insns={total}"
                );
                uc.get_data_mut().trace_rtos_globals_remaining -= 1;
            }
            false
        },
    )
    .map_err(|error| unicorn_error("installing the RTOS-globals trace", error))?;

    cpu.add_mem_hook(
        HookType::MEM_WRITE,
        RTOS_NESTING_DEPTH & !3,
        (RTOS_NESTING_DEPTH & !3) + 3,
        |uc, _, address, size, value| {
            let total = uc.get_data().instructions
                .saturating_add(uc.get_data().fast_forwarded_instructions);
            let write_end = address.saturating_add(size as u64).saturating_sub(1);
            let hits_nesting = address <= RTOS_NESTING_DEPTH
                && write_end >= RTOS_NESTING_DEPTH;
            let should_log = {
                let state = uc.get_data();
                hits_nesting
                    && state.trace_nesting
                    && total >= state.trace_nesting_after
                    && state.trace_nesting_remaining > 0
            };
            if should_log {
                let old = uc.mem_read_as_vec(RTOS_NESTING_DEPTH, 1)
                    .ok()
                    .and_then(|bytes| bytes.first().copied())
                    .unwrap_or(0);
                let byte_offset = RTOS_NESTING_DEPTH.saturating_sub(address) as usize;
                let new = if byte_offset < size {
                    ((value >> (byte_offset * 8)) & 0xff) as u8
                } else {
                    old
                };
                let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                let sp = uc.reg_read(RegisterARM::SP).unwrap_or(0) as u32;
                let lr = uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;
                let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
                eprintln!(
                    "[e830-nesting] old=0x{old:02x} new=0x{new:02x} addr=0x{address:08x} size={size} value=0x{value:08x} pc=0x{pc:08x} sp=0x{sp:08x} lr=0x{lr:08x} cpsr=0x{cpsr:08x} insns={total}"
                );
                uc.get_data_mut().trace_nesting_remaining -= 1;
            }
            false
        },
    )
    .map_err(|error| unicorn_error("installing the RTOS-nesting trace", error))?;

    cpu.add_mem_hook(
        HookType::MEM_WRITE,
        MENU_INPUT_KEY_BLOCK_CANDIDATE,
        MENU_INPUT_KEY_BLOCK_CANDIDATE,
        |uc, _, address, size, value| {
            let total = uc.get_data().instructions
                .saturating_add(uc.get_data().fast_forwarded_instructions);
            let should_log = {
                let state = uc.get_data();
                state.trace_menu_input
                    && total >= state.trace_menu_input_after
                    && state.trace_menu_input_remaining > 0
            };
            if should_log {
                let old = diagnostic_ram_u8(uc, MENU_INPUT_KEY_BLOCK_CANDIDATE)
                    .unwrap_or(u8::MAX);
                let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                let sp = uc.reg_read(RegisterARM::SP).unwrap_or(0) as u32;
                let lr = uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;
                eprintln!(
                    "[e830-menu-input] stage=key-block-write pc=0x{pc:08x} insns={total} addr=0x{address:08x} size={size} old=0x{old:02x} value=0x{value:08x} sp=0x{sp:08x} lr=0x{lr:08x}"
                );
                uc.get_data_mut().trace_menu_input_remaining -= 1;
            }
            false
        },
    )
    .map_err(|error| unicorn_error("installing the late-menu key-block trace", error))?;

    if let Some(start) = env_u64("E830_TRACE_RAM_START") {
        let end = env_u64("E830_TRACE_RAM_END").unwrap_or(start);
        let after = env_u64("E830_TRACE_RAM_AFTER").unwrap_or(0);
        let remaining = Rc::new(Cell::new(
            env_u64("E830_TRACE_RAM_LIMIT").unwrap_or(1000),
        ));
        let trace_reads = std::env::var_os("E830_TRACE_RAM_READS").is_some();
        let write_remaining = Rc::clone(&remaining);
        cpu.add_mem_hook(
            HookType::MEM_WRITE,
            start.min(end),
            start.max(end),
            move |uc, _, address, size, value| {
                let total = uc.get_data().instructions
                    .saturating_add(uc.get_data().fast_forwarded_instructions);
                let left = write_remaining.get();
                if total >= after && left > 0 {
                    let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                    eprintln!(
                        "[e830-ram] write addr=0x{address:08x} size={size} value=0x{value:08x} pc=0x{pc:08x} insns={total}"
                    );
                    write_remaining.set(left - 1);
                }
                false
            },
        )
        .map_err(|error| unicorn_error("installing the focused RAM-write trace", error))?;

        if trace_reads {
            cpu.add_mem_hook(
                HookType::MEM_READ,
                start.min(end),
                start.max(end),
                move |uc, _, address, size, value| {
                    let total = uc.get_data().instructions
                        .saturating_add(uc.get_data().fast_forwarded_instructions);
                    let left = remaining.get();
                    if total >= after && left > 0 {
                        let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                        eprintln!(
                            "[e830-ram] read addr=0x{address:08x} size={size} value=0x{value:08x} pc=0x{pc:08x} insns={total}"
                        );
                        remaining.set(left - 1);
                    }
                    false
                },
            )
            .map_err(|error| unicorn_error("installing the focused RAM-read trace", error))?;
        }
    }

    cpu.add_mem_hook(
        HookType::MEM_INVALID,
        0,
        u64::MAX,
        |uc, kind, address, size, _| {
            let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
            uc.get_data_mut().memory_fault = Some(MemoryFault {
                kind: format!("{kind:?}"),
                address,
                size,
                pc,
            });
            false
        },
    )
    .map_err(|error| unicorn_error("installing the memory-fault recorder", error))?;
    cpu.add_intr_hook(|uc, interrupt| {
        if uc.get_data().trace_mmio {
            let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
            let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
            let lr = uc.reg_read(RegisterARM::LR).unwrap_or(0) as u32;
            let r0 = uc.reg_read(RegisterARM::R0).unwrap_or(0) as u32;
            let r1 = uc.reg_read(RegisterARM::R1).unwrap_or(0) as u32;
            eprintln!("[e830-intr] interrupt={interrupt} pc=0x{pc:08x} cpsr=0x{cpsr:08x} lr=0x{lr:08x} r0=0x{r0:08x} r1=0x{r1:08x}");
        }
    })
    .map_err(|error| unicorn_error("installing the software interrupt hook", error))?;
    Ok(())
}

fn width_mask(size: usize) -> u32 {
    let bits = size.min(4) * 8;
    if bits == 32 {
        u32::MAX
    } else {
        (1u32 << bits) - 1
    }
}

fn wrapping_u16_counter_crossed(previous: u16, ticks: u64, target: u16) -> bool {
    if ticks == 0 { return false; }
    let mut distance = u64::from(target.wrapping_sub(previous));
    if distance == 0 { distance = u64::from(u16::MAX) + 1; }
    ticks >= distance
}

fn unicorn_error(operation: &'static str, error: impl std::fmt::Debug) -> MachineError {
    MachineError::Unicorn {
        operation,
        error: format!("{error:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fast_forward_counting_delay, fast_forward_nested_delay, find_counting_delay_loop_pcs,
        find_fade_delay_loop_pcs, find_nested_delay_loop_pcs, DspBootInterface, E830Machine, E830State,
        interrupt_delivery_allowed, interrupt_service_returned,
        COUNTING_DELAY_LOOP, FADE_DELAY_LOOP,
        E830_TFS_SOURCE_BASE, FLASH_BASE,
        NESTED_DELAY_LOOP, SOC_BOOT_READY_INPUT_MASK, SOC_BOOT_READY_INPUT_REGISTER,
        SOC_INIT_CONTROL_REGISTER, SOC_KEYPAD_INPUT_REGISTERS, SOC_KEYPAD_SCAN_READ_PCS,
        SOC_TIMER,
        SYSTEM_TIMER_CPU_CLOCK_RATIO, SYSTEM_TIMER_IRQ, SYSTEM_TIMER_IRQ20_ENABLE,
        SYSTEM_TIMER_IRQ20_STATUS, SYSTEM_TIMER_IRQ21_ENABLE, SYSTEM_TIMER_IRQ21_STATUS,
        SYSTEM_TIMER_MATCH0_ENABLE, SYSTEM_TIMER_MATCH1_ENABLE, SYSTEM_TIMER_MATCH_IRQ,
    };

    #[test]
    fn unicorn_separate_ram_hooks_report_guest_reads_and_writes() {
        use std::cell::Cell;
        use std::rc::Rc;

        let mut cpu = crate::cpu::Arm946Cpu::new_for_arm946_with_data(()).unwrap();
        let cpu = cpu.raw_engine();
        cpu.mem_map(0x1_0000, 0x1000, crate::cpu::EngineProt::ALL).unwrap();
        cpu.mem_map(0x2_0000, 0x1000, crate::cpu::EngineProt::ALL).unwrap();
        cpu.mem_write(0x1_0000, &[
            0x00, 0x10, 0x90, 0xe5, // ldr r1, [r0]
            0x04, 0x10, 0x80, 0xe5, // str r1, [r0, #4]
        ]).unwrap();
        cpu.mem_write(0x2_0000, &42u32.to_le_bytes()).unwrap();
        cpu.reg_write(crate::cpu::RegisterARM::R0, 0x2_0000).unwrap();

        let reads = Rc::new(Cell::new(0));
        let read_kind = Rc::new(Cell::new(None));
        let reads_hook = Rc::clone(&reads);
        let read_kind_hook = Rc::clone(&read_kind);
        cpu.add_mem_hook(crate::cpu::HookType::MEM_READ, 0x2_0000, 0x2_0007,
            move |_, kind, _, _, _| {
                reads_hook.set(reads_hook.get() + 1);
                read_kind_hook.set(Some(kind));
                false
            }).unwrap();

        let writes = Rc::new(Cell::new(0));
        let write_kind = Rc::new(Cell::new(None));
        let writes_hook = Rc::clone(&writes);
        let write_kind_hook = Rc::clone(&write_kind);
        cpu.add_mem_hook(crate::cpu::HookType::MEM_WRITE, 0x2_0000, 0x2_0007,
            move |_, kind, _, _, _| {
                writes_hook.set(writes_hook.get() + 1);
                write_kind_hook.set(Some(kind));
                false
            }).unwrap();

        cpu.emu_start(0x1_0000, 0x1_0008, 0, 2).unwrap();
        assert_eq!(reads.get(), 1);
        assert_eq!(writes.get(), 1);
        assert_eq!(read_kind.get(), Some(unicorn_engine::unicorn_const::MemType::READ));
        assert_eq!(write_kind.get(), Some(unicorn_engine::unicorn_const::MemType::WRITE));
    }

    #[test]
    fn pmic_gpio_uses_real_group_masks_w0c_and_active_low_input() {
        let mut state = E830State::new(false, vec![0xff; 0x1000],
            Default::default(), Default::default(), Default::default(), Default::default());
        state.i2c = crate::i2c::I2c::with_sim_present(true);
        state.sync_pmic_gpio();
        assert_eq!(state.read_register(super::SOC_BOOT_READY_INPUT_REGISTER, 4, 0), 0x208);
        state.write_register(super::GPIO_BASE + 0x40, 2, 8);
        state.write_register(super::GPIO_BASE + 0x44, 2, 8);
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, 0xffff_fffc);
        state.i2c.write(0, 0x112, 0);
        state.i2c.write(0, 0x2c, 0);
        state.i2c.write(0, 0x21c, 0);
        state.i2c.advance(10_000);
        state.sync_pmic_gpio();
        assert_eq!(state.enabled_pending_lines(), 3); // groups 0 and 1, not IRQ25
        assert_eq!(state.read_register(super::SOC_BOOT_READY_INPUT_REGISTER, 4, 0), 0x200);
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, u32::MAX.into());
        assert_eq!(state.enabled_pending_lines(), 0);
        assert_eq!(state.read_register(super::GPIO_BASE + 0x4c, 2, 0), 8);
        state.write_register(super::GPIO_BASE + 0x4c, 2, 0xfff7);
        state.sync_pmic_gpio();
        assert_eq!(state.read_register(super::GPIO_BASE + 0x4c, 2, 0), 0);
        // ACK does not clear PMIC or produce another falling edge.
        assert!(state.i2c.pmic_irq_pending());
        state.write_register(super::GPIO_BASE + 0x4c, 2, 0xffff);
        assert_eq!(state.read_register(super::GPIO_BASE + 0x4c, 2, 0), 0);
        state.i2c.write(0, 0x112, 10_000);
        state.i2c.write(0, 0x203, 10_000);
        state.i2c.advance(20_000);
        state.i2c.write(0, 0x113, 20_000);
        state.i2c.write(0, 0x200, 20_000);
        state.i2c.advance(30_000);
        assert_eq!(state.i2c.read(0), 0x20);
        state.sync_pmic_gpio();
        assert!(!state.i2c.pmic_irq_pending());
        assert_eq!(state.read_register(super::SOC_BOOT_READY_INPUT_REGISTER, 4, 0), 0x208);
    }

    #[test]
    fn dma_irq_fast_path_preserves_raw_status_and_each_channel_mask() {
        let mut state = E830State::new(false, vec![0xff; 0x1000],
            Default::default(), Default::default(), Default::default(), Default::default());
        for channel in 0..8 {
            state.registers.insert(super::DMA_BASE + 0x110 + channel * 0x20, 0x8000);
        }
        // Upper reserved status bits must not raise a channel IRQ.
        state.registers.insert(super::DMA_BASE + 0x14, 0xffff_ff00);
        assert_eq!(state.dma_interrupt_status(), 0);
        for channel in 0..8 {
            let config = super::DMA_BASE + 0x110 + channel * 0x20;
            state.registers.insert(super::DMA_BASE + 0x14, 0xffff_ff00 | (1 << channel));
            assert_eq!(state.dma_interrupt_status(), 1 << channel);
            state.registers.insert(config, 0);
            assert_eq!(state.dma_interrupt_status(), 0);
            state.registers.insert(config, 0x8000);
        }
        state.registers.insert(super::DMA_BASE + 0x14, 0xff);
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, u64::from(u32::MAX));
        assert_eq!(state.enabled_pending_lines(), 0);
        assert_eq!(state.dma_interrupt_status(), 0xff); // mask is not ack
        state.write_register(
            super::INTC_ENABLE_MASK_REGISTER,
            4,
            u64::from(!(1u32 << super::DMA_IRQ)),
        );
        assert_eq!(state.enabled_pending_lines(), 1 << super::DMA_IRQ);
    }

    #[test]
    fn selective_cpsr_reads_observe_unmask_and_eoi_on_the_next_instruction() {
        use super::RegisterARM;
        for trace in [false, true] {
            let mut cla = vec![0xff; (super::IRQ_VECTOR_TARGET - u64::from(FLASH_BASE)) as usize + 0x1000];
            cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
            cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
            let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
            let mut machine = E830Machine::new(firmware, false).unwrap();
            let task_pc = 0x3000_1000u32;
            let cpu = machine.cpu.raw_engine();
            cpu.mem_write(u64::from(task_pc), &[0x00, 0x00, 0xa0, 0xe1, 0xfd, 0xff, 0xff, 0xea]).unwrap();
            cpu.mem_write(super::RTOS_NESTING_DEPTH, &[5]).unwrap();
            cpu.reg_write(RegisterARM::CPSR, 0xdf).unwrap();
            cpu.reg_write(RegisterARM::PC, u64::from(task_pc)).unwrap();
            cpu.reg_write(RegisterARM::R4, 0x1234_5678).unwrap();
            let state = cpu.get_data_mut();
            state.trace_irq_service = trace;
            state.trace_irq_service_after = 0;
            state.trace_irq_service_remaining = 20;
            state.timer_irq_period = 0;
            state.timer_irq_pending = true;
            state.timer_irq_index = super::DMA_IRQ;
            state.timer_irq_in_service = true;
            state.irq_eoi_seen = true;
            state.registers.insert(super::INTC_ENABLE_MASK_REGISTER, !(1 << super::DMA_IRQ));

            let masked = machine.run(2).unwrap();
            assert_eq!(masked.irqs_taken, 0);
            assert!(machine.cpu.raw_engine().get_data().timer_irq_in_service);
            // A change between slices must not reuse the old CPSR.I value.
            let cpu = machine.cpu.raw_engine();
            cpu.reg_write(RegisterARM::CPSR, 0x1f).unwrap();
            cpu.get_data_mut().irq_eoi_seen = false;
            let before_eoi = machine.run(1).unwrap();
            assert_eq!(before_eoi.irqs_taken, 0); // unmask alone is insufficient
            assert!(machine.cpu.raw_engine().get_data().timer_irq_in_service);
            let return_pc = u64::from(before_eoi.pc) + 4;
            machine.cpu.raw_engine().get_data_mut().irq_eoi_seen = true;
            let entered = machine.run(1).unwrap();
            assert_eq!(entered.irqs_taken, 1);
            assert_eq!(u64::from(entered.pc), super::IRQ_VECTOR_TARGET);
            assert_eq!(entered.cpsr & 0x9f, 0x92);
            assert_eq!(entered.r4, 0x1234_5678);
            let cpu = machine.cpu.raw_engine();
            assert_eq!(cpu.reg_read(RegisterARM::SPSR).unwrap(), 0x1f);
            assert_eq!(cpu.reg_read(RegisterARM::LR).unwrap(), return_pc);
            assert_eq!(cpu.get_data().irq_entry_nesting, 5);
            assert_eq!(super::read_irq_nesting(cpu), 5); // diagnostic byte is untouched
            assert!(!cpu.get_data().irq_eoi_seen);
        }
    }

    #[test]
    fn guest_msr_unmask_delivers_pending_irq_before_the_following_instruction() {
        use super::RegisterARM;
        let mut cla = vec![0xff; (super::IRQ_VECTOR_TARGET - u64::from(FLASH_BASE)) as usize + 0x1000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let task_pc = 0x3000_1000u32;
        let cpu = machine.cpu.raw_engine();
        // msr cpsr_c, r0; mov r4, #7; b . -- R4 must NOT reach seven.
        cpu.mem_write(u64::from(task_pc), &[
            0x00, 0xf0, 0x21, 0xe1, 0x07, 0x40, 0xa0, 0xe3, 0xfe, 0xff, 0xff, 0xea,
        ]).unwrap();
        cpu.reg_write(RegisterARM::CPSR, 0xdf).unwrap();
        cpu.reg_write(RegisterARM::PC, u64::from(task_pc)).unwrap();
        cpu.reg_write(RegisterARM::R0, 0x1f).unwrap();
        cpu.reg_write(RegisterARM::R4, 0x1234).unwrap();
        let state = cpu.get_data_mut();
        state.trace_irq_service = false;
        state.timer_irq_period = 0;
        state.timer_irq_pending = true;
        state.timer_irq_index = super::DMA_IRQ;
        state.timer_irq_in_service = false;
        state.registers.insert(super::INTC_ENABLE_MASK_REGISTER, !(1 << super::DMA_IRQ));
        let report = machine.run(2).unwrap();
        assert!(report.memory_fault.is_none());
        assert_eq!(report.irqs_taken, 1);
        assert_eq!(u64::from(report.pc), super::IRQ_VECTOR_TARGET);
        assert_eq!(report.r4, 0x1234);
        let cpu = machine.cpu.raw_engine();
        assert_eq!(cpu.reg_read(RegisterARM::SPSR).unwrap(), 0x1f);
        assert_eq!(cpu.reg_read(RegisterARM::LR).unwrap(), u64::from(task_pc) + 8);
    }

    #[test]
    fn state_dispatch_counters_classify_tables_not_execution_at_data_symbols() {
        let pc = super::STATE_TABLE_DISPATCH_PC;
        assert_eq!(super::state_dispatch_kind(pc, super::KEYBOARD_INIT_TABLE), (true, false));
        assert_eq!(super::state_dispatch_kind(pc, super::STANDBY_LOOK_TABLE), (false, true));
        assert_eq!(super::state_dispatch_kind(pc, 0x3197_f3bc), (false, false));
        assert_eq!(super::state_dispatch_kind(u64::from(super::KEYBOARD_INIT_TABLE),
            super::KEYBOARD_INIT_TABLE), (false, false));
    }

    #[test]
    fn menu_input_trace_points_have_unique_stage_names() {
        let unique: std::collections::HashSet<_> = super::MENU_INPUT_TRACE_PCS.into_iter().collect();
        assert_eq!(unique.len(), super::MENU_INPUT_TRACE_PCS.len());
        for pc in super::MENU_INPUT_TRACE_PCS {
            assert_ne!(super::menu_input_stage(pc), "unknown");
        }
        assert_eq!(super::menu_input_stage(0), "unknown");
        assert_eq!(super::key_filter_caller(0x200d_4d99), "mcc-getkey-accept-reject-s");
        assert_eq!(super::key_filter_caller(0x2111_2b89), "mcc-getkey-s");
        assert_eq!(super::key_filter_caller(0x2153_66a5), "mcc-getkey-incall-s");
        assert_eq!(super::key_filter_caller(0), "other");
        assert_eq!(super::key_filter_context(0x200d_4d99, 1, 2, 3), 3);
        assert_eq!(super::key_filter_context(0x2111_2b89, 1, 2, 3), 1);
        assert_eq!(super::key_filter_context(0x2153_66a5, 1, 2, 3), 2);
        assert_eq!(super::key_filter_context(0, 1, 2, 3), 0);
    }

    #[test]
    fn dsp_io_trace_is_bounded_passive_and_separate_from_ram_trace() {
        let mut state = E830State::new(false, vec![0xff; 0x1000],
            Default::default(), Default::default(), Default::default(), Default::default());
        state.trace_dsp_io = true;
        state.trace_dsp_io_after = 100;
        state.trace_dsp_io_remaining = 2;
        state.trace_sim = false;
        let access = super::MmioAccess {
            write: false, address: super::DSP_RESULT_SIGNAL, size: 2,
            value: 0x80, pc: 0x205769f8,
        };
        state.registers.insert(super::DSP_RESULT_SIGNAL, 0x80);
        state.dsp_boot.rx_fifo.push_back(0x1234);
        state.record_mmio(access.clone());
        assert_eq!(state.trace_dsp_io_remaining, 2);
        state.instructions = 90;
        state.fast_forwarded_instructions = 10;
        state.record_mmio(super::MmioAccess {
            address: 0x50406034, ..access.clone()
        });
        assert_eq!(state.trace_dsp_io_remaining, 2);
        state.record_mmio(access.clone());
        assert_eq!(state.trace_dsp_io_remaining, 1);
        state.record_mmio(super::MmioAccess {
            address: super::DSP_CONTROL_BASE as u32 + 4, ..access.clone()
        });
        assert_eq!(state.trace_dsp_io_remaining, 0);
        state.record_mmio(super::MmioAccess {
            write: true, address: super::DSP_SERIAL_BASE + 0x200, ..access.clone()
        });
        assert_eq!(state.trace_dsp_io_remaining, 0);
        assert_eq!(state.registers.get(&super::DSP_RESULT_SIGNAL), Some(&0x80));
        assert_eq!(state.registers.len(), 2);
        assert_eq!(state.dsp_boot.rx_fifo.front(), Some(&0x1234));
        assert_eq!(state.dsp_boot.rx_fifo.len(), 1);
        assert_eq!(state.pending_lines(), 0);
        state.trace_dsp_io = false;
        state.trace_dsp_io_remaining = 1;
        state.record_mmio(access);
        assert_eq!(state.trace_dsp_io_remaining, 1);
    }

    #[test]
    fn dsp_io_trace_observes_arm_mmio_callbacks_without_extra_fifo_reads() {
        let run = |trace| {
            let mut cla = vec![0xff; 0x20_000];
            cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
            cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
            // ARM loads from boot RX FIFO, result flags and serial status.
            let words = [
                0xe59f_1014u32, 0xe1d1_00b0,
                0xe59f_1010, 0xe1d1_20b0,
                0xe59f_100c, 0xe1d1_30b0,
                0xeaff_fffe,
                super::DSP_CONTROL_BASE as u32 + 4,
                super::DSP_RESULT_SIGNAL,
                super::DSP_SERIAL_BASE + 0x14,
            ];
            for (index, word) in words.iter().enumerate() {
                cla[0x100 + index * 4..0x104 + index * 4]
                    .copy_from_slice(&word.to_le_bytes());
            }
            let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
            let mut machine = E830Machine::new(firmware, false).unwrap();
            {
                let state = machine.cpu.raw_engine().get_data_mut();
                state.trace_dsp_io = trace;
                state.trace_dsp_io_after = 0;
                state.trace_dsp_io_remaining = 3;
                state.dsp_boot.rx_fifo.extend([0x1234, 0xabcd]);
                state.registers.insert(super::DSP_RESULT_SIGNAL, 0x80);
                state.registers.insert(super::DSP_SERIAL_BASE + 0x14, 4);
            }
            let report = machine.run(8).unwrap();
            let state = machine.cpu.raw_engine().get_data_mut();
            assert_eq!(report.r0, 0x1234);
            assert_eq!(report.r2, 0x80);
            assert_eq!(report.r3, 4);
            assert!(report.memory_fault.is_none());
            assert_eq!(state.trace_dsp_io_remaining, if trace { 0 } else { 3 });
            assert_eq!(state.dsp_boot.rx_fifo.iter().copied().collect::<Vec<_>>(), [0xabcd]);
            assert_eq!(state.registers.get(&super::DSP_RESULT_SIGNAL), Some(&0x80));
            assert_eq!(state.registers.get(&(super::DSP_SERIAL_BASE + 0x14)), Some(&4));
            assert_eq!(state.pending_lines(), 0);
            format!("{report:#?}")
        };
        assert_eq!(run(true), run(false));
    }

    #[test]
    fn dispatch_diagnostics_dereference_state_node_without_guest_writes() {
        use super::RegisterARM;
        let pc = super::STATE_TABLE_DISPATCH_PC;
        let offset = (pc - u64::from(FLASH_BASE)) as usize;
        let mut cla = vec![0xff; offset + 0x1000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        cla[offset..offset + 4].copy_from_slice(&[0x88, 0x47, 0xfe, 0xe7]); // blx r1; b .
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let node = 0x3000_1000u32;
        let callback = 0x3000_1100u32;
        let cpu = machine.cpu.raw_engine();
        cpu.mem_write(u64::from(callback), &[0x70, 0x47]).unwrap(); // bx lr
        cpu.get_data_mut().trace_tasks = true;
        cpu.get_data_mut().trace_tasks_after = 0;
        cpu.get_data_mut().trace_tasks_remaining = 2;
        cpu.get_data_mut().trace_tasks_pcs = [pc].into();

        for (table, expected) in [
            (super::KEYBOARD_INIT_TABLE, (1, 0)),
            (super::STANDBY_LOOK_TABLE, (1, 1)),
            (0x3197_f3bc, (1, 1)),
        ] {
            let mut record = vec![0x5a; 20];
            record[4..8].copy_from_slice(&table.to_le_bytes());
            let cpu = machine.cpu.raw_engine();
            cpu.mem_write(u64::from(node), &record).unwrap();
            cpu.reg_write(RegisterARM::CPSR, 0x3f).unwrap();
            cpu.reg_write(RegisterARM::PC, pc).unwrap();
            cpu.reg_write(RegisterARM::R5, u64::from(node)).unwrap();
            cpu.reg_write(RegisterARM::R1, u64::from(callback | 1)).unwrap();
            let report = machine.run(2).unwrap();
            assert!(report.memory_fault.is_none());
            assert_eq!((report.keyboard_init_dispatches, report.standby_look_dispatches), expected);
            let cpu = machine.cpu.raw_engine();
            assert_eq!(cpu.mem_read_as_vec(u64::from(node), 20).unwrap(), record);
            assert_eq!(cpu.mem_read_as_vec(u64::from(callback), 2).unwrap(), [0x70, 0x47]);
        }
        let cpu = machine.cpu.raw_engine();
        assert_eq!(cpu.get_data().trace_tasks_remaining, 0);
        cpu.reg_write(RegisterARM::R5, 0xffff_fffc).unwrap();
        assert_eq!(super::dispatch_table(cpu), None);
        cpu.reg_write(RegisterARM::R5, super::DSP_CONTROL_BASE).unwrap();
        assert_eq!(super::dispatch_table(cpu), None);
        assert_eq!(super::diagnostic_ram_read(cpu, u64::from(crate::RAM_BASE) - 1, 4), None);
        assert_eq!(super::diagnostic_ram_read(cpu,
            u64::from(crate::RAM_BASE) + crate::RAM_SIZE as u64 - 2, 4), None);
        assert!(super::diagnostic_code_or_ram_read(cpu, u64::from(crate::FLASH_BASE), 4).is_some());
        assert_eq!(super::diagnostic_code_or_ram_read(cpu, super::EXTERNAL_FLASH_WINDOW_BASE, 4), None);
        assert_eq!(super::diagnostic_code_or_ram_read(cpu, u64::from(super::SOC_TIMER), 4), None);
    }

    #[test]
    fn interrupt_delivery_waits_for_outer_vector_completion() {
        assert!(interrupt_delivery_allowed(0x1f, false));
        assert!(!interrupt_delivery_allowed(0x9f, false));
        assert!(!interrupt_delivery_allowed(0x12, false));
        assert!(!interrupt_delivery_allowed(0x1f, true));

        assert!(!interrupt_service_returned(0x1f, true, false));
        assert!(!interrupt_service_returned(0x9f, true, true));
        assert!(!interrupt_service_returned(0x12, true, true));
        assert!(interrupt_service_returned(0x1f, true, true));
        assert!(interrupt_service_returned(0x3f, true, true));
    }

    #[test]
    fn dsp_boot_signature_sets_and_clears_rx_ready() {
        let mut dsp = DspBootInterface::default();
        assert_eq!(dsp.status() & 3, DspBootInterface::TX_READY);

        dsp.write(super::DSP_CONTROL_BASE as u32 + 8, 4, 0x000b);
        assert_eq!(dsp.status() & 3, DspBootInterface::TX_READY | DspBootInterface::RX_READY);
        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32 + 4, 4), 0x0305);
        assert_eq!(dsp.status() & 3, DspBootInterface::TX_READY);
    }

    #[test]
    fn dsp_boot_fifo_preserves_upload_for_readback() {
        let mut dsp = DspBootInterface::default();
        for word in [0x0001u16, 0x0020, 0x0003, 0x1111, 0x2222, 0x3333] {
            dsp.write(super::DSP_CONTROL_BASE as u32 + 8, 4, u64::from(word));
        }
        for word in [0x0005u16, 0x0020, 0x0003] {
            dsp.write(super::DSP_CONTROL_BASE as u32 + 8, 4, u64::from(word));
        }

        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32 + 4, 2), 0x1111);
        assert_ne!(dsp.status() & DspBootInterface::RX_READY, 0);
        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32 + 4, 2), 0x2222);
        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32 + 4, 2), 0x3333);
        assert_eq!(dsp.status() & DspBootInterface::RX_READY, 0);
    }

    #[test]
    fn dsp_boot_memory_spaces_are_independent() {
        let mut dsp = DspBootInterface::default();
        for word in [0x0001, 0x7000, 0x0001, 0xaaaa] {
            dsp.transmit(word);
        }
        for word in [0x0002, 0x7000, 0x0001, 0xbbbb] {
            dsp.transmit(word);
        }
        for word in [0x0005, 0x7000, 0x0001] {
            dsp.transmit(word);
        }
        for word in [0x0006, 0x7000, 0x0001] {
            dsp.transmit(word);
        }

        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32 + 4, 2), 0xaaaa);
        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32 + 4, 2), 0xbbbb);
    }

    #[test]
    fn dsp_memory_snapshot_preserves_word_addresses_and_zero_fills_holes() {
        let mut dsp = DspBootInterface::default();
        dsp.memories[0].insert(0, 0x1234);
        dsp.memories[0].insert(2, 0xabcd);
        let bytes = dsp.memory_snapshot(0).unwrap();
        assert_eq!(&bytes[..6], &[0x34, 0x12, 0, 0, 0xcd, 0xab]);
        assert_eq!(bytes.len(), 0x20_000);
        assert!(dsp.memory_snapshot(2).is_none());
    }

    #[test]
    fn dsp_control_keeps_firmware_bits_while_status_is_device_owned() {
        let mut dsp = DspBootInterface::default();
        dsp.write(super::DSP_CONTROL_BASE as u32, 4, 0x0048);
        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32, 4), 0x0049);
        dsp.transmit(0x000b);
        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32, 4), 0x004b);
    }

    #[test]
    fn dsp_start_leaves_boot_protocol_before_runtime_commands() {
        let mut dsp = DspBootInterface::default();
        for word in [0x0001, 0x0020, 0x0001, 0xabcd, 0x0003, 0x0000] {
            dsp.transmit(word);
        }

        // A runtime command that shares the boot readback opcode must not
        // consume later words as an address/count tuple or produce boot data.
        for word in [0x0006, 0x0020, 0x0001] {
            dsp.transmit(word);
        }

        assert_eq!(dsp.status() & DspBootInterface::RX_READY, 0);
        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32 + 4, 2), 0);
    }

    #[test]
    fn diagnostic_dsp_runtime_hle_requires_recognized_running_phase() {
        let mut disabled = DspBootInterface::new(false);
        disabled.parser = super::DspBootParser::Running;
        assert_eq!(disabled.transmit(0x4001), None);

        let mut recognized = DspBootInterface::new(true);
        recognized.parser = super::DspBootParser::Running;
        recognized.hle_phase = super::DspHlePhase::E830Running;
        assert_eq!(recognized.transmit(0x4001), Some(super::DspEvent::RuntimeReady));
        assert_eq!(recognized.hle_phase, super::DspHlePhase::E830RuntimeReady);
        assert_eq!(recognized.transmit(0x1001), Some(super::DspEvent::RuntimeReady));
        assert!(recognized.runtime_signal_acknowledged());
        assert!(!recognized.runtime_signal_acknowledged());
        assert_eq!(recognized.transmit(0x1002), Some(super::DspEvent::RuntimeReady));
        assert!(!recognized.runtime_signal_acknowledged());
        assert_eq!(recognized.transmit(0x2001), Some(super::DspEvent::RuntimeReady));
        assert!(!recognized.runtime_signal_acknowledged());
        assert_eq!(recognized.transmit(0x4001), Some(super::DspEvent::RuntimeReady));
    }

    #[test]
    fn diagnostic_dsp_runtime_readback_returns_uploaded_words_and_resumes_runtime() {
        let mut dsp = DspBootInterface::new(true);
        dsp.hle_phase = super::DspHlePhase::E830Running;
        dsp.parser = super::DspBootParser::Running;
        dsp.memories[1].insert(0x7800, 0x1234);
        dsp.memories[1].insert(0x7801, 0xabcd);

        for word in [0x0006, 0x7800, 0x0002] {
            assert_eq!(dsp.transmit(word), None);
        }
        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32 + 4, 2), 0x1234);
        assert_eq!(dsp.read(super::DSP_CONTROL_BASE as u32 + 4, 2), 0xabcd);
        assert_eq!(dsp.parser, super::DspBootParser::Running);
        assert_eq!(dsp.transmit(0x4001), Some(super::DspEvent::RuntimeReady));
    }

    #[test]
    fn dsp_signal_acknowledgement_clears_only_written_zero_bits() {
        let mut state = E830State::new(
            false,
            vec![0xff; super::EXTERNAL_FLASH_WINDOW_SIZE as usize],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.registers.insert(super::DSP_START_SIGNAL, 0x0003);
        state.write_register(super::DSP_START_SIGNAL, 2, 0xfffe);
        assert_eq!(state.read_register(super::DSP_START_SIGNAL, 2, 0), 0x0002);
        state.write_register(super::DSP_START_SIGNAL, 2, 0);
        assert_eq!(state.read_register(super::DSP_START_SIGNAL, 2, 0), 0);
    }

    #[test]
    fn dsp_result_acknowledgement_does_not_create_measurement_flags() {
        let mut state = E830State::new(
            false,
            vec![0xff; super::EXTERNAL_FLASH_WINDOW_SIZE as usize],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        let result = super::DSP_RESULT_SIGNAL;
        for value in [0xffff, 0xff7f, 0x0080, 0] {
            assert_eq!(state.write_register(result, 2, value), None);
            assert_eq!(state.read_register(result, 2, 0), 0);
            assert_eq!(state.pending_lines(), 0);
        }
        // Inject device state only in this unit test to exercise the original
        // ISR's complement acknowledgement, not to synthesize guest results.
        state.registers.insert(result, 0x0083);
        state.write_register(result, 2, 0xff7f);
        assert_eq!(state.read_register(result, 2, 0), 3);
        state.write_register(result, 2, 0xfffe);
        assert_eq!(state.read_register(result, 2, 0), 2);
        state.write_register(result, 2, 0);
        assert_eq!(state.read_register(result, 2, 0), 0);
    }

    #[test]
    fn dsp_result_acknowledgement_preserves_unwritten_bytes_and_other_signals() {
        let mut state = E830State::new(
            false,
            vec![0xff; super::EXTERNAL_FLASH_WINDOW_SIZE as usize],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        let result = super::DSP_RESULT_SIGNAL;
        state.registers.insert(result, 0xa55a_0183);
        state.registers.insert(super::DSP_START_SIGNAL, 3);
        state.registers.insert(super::DSP_RUNTIME_SIGNAL, 1);
        state.dsp_boot.runtime_completion_pending = true;
        state.write_register(result, 1, 0x7f);
        assert_eq!(state.read_register(result, 4, 0), 0xa55a_0103);
        state.write_register(result + 1, 1, 0xfe);
        assert_eq!(state.read_register(result, 4, 0), 0xa55a_0003);
        state.write_register(result, 2, 0);
        assert_eq!(state.read_register(result, 4, 0), 0xa55a_0000);
        assert_eq!(state.read_register(super::DSP_START_SIGNAL, 2, 0), 3);
        assert_eq!(state.read_register(super::DSP_RUNTIME_SIGNAL, 2, 0), 1);
        assert!(state.dsp_boot.runtime_completion_pending);
    }

    #[test]
    fn short_valid_vector_image_is_rejected_before_boot_alias_slicing() {
        let mut cla = vec![0; 0x200];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        assert!(matches!(
            E830Machine::new(firmware, false),
            Err(super::MachineError::ClaTooShortForBootMap)
        ));
    }

    #[test]
    fn boot_ready_gpio_is_asserted_for_the_reset_sequence() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        assert_eq!(
            state.read_register(SOC_BOOT_READY_INPUT_REGISTER, 4, 0),
            u64::from(SOC_BOOT_READY_INPUT_MASK)
        );
    }

    #[test]
    fn phone_contacts_raise_only_masked_controller_irq16_and_preserve_task() {
        use super::RegisterARM;
        let mut cla = vec![0xff; (super::IRQ_VECTOR_TARGET - u64::from(FLASH_BASE)) as usize + 0x1000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        cpu.mem_write(0x3000_1000, &[0x00, 0x00, 0xa0, 0xe1, 0xfd, 0xff, 0xff, 0xea]).unwrap();
        cpu.reg_write(RegisterARM::PC, 0x3000_1000).unwrap();
        cpu.reg_write(RegisterARM::CPSR, 0x1f).unwrap();
        cpu.reg_write(RegisterARM::R4, 0x1234_5678).unwrap();
        cpu.get_data_mut().timer_irq_period = 0;
        machine.set_phone_keys(crate::PhoneKey::Ok.bit());
        let state = machine.cpu.raw_engine().get_data_mut();
        assert_eq!(state.pending_lines(), 0);
        state.write_register(0x5030_8000, 1, 3);
        assert_eq!(state.pending_lines(), 1 << 16);
        assert_eq!(state.enabled_pending_lines(), 0);
        assert_eq!(machine.run(2).unwrap().irqs_taken, 0);
        let state = machine.cpu.raw_engine().get_data_mut();
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, u64::from(!(1u32 << 16)));
        let entered = machine.run(1).unwrap();
        assert_eq!(entered.irqs_taken, 1);
        assert_eq!(u64::from(entered.pc), super::IRQ_VECTOR_TARGET);
        assert_eq!(entered.r4, 0x1234_5678);
        let cpu = machine.cpu.raw_engine();
        assert_eq!(cpu.reg_read(RegisterARM::SPSR).unwrap(), 0x1f);
        assert_eq!(cpu.get_data().active_irq_line, 16);
        cpu.get_data_mut().write_register(0x5030_8008, 1, 7);
        assert_eq!(cpu.get_data().pending_lines(), 0);
    }

    #[test]
    fn keypad_scan_sees_released_lines_without_overwriting_gpio_configuration() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        let register = SOC_KEYPAD_INPUT_REGISTERS[0];
        state.write_register(register, 4, 0x0055_4027);
        assert_eq!(
            state.read_register(register, 4, SOC_KEYPAD_SCAN_READ_PCS[0]),
            u64::from(u32::MAX)
        );
        assert_eq!(state.read_register(register, 4, 0), 0x0055_4027);
    }

    #[test]
    fn system_timer_advances_independently_of_counter_reads() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.advance_system_timer(123 * SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER, 2, 0), 123);
        assert_eq!(state.read_register(SOC_TIMER, 2, 0), 123);
        state.advance_system_timer(130 * SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER, 2, 0), 130);
    }

    #[test]
    fn secondary_timer_counter_advances_at_provisional_eight_to_one_ratio() {
        let mut state = E830State::new(
            false, vec![0xff; 0x1000], Default::default(), Default::default(),
            Default::default(), Default::default(),
        );
        let base = super::SECONDARY_TIMER_BASES[0];
        state.advance_secondary_timers(7);
        assert_eq!(state.read_register(base + 8, 2, 0), 0);
        state.advance_secondary_timers(super::SECONDARY_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(base + 8, 2, 0), 1);
        state.advance_secondary_timers(17 * super::SECONDARY_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(base + 8, 2, 0), 17);
    }

    #[test]
    fn secondary_timer_compare_is_armed_by_control_and_sets_device_status() {
        let mut state = E830State::new(
            false, vec![0xff; 0x1000], Default::default(), Default::default(),
            Default::default(), Default::default(),
        );
        let base = super::SECONDARY_TIMER_BASES[0];
        let irq = 1 << super::SECONDARY_TIMER_IRQS[0];
        state.write_register(base + 0x10, 2, 2);
        state.write_register(base, 2, 0x20);
        state.advance_secondary_timers(2 * super::SECONDARY_TIMER_CPU_CLOCK_RATIO);
        assert_ne!(state.pending_lines() & irq, 0);
        assert_eq!(state.read_register(base + 0x1c, 1, 0), 4);
    }

    #[test]
    fn secondary_timer_irq18_persists_until_original_isr_style_clear() {
        let mut state = E830State::new(
            false, vec![0xff; 0x1000], Default::default(), Default::default(),
            Default::default(), Default::default(),
        );
        let base = super::SECONDARY_TIMER_BASES[0];
        let irq = 1 << super::SECONDARY_TIMER_IRQS[0];
        state.write_register(base + 0x10, 2, 2);
        state.write_register(base, 2, 0x20);
        state.advance_secondary_timers(2 * super::SECONDARY_TIMER_CPU_CLOCK_RATIO);
        assert_ne!(state.pending_lines() & irq, 0);
        assert_eq!(state.read_register(base + 0x1c, 1, 0), 4);

        state.write_register(base, 2, 0);
        assert_eq!(state.pending_lines() & irq, 0);
        assert_eq!(state.read_register(base + 0x1c, 1, 0), 4);
        state.write_register(base + 0x1c, 1, 0xfb);
        assert_eq!(state.read_register(base + 0x1c, 1, 0), 0);
        state.write_register(base, 2, 0x20);
        assert_eq!(state.pending_lines() & irq, 0); // no duplicate without crossing
    }

    #[test]
    fn secondary_timer_compare_crosses_wrap_and_keeps_irq17_isolated() {
        let mut state = E830State::new(
            false, vec![0xff; 0x1000], Default::default(), Default::default(),
            Default::default(), Default::default(),
        );
        let irq18 = 1 << super::SECONDARY_TIMER_IRQS[0];
        let irq17 = 1 << super::SECONDARY_TIMER_IRQS[1];
        let base = super::SECONDARY_TIMER_BASES[1];
        state.secondary_timer_counters[1] = 0xfffe;
        state.write_register(base + 0x10, 2, 1);
        state.write_register(base, 2, 0x20);
        state.advance_secondary_timers(3 * super::SECONDARY_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(base + 8, 2, 0), 1);
        assert_eq!(state.pending_lines() & irq18, 0);
        assert_ne!(state.pending_lines() & irq17, 0);
    }

    #[test]
    fn enabled_system_timer_wrap_sets_status_and_irq20() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.write_register(SOC_TIMER + 8, 2, 0x20);
        state.timer_counter = 0xfffe;
        state.advance_system_timer(2 * SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER, 2, 0), 0);
        assert_ne!(state.read_register(SOC_TIMER + 8, 2, 0) & 0x80, 0);
        assert_ne!(state.pending_lines() & (1 << SYSTEM_TIMER_IRQ), 0);
    }

    #[test]
    fn system_timer_wrap_preserves_compare_registers_without_match_mode() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.write_register(
            SOC_TIMER + 8,
            2,
            u64::from(SYSTEM_TIMER_IRQ20_ENABLE),
        );
        state.write_register(SOC_TIMER + 0x0c, 2, 5000);
        state.write_register(SOC_TIMER + 0x14, 2, 4700);
        state.timer_counter = 0xfffe;

        state.advance_system_timer(SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER + 0x0c, 2, 0), 5000);
        state.advance_system_timer(2 * SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER + 0x0c, 2, 0), 5000);
        assert_eq!(state.read_register(SOC_TIMER + 0x14, 2, 0), 4700);
    }

    #[test]
    fn system_timer_status_bits_are_write_zero_to_clear() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.registers.insert(
            SOC_TIMER + 8,
            SYSTEM_TIMER_MATCH0_ENABLE
                | SYSTEM_TIMER_MATCH1_ENABLE
                | SYSTEM_TIMER_IRQ20_ENABLE
                | SYSTEM_TIMER_IRQ21_ENABLE
                | SYSTEM_TIMER_IRQ20_STATUS
                | SYSTEM_TIMER_IRQ21_STATUS,
        );
        state.system_timer_irq_pending = true;
        state.system_timer_match_irq_pending = true;

        state.write_register(SOC_TIMER + 8, 2, 0x0760);
        assert_eq!(state.read_register(SOC_TIMER + 8, 2, 0), 0x0760);
        assert_eq!(state.pending_lines() & (1 << SYSTEM_TIMER_IRQ), 0);
        assert_ne!(state.pending_lines() & (1 << SYSTEM_TIMER_MATCH_IRQ), 0);

        state.write_register(SOC_TIMER + 8, 2, 0x06e0);
        assert_eq!(state.read_register(SOC_TIMER + 8, 2, 0), 0x0660);
        assert_eq!(state.pending_lines() & (1 << SYSTEM_TIMER_MATCH_IRQ), 0);
    }

    #[test]
    fn periodic_mode_write_preserves_but_does_not_create_timer_status() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.write_register(SOC_TIMER + 8, 2, 0x07e0);
        assert_eq!(state.read_register(SOC_TIMER + 8, 2, 0), 0x0660);
        assert_eq!(state.pending_lines() & (1 << SYSTEM_TIMER_IRQ), 0);
        assert_eq!(state.pending_lines() & (1 << SYSTEM_TIMER_MATCH_IRQ), 0);

        state.registers.insert(SOC_TIMER + 8, 0x0760);
        state.system_timer_match_irq_pending = true;
        state.write_register(SOC_TIMER + 8, 2, 0x07e0);
        assert_eq!(state.read_register(SOC_TIMER + 8, 2, 0), 0x0760);
        assert_ne!(state.pending_lines() & (1 << SYSTEM_TIMER_MATCH_IRQ), 0);
    }

    #[test]
    fn irq21_enable_without_match1_enable_does_not_compare() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.write_register(SOC_TIMER + 8, 2, u64::from(SYSTEM_TIMER_IRQ21_ENABLE));
        state.write_register(SOC_TIMER + 0x0c, 2, 5000);
        state.write_register(SOC_TIMER + 0x14, 2, 4700);
        state.timer_counter = 4699;

        state.advance_system_timer(SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER, 2, 0), 4700);
        assert_eq!(state.pending_lines() & (1 << SYSTEM_TIMER_MATCH_IRQ), 0);
    }

    #[test]
    fn observed_periodic_mode_raises_early_match_then_resets_at_period() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.write_register(
            SOC_TIMER + 8,
            2,
            u64::from(
                SYSTEM_TIMER_MATCH0_ENABLE
                    | SYSTEM_TIMER_MATCH1_ENABLE
                    | SYSTEM_TIMER_IRQ20_ENABLE
                    | SYSTEM_TIMER_IRQ21_ENABLE,
            ),
        );
        state.write_register(SOC_TIMER + 0x0c, 2, 5000);
        state.write_register(SOC_TIMER + 0x14, 2, 4700);
        state.timer_counter = 586 * 4;

        state.advance_system_timer(4114 * 4 * SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER, 2, 0), 4700 * 4);
        assert_eq!(state.pending_lines() & (1 << SYSTEM_TIMER_IRQ), 0);
        assert_ne!(state.pending_lines() & (1 << SYSTEM_TIMER_MATCH_IRQ), 0);

        state.write_register(SOC_TIMER + 8, 2, 0x06e0);
        state.advance_system_timer((4114 + 300) * 4 * SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER, 2, 0), 0);
        assert_ne!(state.pending_lines() & (1 << SYSTEM_TIMER_IRQ), 0);
        assert_eq!(state.pending_lines() & (1 << SYSTEM_TIMER_MATCH_IRQ), 0);
    }

    #[test]
    fn timer_compare_fields_preserve_fractional_counter_and_reach_delay_target() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.write_register(SOC_TIMER + 8, 2, 0x0660);
        // API preserves upper two register bits while writing the low 14.
        state.write_register(SOC_TIMER + 0x0c, 2, 0xc000 | 5000);
        state.write_register(SOC_TIMER + 0x14, 2, 0x8000 | 4700);
        state.advance_system_timer(7835 * SYSTEM_TIMER_CPU_CLOCK_RATIO);
        let raw = state.read_register(SOC_TIMER, 2, 0);
        assert_eq!(raw, 7835);
        assert_eq!(raw >> 2, 1958);
        assert_eq!(raw & 3, 3);
        assert_eq!(state.pending_lines(), 0);
        state.advance_system_timer(18800 * SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER, 2, 0), 18800);
        assert_ne!(state.pending_lines() & (1 << SYSTEM_TIMER_MATCH_IRQ), 0);
        state.advance_system_timer(20000 * SYSTEM_TIMER_CPU_CLOCK_RATIO);
        assert_eq!(state.read_register(SOC_TIMER, 2, 0), 0);
        assert_ne!(state.pending_lines() & (1 << SYSTEM_TIMER_IRQ), 0);
        assert_eq!(state.read_register(SOC_TIMER + 0x0c, 2, 0), 0xc000 | 5000);
    }

    #[test]
    fn system_timer_irq_persists_until_peripheral_status_is_cleared() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.write_register(SOC_TIMER + 8, 2, 0x20);
        state.timer_counter = 0xffff;
        state.advance_system_timer(SYSTEM_TIMER_CPU_CLOCK_RATIO);
        state.active_irq_line = SYSTEM_TIMER_IRQ;
        state.timer_irq_in_service = true;

        state.write_register(SOC_INIT_CONTROL_REGISTER, 1, 0x80);
        assert!(state.timer_irq_in_service);
        assert!(state.irq_eoi_seen);
        assert_ne!(state.pending_lines() & (1 << SYSTEM_TIMER_IRQ), 0);

        state.write_register(SOC_TIMER + 8, 2, 0x120);
        assert_eq!(state.pending_lines() & (1 << SYSTEM_TIMER_IRQ), 0);
        assert_eq!(state.read_register(SOC_TIMER + 8, 2, 0) & 0x80, 0);
    }

    #[test]
    fn dsp_serial_completion_uses_device_irq_and_w0c_acknowledgement() {
        let mut state = E830State::new(
            false, vec![0xff; 0x1000], Default::default(),
            Default::default(), Default::default(), Default::default(),
        );
        let base = super::DSP_SERIAL_BASE;
        let irq = 1 << super::DSP_SERIAL_IRQ;
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, 0);
        state.dsp_queue_hle_phase = super::DspQueueHlePhase::CompletePending { deadline: 4096 };
        state.advance_dsp_serial(4095);
        assert_eq!(state.pending_lines(), 0);
        state.advance_dsp_serial(4096);
        assert_eq!(state.read_register(base + 0x14, 2, 0), 4);
        assert_eq!(state.pending_lines(), 0); // peripheral IRQ disabled
        state.write_register(base + 0x0c, 2, 2);
        assert_eq!(state.enabled_pending_lines(), irq);
        state.write_register(base + 0x0c, 2, 1); // genuine 32-bit start path
        assert_eq!(state.enabled_pending_lines(), irq);
        assert_eq!(state.read_register(super::INTC_PENDING_MASK_REGISTER, 4, 0), 0);
        state.active_irq_line = super::DSP_SERIAL_IRQ;
        state.timer_irq_in_service = true;
        assert_eq!(state.read_register(super::INTC_PENDING_MASK_REGISTER, 4, 0), u64::from(irq));
        state.write_register(base + 0x0c, 2, 0); // original ISR disables source
        assert_eq!(state.pending_lines(), 0);
        assert_eq!(state.read_register(base + 0x14, 2, 0), 4);
        state.write_register(base + 0x14, 2, 0x3fc); // ones preserve status
        assert_eq!(state.read_register(base + 0x14, 2, 0), 4);
        state.write_register(base + 0x14, 2, 0x3f8); // actual ISR acknowledgement
        assert_eq!(state.read_register(base + 0x14, 2, 0), 0);
        assert_eq!(state.dsp_queue_hle_phase, super::DspQueueHlePhase::Idle);
        state.advance_dsp_serial(8192);
        state.write_register(base + 0x0c, 2, 2);
        assert_eq!(state.pending_lines(), 0); // no spontaneous duplicate
    }

    #[test]
    fn dsp_serial_packed_one_word_completion_obeys_masks_and_acknowledgement() {
        let mut state = E830State::new(
            false, vec![0xff; 0x1000], Default::default(),
            Default::default(), Default::default(), Default::default(),
        );
        let base = super::DSP_SERIAL_BASE;
        let irq = 1 << super::DSP_SERIAL_IRQ;
        // 204d03e8..ec: packed entry[+4]=1 enables 2 << 1, not a FIFO bit.
        state.write_register(base + 0x0c, 2, 4);
        state.dsp_queue_hle_phase = super::DspQueueHlePhase::CompletePending { deadline: 4096 };
        state.advance_dsp_serial(4095);
        assert_eq!(state.pending_lines(), 0);
        state.advance_dsp_serial(4096);
        assert_eq!(state.read_register(base + 0x14, 2, 0), 4);
        assert_eq!(state.pending_lines(), irq);
        assert_eq!(state.enabled_pending_lines(), 0); // INTC still masked
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, u64::from(!irq));
        assert_eq!(state.enabled_pending_lines(), irq);
        state.write_register(base + 0x0c, 2, 0); // original ISR disables source
        assert_eq!(state.pending_lines(), 0);
        state.write_register(base + 0x14, 2, 0x3f8); // original W0C acknowledgement
        assert_eq!(state.dsp_queue_hle_phase, super::DspQueueHlePhase::Idle);
        state.write_register(base + 0x0c, 2, 4);
        state.advance_dsp_serial(8192);
        assert_eq!(state.pending_lines(), 0); // no duplicate completion
        // Do not infer unobserved longer packed transfers from this one-word case.
        state.registers.insert(base + 0x14, 4);
        state.write_register(base + 0x0c, 2, 8);
        assert_eq!(state.pending_lines(), 0);
    }

    #[test]
    fn analog_hle_uses_delayed_serial_completion_and_retains_sample_after_ack() {
        let mut state = E830State::new(
            false, vec![0xff; 0x1000], Default::default(),
            Default::default(), Default::default(), Default::default(),
        );
        state.analog_hle = crate::analog_hle::AnalogHle::new(true);
        state.dsp_boot.hle_phase = super::DspHlePhase::E830RuntimeReady;
        let base = super::DSP_SERIAL_BASE;
        let irq = 1 << super::DSP_SERIAL_IRQ;
        state.write_register(base + 0x18, 2, 123); // preserve baseline register
        state.write_register(base + 4, 4, 0x3b);
        state.write_register(base + 8, 4, 0);
        state.write_register(base + 0x100, 4, 0xec0);
        state.write_register(base + 0x0c, 2, 4);
        state.advance_dsp_serial(8192); // submission alone cannot create an IRQ
        assert_eq!(state.pending_lines(), 0);
        assert_eq!(state.read_register(base + 0x18, 2, 0), 123);
        state.dsp_queue_hle_phase = super::DspQueueHlePhase::CompletePending { deadline: 10000 };
        state.advance_dsp_serial(9999);
        assert_eq!(state.read_register(base + 0x18, 2, 0), 123);
        state.advance_dsp_serial(10000);
        assert_eq!(state.read_register(base + 0x18, 2, 0), 1000);
        assert_eq!(state.pending_lines(), irq);
        assert_eq!(state.enabled_pending_lines(), 0);
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, u64::from(!irq));
        assert_eq!(state.enabled_pending_lines(), irq);
        state.write_register(base + 0x0c, 2, 0);
        state.write_register(base + 0x14, 2, 0x3f8);
        assert_eq!(state.pending_lines(), 0);
        assert_eq!(state.dsp_queue_hle_phase, super::DspQueueHlePhase::Idle);
        assert_eq!(state.read_register(base + 0x18, 2, 0), 1000);
        assert_eq!(state.read_register(base + 0x19, 1, 0), 3);
        state.advance_dsp_serial(20000);
        assert_eq!(state.pending_lines(), 0);
        // The next, unsupported ADC address must see the old baseline, not 1000.
        state.write_register(base + 4, 4, 0x3d);
        state.write_register(base + 0x100, 4, 0xf40);
        state.write_register(base + 0x0c, 2, 4);
        state.dsp_queue_hle_phase = super::DspQueueHlePhase::CompletePending { deadline: 24096 };
        state.advance_dsp_serial(24096);
        assert_eq!(state.read_register(base + 0x18, 2, 0), 123);
        assert_eq!(state.registers.get(&(base + 0x18)), Some(&123));
        state.write_register(base + 4, 4, 0x3b);
        assert_eq!(state.read_register(base + 0x18, 2, 0), 123); // old result invalidated
    }

    #[test]
    fn analog_hle_gates_unknown_images_default_off_and_misaligned_controls() {
        for (enabled, active, offset, size) in [
            (false, true, 0, 2), (true, false, 0, 2),
            (true, true, 1, 2), (true, true, 0, 1), (true, true, 0, 4),
        ] {
            let mut state = E830State::new(
                false, vec![0xff; 0x1000], Default::default(),
                Default::default(), Default::default(), Default::default(),
            );
            state.analog_hle = crate::analog_hle::AnalogHle::new(enabled);
            if active { state.dsp_boot.hle_phase = super::DspHlePhase::E830RuntimeReady; }
            let base = super::DSP_SERIAL_BASE;
            state.write_register(base + 0x18, 2, 123);
            state.write_register(base + 4, 4, 0x3b);
            state.write_register(base + 0x100, 4, 0xec0);
            state.write_register(base + 0x0c + offset, size, 4);
            state.dsp_queue_hle_phase = super::DspQueueHlePhase::CompletePending { deadline: 4096 };
            state.advance_dsp_serial(4096);
            assert_eq!(state.read_register(base + 0x18, 2, 0), 123);
        }
    }

    #[test]
    fn analog_hle_observes_guest_control_stores_without_writing_ram_or_cpu_state() {
        let mut cla = vec![0xff; 0x20000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        cpu.mem_write(0x30001000, &[0x08, 0x80]).unwrap(); // STRH r0,[r1]
        cpu.mem_write(0x30491b40, &[0xa5; 32]).unwrap(); // intermediate ADC words
        cpu.mem_write(0x301ce470, &[0x5a; 32]).unwrap(); // original checked message
        cpu.reg_write(super::RegisterARM::PC, 0x30001000).unwrap();
        cpu.reg_write(super::RegisterARM::CPSR, 0xbf).unwrap();
        cpu.reg_write(super::RegisterARM::R0, 4).unwrap();
        cpu.reg_write(super::RegisterARM::R1, u64::from(super::DSP_SERIAL_BASE + 0x0c)).unwrap();
        cpu.reg_write(super::RegisterARM::R4, 0x12345678).unwrap();
        cpu.reg_write(super::RegisterARM::SP, 0x30002000).unwrap();
        let state = cpu.get_data_mut();
        state.analog_hle = crate::analog_hle::AnalogHle::new(true);
        state.dsp_boot.hle_phase = super::DspHlePhase::E830RuntimeReady;
        state.write_register(super::DSP_SERIAL_BASE + 4, 4, 0x3b);
        state.write_register(super::DSP_SERIAL_BASE + 0x100, 4, 0xec0);
        machine.run(1).unwrap();
        let cpu = machine.cpu.raw_engine();
        let state = cpu.get_data_mut();
        assert_eq!(state.read_register(super::DSP_SERIAL_BASE + 0x18, 2, 0), 0);
        state.dsp_queue_hle_phase = super::DspQueueHlePhase::CompletePending { deadline: 4096 };
        state.advance_dsp_serial(4096);
        assert_eq!(state.read_register(super::DSP_SERIAL_BASE + 0x18, 2, 0), 1000);
        assert_eq!(cpu.mem_read_as_vec(0x30491b40, 32).unwrap(), [0xa5; 32]);
        assert_eq!(cpu.mem_read_as_vec(0x301ce470, 32).unwrap(), [0x5a; 32]);
        assert_eq!(cpu.reg_read(super::RegisterARM::CPSR).unwrap(), 0xbf);
        assert_eq!(cpu.reg_read(super::RegisterARM::R0).unwrap(), 4);
        assert_eq!(cpu.reg_read(super::RegisterARM::R4).unwrap(), 0x12345678);
        assert_eq!(cpu.reg_read(super::RegisterARM::SP).unwrap(), 0x30002000);
    }

    #[test]
    fn lcd_revision_exposes_partial_writes_without_changing_frame_count() {
        let mut cla = vec![0xff; super::FLASH_ALIAS_OFFSET + 0x1000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        assert_eq!(machine.lcd_write_revision(), 0);
        let state = machine.cpu.raw_engine().get_data_mut();
        state.lcd_frames = 1;
        state.lcd_pixels_since_frame = 5;
        assert_eq!(machine.lcd_write_revision(), super::LCD_PIXEL_COUNT as u64 + 5);
        assert_eq!(machine.cpu.raw_engine().get_data().lcd_frames, 1);
    }

    #[test]
    fn lcd_partial_window_uses_original_packed_coordinates_and_wraps_inside_it() {
        let mut state = E830State::new(false, vec![0xff; 0x1000],
            Default::default(), Default::default(), Default::default(), Default::default());
        let bus = super::SOC_EXTERNAL_CONTROL_BASE as u32;
        let register = |state: &mut E830State, index, value| {
            state.write_lcd_register(bus, 2, 0);
            state.write_lcd_register(bus + 2, 2, index);
            state.write_lcd_register(bus, 2, 1);
            state.write_lcd_register(bus + 2, 2, value);
        };
        // 2119f1d0: x bounds 2..3, y bounds 4..5, cursor (2,4).
        register(&mut state, 3, 0x1030);
        register(&mut state, 0x44, 0x0302);
        register(&mut state, 0x45, 0x0504);
        register(&mut state, 0x21, 0x0402);
        state.write_lcd_register(bus, 2, 0);
        state.write_lcd_register(bus + 2, 2, 0x22);
        state.write_lcd_register(bus, 2, 1);
        for pixel in 1..=5 { state.write_lcd_register(bus + 2, 2, pixel); }
        let at = 4 * super::LCD_WIDTH + 2;
        assert_eq!(&state.lcd_framebuffer[at..at + 2], &[5, 2]);
        assert_eq!(&state.lcd_framebuffer[at + super::LCD_WIDTH..at + super::LCD_WIDTH + 2], &[3, 4]);
        assert_eq!(state.lcd_framebuffer.iter().filter(|&&pixel| pixel != 0).count(), 4);
        assert_eq!(state.lcd_registers[0x21], 0x0403);
        assert_eq!(state.lcd_frames, 0);
        assert_eq!(state.lcd_pixels_since_frame, 5);
        // Register programming and partial updates do not erase the image.
        register(&mut state, 0x21, 0x0402);
        assert_eq!(state.lcd_framebuffer[at], 5);
        register(&mut state, 0x21, 0x04ff); // Invalid x must not alias next row.
        assert_eq!(state.lcd_pixel_cursor, super::LCD_PIXEL_COUNT);
    }

    #[test]
    fn lcd_full_window_preserves_frame_count_and_rejects_invalid_addressing() {
        let mut state = E830State::new(false, vec![0xff; 0x1000],
            Default::default(), Default::default(), Default::default(), Default::default());
        let bus = super::SOC_EXTERNAL_CONTROL_BASE as u32;
        state.write_lcd_register(bus + 2, 2, 0x22);
        state.write_lcd_register(bus, 2, 1);
        for _ in 0..super::LCD_PIXEL_COUNT { state.write_lcd_register(bus + 2, 2, 0xabcd); }
        assert_eq!(state.lcd_frames, 1);
        assert_eq!(state.lcd_pixels_since_frame, 0);
        assert_eq!(state.lcd_pixel_cursor, 0);
        assert!(state.lcd_framebuffer.iter().all(|&pixel| pixel == 0xabcd));
        let expected = state.lcd_framebuffer.clone();
        for (window_x, window_y, cursor, mode) in [
            (0xff00, 0xdb00, 0, 0x1030), // x outside physical LCD
            (0xaf00, 0xff00, 0, 0x1030), // y outside physical LCD
            (0x0203, 0xdb00, 0, 0x1030), // reversed bounds
            (0xaf00, 0xdb00, 0xffff, 0x1030), // cursor outside LCD
            (0x0302, 0x0504, 0, 0x1030), // cursor outside window
            (0xaf00, 0xdb00, 0, 0x1020), // unsupported scan direction
        ] {
            state.lcd_registers[0x44] = window_x;
            state.lcd_registers[0x45] = window_y;
            state.lcd_pixel_cursor = cursor;
            state.lcd_registers[3] = mode;
            state.write_lcd_register(bus + 2, 2, 1);
            assert_eq!(state.lcd_framebuffer, expected);
            assert_eq!(state.lcd_frames, 1);
            assert_eq!(state.lcd_pixels_since_frame, 0);
        }
        state.write_lcd_register(bus + 2, 4, 1); // unsupported bus width
        assert_eq!(state.lcd_framebuffer, expected);
    }

    #[test]
    fn uart0_dma_moves_real_bytes_and_preserves_cpu_and_guest_state() {
        use super::RegisterARM;
        let mut cla = vec![0xff; 0x20000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        let source = 0x3000_1000u32;
        let base = super::DMA_BASE + 0x160;
        cpu.mem_write(u64::from(source), b"original-data").unwrap();
        cpu.reg_write(RegisterARM::R4, 0x1234_5678).unwrap();
        cpu.reg_write(RegisterARM::SP, 0x3000_2000).unwrap();
        let state = cpu.get_data_mut();
        state.write_register(super::DMA_BASE + 0x30, 4, 1);
        state.write_register(base, 4, u64::from(source));
        state.write_register(base + 4, 4, u64::from(super::UART_BASES[0]));
        state.write_register(base + 8, 4, 0);
        state.write_register(base + 0x0c, 4, u64::from(super::DMA_UART0_TX_CONTROL | 13));
        state.write_register(base + 0x10, 4, u64::from(super::DMA_UART0_TX_CONFIG));
        let deadline = 13 * super::DMA_BYTE_INSTRUCTIONS;
        state.instructions = deadline - 1;
        super::advance_dma(cpu);
        assert!(cpu.get_data().uarts[0].output().is_empty());
        cpu.get_data_mut().instructions = deadline;
        super::advance_dma(cpu);
        assert_eq!(cpu.get_data().uarts[0].output(), b"original-data");
        assert_eq!(cpu.mem_read_as_vec(u64::from(source), 13).unwrap(), b"original-data");
        assert_eq!(cpu.reg_read(RegisterARM::R4).unwrap(), 0x1234_5678);
        assert_eq!(cpu.reg_read(RegisterARM::SP).unwrap(), 0x3000_2000);
        let state = cpu.get_data_mut();
        assert_eq!(state.read_register(base, 4, 0), u64::from(source + 13));
        assert_eq!(state.read_register(base + 4, 4, 0), u64::from(super::UART_BASES[0]));
        assert_eq!(state.read_register(base + 0x0c, 4, 0), u64::from(super::DMA_UART0_TX_CONTROL));
        assert_eq!(state.read_register(base + 0x10, 4, 0), 0x8840);
        assert_eq!(state.read_register(super::DMA_BASE, 4, 0), 8);
        assert_eq!(state.pending_lines(), 1 << super::DMA_IRQ);
        assert_eq!(state.enabled_pending_lines(), 0); // INTC mask still active.
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, u64::from(!(1u32 << super::DMA_IRQ)));
        assert_eq!(state.enabled_pending_lines(), 1 << super::DMA_IRQ);
        state.write_register(super::DMA_BASE + 0x14, 4, 0); // status is read-only
        state.write_register(super::DMA_BASE + 8, 4, 1); // another channel's W1C
        assert_eq!(state.dma_interrupt_status(), 8);
        state.irq_eoi_seen = true;
        assert_eq!(state.dma_interrupt_status(), 8); // EOI is not a DMA ack.
        state.write_register(base + 0x10, 4, 0x840); // source mask, raw stays
        assert_eq!(state.pending_lines(), 0);
        assert_eq!(state.read_register(super::DMA_BASE + 0x14, 4, 0), 8);
        state.write_register(base + 0x10, 4, 0x8840);
        state.write_register(super::DMA_BASE + 8, 4, 8);
        assert_eq!(state.pending_lines(), 0);
        super::advance_dma(cpu);
        assert_eq!(cpu.get_data().uarts[0].output(), b"original-data"); // no duplicate
    }

    #[test]
    fn uart0_dma_completion_waits_for_architectural_irq29_entry() {
        use super::RegisterARM;
        let mut cla = vec![0xff; (super::IRQ_VECTOR_TARGET - u64::from(FLASH_BASE)) as usize + 0x1000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let task_pc = 0x3000_1200u32;
        let source = 0x3000_1000u32;
        let stack = 0x3000_2000u32;
        let base = super::DMA_BASE + 0x160;
        let cpu = machine.cpu.raw_engine();
        cpu.mem_write(u64::from(task_pc), &[0x00, 0x46, 0xfd, 0xe7]).unwrap();
        cpu.mem_write(u64::from(source), b"DMA").unwrap();
        cpu.mem_write(u64::from(stack), &[0x5a; 32]).unwrap();
        let state = cpu.get_data_mut();
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, 0);
        state.write_register(super::DMA_BASE + 0x30, 4, 1);
        state.write_register(base, 4, u64::from(source));
        state.write_register(base + 4, 4, u64::from(super::UART_BASES[0]));
        state.write_register(base + 0x0c, 4, u64::from(super::DMA_UART0_TX_CONTROL | 3));
        state.write_register(base + 0x10, 4, u64::from(super::DMA_UART0_TX_CONFIG));
        cpu.reg_write(RegisterARM::CPSR, 0xbf).unwrap(); // Task masks CPU IRQs.
        cpu.reg_write(RegisterARM::PC, u64::from(task_pc | 1)).unwrap();
        cpu.reg_write(RegisterARM::R4, 0x1234_5678).unwrap();
        cpu.reg_write(RegisterARM::SP, u64::from(stack)).unwrap();
        cpu.reg_write(RegisterARM::LR, 0x3000_3001).unwrap();

        let report = machine.run(3 * super::DMA_BYTE_INSTRUCTIONS + 8).unwrap();
        assert!(report.memory_fault.is_none());
        assert_eq!(report.irqs_taken, 0);
        assert!(matches!(report.pc, 0x3000_1200 | 0x3000_1202));
        assert_eq!(report.r4, 0x1234_5678);
        assert_eq!(report.sp, stack);
        assert_eq!(report.lr, 0x3000_3001);
        let cpu = machine.cpu.raw_engine();
        assert_eq!(cpu.get_data().uarts[0].output(), b"DMA");
        assert_eq!(cpu.get_data_mut().enabled_pending_lines(), 1 << super::DMA_IRQ);
        assert_eq!(cpu.mem_read_as_vec(u64::from(stack), 32).unwrap(), [0x5a; 32]);
        assert_eq!(cpu.mem_read_as_vec(u64::from(source), 3).unwrap(), b"DMA");

        // Only normal CPU IRQ entry may switch mode/stack and call the ISR.
        cpu.reg_write(RegisterARM::CPSR, 0x3f).unwrap();
        machine.set_breakpoint(super::IRQ_VECTOR_TARGET as u32, 0);
        let report = machine.run(4).unwrap();
        assert_eq!(report.stop_reason, super::StopReason::Breakpoint);
        assert_eq!(u64::from(report.pc), super::IRQ_VECTOR_TARGET);
        assert_eq!(report.irqs_taken, 1);
        assert_eq!(report.cpsr & 0x9f, 0x92);
        assert_eq!(report.r4, 0x1234_5678);
        let cpu = machine.cpu.raw_engine();
        assert_eq!(cpu.get_data().active_irq_line, super::DMA_IRQ);
        assert_eq!(cpu.reg_read(RegisterARM::SPSR).unwrap(), 0x3f);
        assert_eq!(cpu.mem_read_as_vec(u64::from(stack), 32).unwrap(), [0x5a; 32]);
        assert_eq!(cpu.get_data().uarts[0].output(), b"DMA");
    }

    #[test]
    fn dma_rejects_rx_linked_wide_and_out_of_ram_transfers() {
        let mut state = E830State::new(false, vec![0xff; 0x1000],
            Default::default(), Default::default(), Default::default(), Default::default());
        let base = super::DMA_BASE + 0x160;
        for (source, destination, link, control, config, enabled) in [
            (0x50403000, 0x30001000, 0, 0x8800000d, 0x9005, 1), // RX: no cable data
            (0x30001000, 0x50403000, 0x30002000, 0x8400000d, 0x8841, 1),
            (0x30001000, 0x50403000, 0, 0x8440000d, 0x8841, 1),
            (0x31fffffc, 0x50403000, 0, 0x8400000d, 0x8841, 1),
            (0x2ffffffc, 0x50403000, 0, 0x8400000d, 0x8841, 1),
            (0x30001000, 0x50403000, 0, 0x84000000, 0x8841, 1),
            (0x30001000, 0x50403000, 0, 0x8400000d, 0x8840, 1),
            (0x30001000, 0x50403000, 0, 0x8400000d, 0x8841, 0),
        ] {
            state.write_register(super::DMA_BASE + 0x30, 4, enabled);
            for (offset, value) in [(0, source), (4, destination), (8, link), (12, control), (16, config)] {
                state.write_register(base + offset, 4, value);
            }
            assert_eq!(state.dma_uart0_tx(3), None);
            assert_eq!(state.dma_deadlines[3], None);
            assert_eq!(state.pending_lines(), 0);
        }
    }

    #[test]
    fn sim_dma_uses_ram_without_inventing_card_exchange_completion() {
        let mut cla = vec![0xff; 0x20000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        let source = 0x3000_1000u32;
        cpu.mem_write(u64::from(source), &[0x7f, 0x20]).unwrap();
        cpu.reg_write(super::RegisterARM::R4, 0x1234_5678).unwrap();
        let state = cpu.get_data_mut();
        state.i2c = crate::i2c::I2c::with_sim_present(true);
        state.i2c.write(0, 0x112, 0);
        state.i2c.write(0, 0x2c, 0);
        state.i2c.write(0, 0x21c, 0);
        state.i2c.advance(10_000);
        state.sim.advance(0, true);
        for (offset, value) in [(0x0c, 0xf0), (0x800, 3), (0x804, 2),
            (0x808, 0xa0), (0x80c, 0xa4), (0x818, 2), (0xfe4, 0x800)] {
            state.write_register(0x50406000 + offset, 2, value);
        }
        let base = super::DMA_BASE + 0x1a0;
        state.write_register(super::DMA_BASE + 0x30, 4, 1);
        for (offset, value) in [(0, source), (4, 0x50406034), (8, 0),
            (12, 0x84000002), (16, super::DMA_SIM_TX_CONFIG)] {
            state.write_register(base + offset, 4, u64::from(value));
        }
        assert_eq!(state.dma_sim_tx(5), Some((source, 2)));
        state.instructions = 2 * super::DMA_BYTE_INSTRUCTIONS;
        super::advance_dma(cpu);
        let state = cpu.get_data_mut();
        assert_eq!(state.read_register(base, 4, 0), u64::from(source + 2));
        assert_eq!(state.pending_lines(), 1 << super::DMA_IRQ);
        assert_eq!(state.read_register(0x50406fe0, 2, 0), 0);
        state.sim.advance(100, true); // DMA completed; card response not due yet
        assert_eq!(state.pending_lines(), 1 << super::DMA_IRQ);
        assert_eq!(state.read_register(0x5040681c, 2, 0), 0);
        state.write_register(super::DMA_BASE + 8, 4, 1 << 5);
        assert_eq!(state.pending_lines(), 0);
        assert_eq!(cpu.reg_read(super::RegisterARM::R4).unwrap(), 0x1234_5678);
        assert_eq!(cpu.mem_read_as_vec(u64::from(source), 2).unwrap(), [0x7f, 0x20]);
    }

    #[test]
    fn sim_dma_rejects_absent_unpowered_linked_wide_rx_and_wrong_request() {
        let mut state = E830State::new(false, vec![0xff; 0x1000],
            Default::default(), Default::default(), Default::default(), Default::default());
        let base = super::DMA_BASE + 0x1a0;
        for (offset, value) in [(0, 0x30001000), (4, 0x50406034), (8, 0),
            (12, 0x84000002), (16, 0xaa41)] {
            state.write_register(base + offset, 4, value);
        }
        state.write_register(super::DMA_BASE + 0x30, 4, 1);
        assert_eq!(state.dma_sim_tx(5), None); // opt-in absent
        assert_eq!(state.dma_deadlines[5], None);
        state.i2c = crate::i2c::I2c::with_sim_present(true);
        assert_eq!(state.dma_sim_tx(5), None); // present but no power
        state.i2c.write(0, 0x112, 0);
        state.i2c.write(0, 0x2c, 0);
        state.i2c.write(0, 0x21c, 0);
        state.i2c.advance(10_000);
        for (source, destination, link, control, config, enabled) in [
            (0x30001000, 0x50406034, 0x30002000, 0x84000002, 0xaa41, 1),
            (0x30001000, 0x50406034, 0, 0x84400002, 0xaa41, 1),
            (0x50406034, 0x30001000, 0, 0x88000002, 0x900d, 1),
            (0x30001000, 0x50406034, 0, 0x84000002, 0x8841, 1),
            (0x31ffffff, 0x50406034, 0, 0x84000002, 0xaa41, 1),
            (0x2fffffff, 0x50406034, 0, 0x84000002, 0xaa41, 1),
            (0x30001000, 0x50406034, 0, 0x84000000, 0xaa41, 1),
            (0x30001000, 0x50406034, 0, 0x84000002, 0xaa40, 1),
            (0x30001000, 0x50406034, 0, 0x84000002, 0xaa41, 0),
        ] {
            state.write_register(super::DMA_BASE + 0x30, 4, enabled);
            for (offset, value) in [(0, source), (4, destination), (8, link), (12, control), (16, config)] {
                state.write_register(base + offset, 4, value);
            }
            assert_eq!(state.dma_sim_tx(5), None);
            assert_eq!(state.dma_deadlines[5], None);
            assert_eq!(state.pending_lines(), 0);
        }
    }

    #[test]
    fn sim_rx_dma_moves_only_requested_data_and_preserves_task_context() {
        let mut cla = vec![0xff; 0x20000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        let destination = 0x3000_1000u32;
        let stack = 0x3000_2000u32;
        cpu.mem_write(u64::from(destination - 1), &[0x5a; 26]).unwrap();
        cpu.mem_write(u64::from(stack), &[0xa5; 32]).unwrap();
        for (register, value) in [(super::RegisterARM::CPSR, 0xbf),
            (super::RegisterARM::R4, 0x12345678), (super::RegisterARM::SP, stack),
            (super::RegisterARM::LR, 0x30003001)] {
            cpu.reg_write(register, u64::from(value)).unwrap();
        }
        let state = cpu.get_data_mut();
        state.i2c = crate::i2c::I2c::with_sim_present(true);
        for value in [0x112, 0x2c, 0x21c] { state.i2c.write(0, value, 0); }
        state.i2c.advance(10_000);
        state.sim.advance(0, true);
        for (offset, value) in [(0x0c, 0xf0), (0x800, 2), (0x804, 22),
            (0x808, 0xa0), (0x80c, 0xc0), (0x818, 22), (0xfe4, 0x800)] {
            state.write_register(0x50406000 + offset, 2, value);
        }
        let base = super::DMA_BASE + 0x1a0;
        state.write_register(super::DMA_BASE + 0x30, 4, 1);
        for (offset, value) in [(0, 0x50406034), (4, destination), (8, 0),
            (12, 0x88000016), (16, super::DMA_SIM_RX_CONFIG)] {
            state.write_register(base + offset, 4, u64::from(value));
        }
        state.instructions = 22 * super::DMA_BYTE_INSTRUCTIONS;
        super::advance_dma(cpu); // initiates card exchange; no data yet
        assert_eq!(cpu.mem_read_as_vec(u64::from(destination - 1), 26).unwrap(), [0x5a; 26]);
        assert_eq!(cpu.get_data().pending_lines(), 0);
        let state = cpu.get_data_mut();
        state.instructions += 256;
        state.sim.advance(state.instructions, true);
        super::advance_dma(cpu);
        let state = cpu.get_data_mut();
        assert_eq!(state.pending_lines(), 1 << super::DMA_IRQ);
        assert_eq!(state.read_register(base + 4, 4, 0), u64::from(destination + 22));
        assert_eq!(state.read_register(base + 12, 4, 0), 0x88000000);
        assert_eq!(state.read_register(base + 16, 4, 0), 0xb012);
        assert_eq!(state.read_register(0x50406820, 1, 0), 0x90);
        assert_eq!(state.read_register(0x5040681c, 2, 0), 22);
        assert_eq!(state.read_register(0x50406fe0, 2, 0), 0);
        state.write_register(super::DMA_BASE + 8, 4, 1 << 5);
        super::advance_dma(cpu);
        assert_eq!(cpu.get_data().pending_lines(), 0); // completion consumed once
        assert_eq!(cpu.mem_read_as_vec(u64::from(destination - 1), 26).unwrap(),
            [0x5a, 0, 0, 0, 0, 0x3f, 0, 1, 0, 0, 0, 0, 0,
                9, 0x80, 2, 2, 2, 0, 0x83, 0x8a, 0x83, 0x8a, 0x5a, 0x5a, 0x5a]);
        assert_eq!(cpu.mem_read_as_vec(u64::from(stack), 32).unwrap(), [0xa5; 32]);
        for (register, value) in [(super::RegisterARM::CPSR, 0xbf),
            (super::RegisterARM::R4, 0x12345678), (super::RegisterARM::SP, stack),
            (super::RegisterARM::LR, 0x30003001)] {
            assert_eq!(cpu.reg_read(register).unwrap(), u64::from(value));
        }
    }

    #[test]
    fn sim_rx_dma_rejects_unsupported_descriptors_and_unpowered_card() {
        let mut state = E830State::new(false, vec![0xff; 0x1000],
            Default::default(), Default::default(), Default::default(), Default::default());
        let base = super::DMA_BASE + 0x1a0;
        state.write_register(super::DMA_BASE + 0x30, 4, 1);
        for (offset, value) in [(0, 0x50406034), (4, 0x30001000), (8, 0),
            (12, 0x88000016), (16, super::DMA_SIM_RX_CONFIG)] {
            state.write_register(base + offset, 4, u64::from(value));
        }
        assert_eq!(state.dma_sim_rx(5), None);
        assert_eq!(state.dma_deadlines[5], None);
        state.i2c = crate::i2c::I2c::with_sim_present(true);
        assert_eq!(state.dma_sim_rx(5), None);
        for value in [0x112, 0x2c, 0x21c] { state.i2c.write(0, value, 0); }
        state.i2c.advance(10_000);
        assert_eq!(state.dma_sim_rx(5), Some((0x30001000, 22)));
        for (source, destination, link, control, config, enabled) in [
            (0x50404000, 0x30001000, 0, 0x88000016, 0xb013, 1),
            (0x50406034, 0x30001000, 0x30002000, 0x88000016, 0xb013, 1),
            (0x50406034, 0x30001000, 0, 0x88400016, 0xb013, 1),
            (0x50406034, 0x30001000, 0, 0x88000016, 0x9009, 1),
            (0x50406034, 0x31ffffff, 0, 0x88000016, 0xb013, 1),
            (0x50406034, 0x2fffffff, 0, 0x88000016, 0xb013, 1),
            (0x50406034, 0x30001000, 0, 0x88000000, 0xb013, 1),
            (0x50406034, 0x30001000, 0, 0x88000016, 0xb012, 1),
            (0x50406034, 0x30001000, 0, 0x88000016, 0xb013, 0),
        ] {
            state.write_register(super::DMA_BASE + 0x30, 4, enabled);
            for (offset, value) in [(0, source), (4, destination), (8, link),
                (12, control), (16, config)] { state.write_register(base + offset, 4, value); }
            assert_eq!(state.dma_sim_rx(5), None);
            assert_eq!(state.dma_deadlines[5], None);
            assert_eq!(state.pending_lines(), 0);
        }
    }

    #[test]
    fn sim_reset_completion_waits_for_architectural_irq30_and_preserves_task() {
        use super::RegisterARM;
        let mut cla = vec![0xff; (super::IRQ_VECTOR_TARGET - u64::from(FLASH_BASE)) as usize + 0x1000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let task_pc = 0x3000_1200u32;
        let stack = 0x3000_2000u32;
        let cpu = machine.cpu.raw_engine();
        cpu.mem_write(u64::from(task_pc), &[0x00, 0x46, 0xfd, 0xe7]).unwrap();
        cpu.mem_write(u64::from(stack), &[0x5a; 32]).unwrap();
        let state = cpu.get_data_mut();
        state.i2c = crate::i2c::I2c::with_sim_present(true);
        state.i2c.write(0, 0x112, 0);
        state.i2c.write(0, 0x2c, 0);
        state.i2c.write(0, 0x21c, 0);
        state.i2c.advance(10_000);
        assert!(state.i2c.sim_powered());
        state.sim.advance(0, true);
        state.write_register(0x50406fe4, 2, 0x20);
        state.write_register(0x50406020, 1, 9);
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, u64::from(!(1u32 << 30)));
        cpu.reg_write(RegisterARM::CPSR, 0xbf).unwrap();
        cpu.reg_write(RegisterARM::PC, u64::from(task_pc | 1)).unwrap();
        cpu.reg_write(RegisterARM::R4, 0x1234_5678).unwrap();
        cpu.reg_write(RegisterARM::SP, u64::from(stack)).unwrap();
        cpu.reg_write(RegisterARM::LR, 0x3000_3001).unwrap();
        let report = machine.run(4104).unwrap();
        assert!(report.memory_fault.is_none());
        assert_eq!(report.irqs_taken, 0);
        assert_eq!(report.r4, 0x1234_5678);
        assert_eq!(report.sp, stack);
        assert_eq!(report.lr, 0x3000_3001);
        let cpu = machine.cpu.raw_engine();
        assert_eq!(cpu.get_data_mut().enabled_pending_lines(), 1 << 30);
        assert_eq!(cpu.mem_read_as_vec(u64::from(stack), 32).unwrap(), [0x5a; 32]);
        cpu.reg_write(RegisterARM::CPSR, 0x3f).unwrap();
        machine.set_breakpoint(super::IRQ_VECTOR_TARGET as u32, 0);
        let report = machine.run(4).unwrap();
        assert_eq!(report.stop_reason, super::StopReason::Breakpoint);
        assert_eq!(report.irqs_taken, 1);
        assert_eq!(report.r4, 0x1234_5678);
        let cpu = machine.cpu.raw_engine();
        assert_eq!(cpu.get_data().active_irq_line, 30);
        assert_eq!(cpu.reg_read(RegisterARM::SPSR).unwrap(), 0x3f);
        assert_eq!(cpu.mem_read_as_vec(u64::from(stack), 32).unwrap(), [0x5a; 32]);
    }

    #[test]
    fn dsp_completion_preserves_task_context_and_waits_for_architectural_irq() {
        use super::RegisterARM;
        let start = super::DSP_ASYNC_EVENT_HANDLER_PC;
        let offset = (start - u64::from(FLASH_BASE)) as usize;
        let mut cla = vec![0xff; (super::IRQ_VECTOR_TARGET - u64::from(FLASH_BASE)) as usize + 0x1000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        cla[offset..offset + 2].copy_from_slice(&[0x70, 0x47]); // synthetic start: bx lr
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let task_pc = 0x3000_1200u32;
        let entry = 0x3000_1000u32;
        let mut record = vec![0x5a; 64];
        record[0x10..0x14].copy_from_slice(&super::DSP_ASYNC_CALLBACK.to_le_bytes());
        let cpu = machine.cpu.raw_engine();
        cpu.mem_write(u64::from(task_pc), &[0x00, 0x46, 0xfd, 0xe7]).unwrap(); // mov r0,r0; b back
        cpu.mem_write(u64::from(entry), &record).unwrap();
        cpu.mem_write(u64::from(super::DSP_ASYNC_QUEUE + 4), &entry.to_le_bytes()).unwrap();
        cpu.get_data_mut().dsp_boot.hle_phase = super::DspHlePhase::E830RuntimeReady;
        cpu.get_data_mut().write_register(super::INTC_ENABLE_MASK_REGISTER, 4, 0);
        cpu.get_data_mut().write_register(super::DSP_SERIAL_BASE + 0x0c, 2, 1);
        cpu.reg_write(RegisterARM::CPSR, 0xbf).unwrap(); // task with CPSR.I set
        cpu.reg_write(RegisterARM::PC, start | 1).unwrap();
        cpu.reg_write(RegisterARM::R0, u64::from(super::DSP_ASYNC_QUEUE)).unwrap();
        cpu.reg_write(RegisterARM::R1, 1).unwrap();
        cpu.reg_write(RegisterARM::R4, 0x1234_5678).unwrap();
        cpu.reg_write(RegisterARM::SP, 0x3000_2000).unwrap();
        cpu.reg_write(RegisterARM::LR, u64::from(task_pc | 1)).unwrap();

        let report = machine.run(4100).unwrap();
        assert!(report.memory_fault.is_none());
        assert_eq!(report.irqs_taken, 0);
        assert_eq!(report.r4, 0x1234_5678);
        assert_eq!(report.sp, 0x3000_2000);
        assert_eq!(report.lr, task_pc | 1);
        assert!(matches!(report.pc, 0x3000_1200 | 0x3000_1202), "unexpected task PC: {report:?}");
        let cpu = machine.cpu.raw_engine();
        assert_eq!(cpu.mem_read_as_vec(u64::from(entry), 64).unwrap(), record);
        assert_eq!(cpu.get_data_mut().enabled_pending_lines(), 1 << super::DSP_SERIAL_IRQ);
        assert_eq!(cpu.get_data().dsp_queue_hle_phase, super::DspQueueHlePhase::CompleteSignaled);

        // Clearing CPSR.I permits normal IRQ entry; no callback was host-called.
        cpu.reg_write(RegisterARM::CPSR, 0x3f).unwrap();
        machine.set_breakpoint(super::IRQ_VECTOR_TARGET as u32, 0);
        let report = machine.run(4).unwrap();
        assert_eq!(report.stop_reason, super::StopReason::Breakpoint);
        assert_eq!(u64::from(report.pc), super::IRQ_VECTOR_TARGET);
        assert_eq!(report.irqs_taken, 1);
        assert_eq!(report.cpsr & 0x9f, 0x92);
        assert_eq!(report.r4, 0x1234_5678);
        assert_eq!(machine.cpu.raw_engine().mem_read_as_vec(u64::from(entry), 64).unwrap(), record);
    }

    #[test]
    fn rf_frame_phase_projects_fractional_ticks_and_wrap_without_side_effects() {
        let mut state = E830State::new(false, vec![0xff; 0x1000],
            Default::default(), Default::default(), Default::default(), Default::default());
        assert_eq!(state.rf_timer_phase_clocks(100), None);
        state.write_register(SOC_TIMER + 8, 2, 0x0660);
        state.write_register(SOC_TIMER + 0x0c, 2, 0xc000 | 5000);
        state.write_register(SOC_TIMER + 0x14, 2, 0x8000 | 4700);
        state.timer_counter = 2249;
        state.timer_last_update = 274_731_848;
        assert_eq!(state.rf_timer_phase_clocks(274_731_848), Some(2249 * 8));
        state.timer_counter = 19999;
        state.timer_clock_remainder = 7;
        state.timer_last_update = 200_000;
        state.registers.insert(SOC_TIMER + 8, 0x07e0); // latched status accepted
        assert_eq!(state.rf_timer_phase_clocks(200_000), Some(159999));
        assert_eq!(state.rf_timer_phase_clocks(200_001), Some(0));
        assert_eq!(state.rf_timer_phase_clocks(200_002), Some(1));
        assert_eq!(state.rf_timer_phase_clocks(520_003), Some(2)); // multiple wraps
        assert_eq!(state.timer_counter, 19999);
        assert_eq!(state.timer_clock_remainder, 7);
        assert_eq!(state.timer_last_update, 200_000);
        assert_eq!(state.pending_lines(), 0);
        assert_eq!(state.registers[&(SOC_TIMER + 8)], 0x07e0);
        assert_eq!(state.rf_timer_phase_clocks(199_999), None);
        for (offset, value) in [(8, 0x0460), (0x0c, 4999), (0x10, 1), (0x14, 4699)] {
            let previous = state.registers.get(&(SOC_TIMER + offset)).copied().unwrap_or(0);
            state.registers.insert(SOC_TIMER + offset, value);
            assert_eq!(state.rf_timer_phase_clocks(200_000), None);
            state.registers.insert(SOC_TIMER + offset, previous);
        }
        state.timer_counter = 20000;
        assert_eq!(state.rf_timer_phase_clocks(200_000), None);
        state.timer_counter = 0;
        state.timer_clock_remainder = 8;
        assert_eq!(state.rf_timer_phase_clocks(200_000), None);
    }

    #[test]
    fn rf_hle_publishes_only_hardware_payload_and_preserves_it_after_ack() {
        let mut cla = vec![0xff; 0x20000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(
            cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        cpu.mem_write(crate::dsp_rf::RESULT_BASE - 2, &[0x5a; 20]).unwrap();
        cpu.mem_write(0x3049_198c, &[0xa5; 100]).unwrap();
        cpu.reg_write(super::RegisterARM::CPSR, 0xbf).unwrap();
        cpu.reg_write(super::RegisterARM::R4, 0x12345678).unwrap();
        cpu.reg_write(super::RegisterARM::SP, 0x30002000).unwrap();
        let state = cpu.get_data_mut();
        state.dsp_rf = crate::dsp_rf::RfHle::new(true);
        state.dsp_boot.hle_phase = super::DspHlePhase::E830RuntimeReady;
        assert_eq!(state.dsp_rf.submit(crate::dsp_rf::tests::schedule(), 100, 0, 32), 5);
        state.instructions = 100 + (5000 + 536) * 32 - 1;
        super::advance_dsp_rf(cpu);
        assert_eq!(cpu.mem_read_as_vec(crate::dsp_rf::RESULT_BASE - 2, 20).unwrap(), [0x5a; 20]);
        cpu.get_data_mut().instructions += 1;
        super::advance_dsp_rf(cpu);
        let mut expected = [0x5a; 20];
        expected[16..18].copy_from_slice(&(-400i16).to_le_bytes());
        assert_eq!(cpu.mem_read_as_vec(crate::dsp_rf::RESULT_BASE - 2, 20).unwrap(), expected);
        assert_eq!(cpu.get_data_mut().read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0x80);
        assert_eq!(cpu.get_data().pending_lines(), 0); // no host-forced IRQ
        cpu.get_data_mut().write_register(super::DSP_RESULT_SIGNAL, 2, 0xff7f);
        assert_eq!(cpu.get_data_mut().read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0);
        assert_eq!(cpu.mem_read_as_vec(crate::dsp_rf::RESULT_BASE - 2, 20).unwrap(), expected);
        super::advance_dsp_rf(cpu);
        assert_eq!(cpu.get_data_mut().read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0);
        assert_eq!(cpu.mem_read_as_vec(0x3049_198c, 100).unwrap(), [0xa5; 100]);
        assert_eq!(cpu.reg_read(super::RegisterARM::CPSR).unwrap(), 0xbf);
        assert_eq!(cpu.reg_read(super::RegisterARM::R4).unwrap(), 0x12345678);
        assert_eq!(cpu.reg_read(super::RegisterARM::SP).unwrap(), 0x30002000);
        // Late producer submission still executes in the next timer frame.
        let state = cpu.get_data_mut();
        state.dsp_rf.reset();
        state.instructions = 200000;
        assert_eq!(state.dsp_rf.submit(crate::dsp_rf::tests::profile0333_schedule(), 200000, 159999, 32), 5);
        assert_eq!(state.pending_lines(), 0);
        assert_eq!(state.read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0);
        super::advance_dsp_rf(cpu);
        assert_eq!(cpu.get_data_mut().read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0);
        cpu.get_data_mut().instructions = 200000 + 4176 * 32 + 1;
        super::advance_dsp_rf(cpu);
        assert_eq!(cpu.get_data_mut().read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0xf8);
        assert_eq!(cpu.get_data().pending_lines(), 0);
        assert_eq!(cpu.mem_read_as_vec(0x3049_198c, 100).unwrap(), [0xa5; 100]);
        assert_eq!(cpu.mem_read_as_vec(crate::dsp_rf::RESULT_BASE + 6, 2).unwrap(),
            (-400i16).to_le_bytes()); // profile0333 uses device payload, not ARM seeding
        let completed = cpu.mem_read_as_vec(crate::dsp_rf::RESULT_BASE, 16).unwrap();
        cpu.get_data_mut().write_register(super::DSP_RESULT_SIGNAL, 2, 0xff07);
        super::advance_dsp_rf(cpu);
        assert_eq!(cpu.get_data_mut().read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0);
        assert_eq!(cpu.mem_read_as_vec(crate::dsp_rf::RESULT_BASE, 16).unwrap(), completed);
        assert_eq!(cpu.mem_read_as_vec(0x3049_198c, 100).unwrap(), [0xa5; 100]);
        assert_eq!(cpu.reg_read(super::RegisterARM::CPSR).unwrap(), 0xbf);
        assert_eq!(cpu.reg_read(super::RegisterARM::R4).unwrap(), 0x12345678);
        assert_eq!(cpu.reg_read(super::RegisterARM::SP).unwrap(), 0x30002000);
    }

    #[test]
    fn rf_hle_cannot_publish_without_image_specific_runtime() {
        let mut cla = vec![0xff; 0x20000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(
            cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        cpu.mem_write(crate::dsp_rf::RESULT_BASE, &[0x5a; 16]).unwrap();
        let state = cpu.get_data_mut();
        state.dsp_rf = crate::dsp_rf::RfHle::new(true);
        state.dsp_rf.submit(crate::dsp_rf::tests::schedule(), 0, 0, 32);
        state.instructions = 200000;
        super::advance_dsp_rf(cpu);
        assert!(!cpu.get_data().dsp_rf.busy());
        assert_eq!(cpu.mem_read_as_vec(crate::dsp_rf::RESULT_BASE, 16).unwrap(), [0x5a; 16]);
        assert_eq!(cpu.get_data_mut().read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0);
    }

    #[test]
    fn rf_hle_consumes_real_mmio_control_and_guest_mask_clear() {
        let mut cla = vec![0xff; 0x20000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        // Three STRH r0,[r1] then STRH r2,[r3]. All hooks
        // are exercised by actual guest stores, not direct state shortcuts.
        cpu.mem_write(0x30001000, &[0x08, 0x80, 0x08, 0x80, 0x08, 0x80, 0x1a, 0x80]).unwrap();
        cpu.mem_write(crate::dsp_rf::SNAPSHOT_BASE, &crate::dsp_rf::tests::schedule()).unwrap();
        cpu.get_data_mut().dsp_rf = crate::dsp_rf::RfHle::new(true);
        cpu.reg_write(super::RegisterARM::CPSR, 0xbf).unwrap();
        cpu.reg_write(super::RegisterARM::PC, 0x30001001).unwrap();
        cpu.reg_write(super::RegisterARM::R0, 0x40).unwrap();
        cpu.reg_write(super::RegisterARM::R1, u64::from(super::DSP_SIGNAL_BASE)).unwrap();
        cpu.reg_write(super::RegisterARM::R2, 0).unwrap();
        cpu.reg_write(super::RegisterARM::R3, crate::dsp_rf::SNAPSHOT_BASE).unwrap();
        machine.run(1).unwrap();
        assert!(!machine.cpu.raw_engine().get_data().dsp_rf.busy()); // wrong image phase
        machine.cpu.raw_engine().get_data_mut().dsp_boot.hle_phase = super::DspHlePhase::E830RuntimeReady;
        machine.run(1).unwrap();
        assert!(!machine.cpu.raw_engine().get_data().dsp_rf.busy()); // unsupported timer mode
        let state = machine.cpu.raw_engine().get_data_mut();
        state.write_register(SOC_TIMER + 8, 2, 0x0660);
        state.write_register(SOC_TIMER + 0x0c, 2, 5000);
        state.write_register(SOC_TIMER + 0x14, 2, 4700);
        machine.run(1).unwrap();
        assert!(machine.cpu.raw_engine().get_data().dsp_rf.busy());
        machine.run(1).unwrap();
        assert!(machine.cpu.raw_engine().get_data().dsp_rf.busy()); // accepted work survives table clear
        assert_eq!(machine.cpu.raw_engine().get_data_mut().read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0);
        let cpu = machine.cpu.raw_engine();
        cpu.get_data_mut().instructions = 300000;
        super::advance_dsp_rf(cpu);
        assert!(!cpu.get_data().dsp_rf.busy());
        assert_eq!(cpu.get_data_mut().read_register(super::DSP_RESULT_SIGNAL, 2, 0), 0xf8);
    }

    #[test]
    fn controller_service_remains_visible_after_peripheral_ack_until_eoi() {
        let mut state = E830State::new(
            false,
            vec![0xff; 0x1000],
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        );
        state.write_register(super::INTC_ENABLE_MASK_REGISTER, 4, 0);
        state.system_timer_irq_pending = true;
        state.active_irq_line = SYSTEM_TIMER_IRQ;
        state.timer_irq_in_service = true;
        let service = 1u32 << SYSTEM_TIMER_IRQ;
        assert_eq!(state.read_register(super::INTC_PENDING_MASK_REGISTER, 4, 0),
            u64::from(service));

        state.write_register(SOC_TIMER + 8, 2, 0);
        assert_eq!(state.pending_lines(), 0);
        assert_eq!(state.enabled_pending_lines(), 0);
        assert_eq!(state.read_register(super::INTC_MASKED_PENDING_REGISTER, 4, 0), 0);
        assert_eq!(state.read_register(super::INTC_PENDING_MASK_REGISTER, 4, 0),
            u64::from(service));

        // A new device level is a request, not a second accepted interrupt.
        state.system_timer_match_irq_pending = true;
        let next = 1u32 << SYSTEM_TIMER_MATCH_IRQ;
        assert_eq!(state.read_register(super::INTC_PENDING_MASK_REGISTER, 4, 0),
            u64::from(service));
        assert_eq!(state.read_register(super::INTC_MASKED_PENDING_REGISTER, 4, 0),
            u64::from(next));
        state.write_register(SOC_INIT_CONTROL_REGISTER, 1, 0x80);
        assert!(state.timer_irq_in_service);
        assert_eq!(state.read_register(super::INTC_PENDING_MASK_REGISTER, 4, 0), 0);
        state.timer_irq_in_service = false;
        assert_eq!(state.read_register(super::INTC_PENDING_MASK_REGISTER, 4, 0), 0);
        assert_eq!(state.enabled_pending_lines(), next);
        state.system_timer_match_irq_pending = false;
        assert_eq!(state.read_register(super::INTC_PENDING_MASK_REGISTER, 4, 0), 0);
    }

    #[test]
    fn finds_the_thumb_counting_delay_loop() {
        let mut image = vec![0; 16];
        image[3..13].copy_from_slice(&COUNTING_DELAY_LOOP);
        assert_eq!(
            find_counting_delay_loop_pcs(&image),
            [u64::from(FLASH_BASE) + 5].into()
        );
    }

    #[test]
    fn combined_loop_lookup_preserves_all_memberships_and_overlaps() {
        let sets = [
            [0x2000, 0x2100, 0x2500].into(),
            [0x2100, 0x2200, 0x2500].into(),
            [0x2300, 0x2500].into(),
            [0x2200, 0x2400, 0x2500].into(),
        ];
        let kinds = super::collect_loop_kinds(sets.clone());
        for pc in (0x1f00..=0x2600).chain([0, u64::MAX]) {
            let combined = super::loop_kinds_at(&kinds, pc);
            for (index, original) in sets.iter().enumerate() {
                assert_eq!(combined & (1 << index) != 0, original.contains(&pc));
            }
        }
        assert_eq!(kinds.len(), 6);
        assert_eq!(super::loop_kinds_at(&kinds, 0x2500), 0x0f);
        assert!(super::collect_loop_kinds(Default::default()).is_empty());
    }

    #[test]
    fn combined_loop_lookup_preserves_detected_nested_fade_overlap() {
        let mut image = vec![0; 64];
        image[8..24].copy_from_slice(&NESTED_DELAY_LOOP);
        let kinds = super::collect_loop_kinds([
            find_counting_delay_loop_pcs(&image),
            find_nested_delay_loop_pcs(&image),
            super::find_memset_loop_pcs(&image),
            find_fade_delay_loop_pcs(&image),
        ]);
        assert_eq!(super::loop_kinds_at(&kinds, u64::from(FLASH_BASE) + 10),
            super::LOOP_NESTED | super::LOOP_FADE);
    }

    #[test]
    fn finds_the_thumb_fade_delay_loop() {
        let mut image = vec![0; 16];
        image[4..10].copy_from_slice(&FADE_DELAY_LOOP);
        assert_eq!(
            find_fade_delay_loop_pcs(&image),
            [u64::from(FLASH_BASE) + 4].into()
        );
    }

    #[test]
    fn fade_delay_fast_forward_preserves_thumb_state() {
        use super::RegisterARM;
        let mut cla = vec![0xff; 0x20_000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        cla[0x100..0x106].copy_from_slice(&FADE_DELAY_LOOP);
        // The instruction after the skipped delay must execute as Thumb.
        cla[0x106..0x10a].copy_from_slice(&[0x01, 0x21, 0xfe, 0xe7]);
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        let entry = u64::from(FLASH_BASE) + 0x100;
        cpu.reg_write(RegisterARM::CPSR, 0xbf).unwrap();
        cpu.reg_write(RegisterARM::PC, entry | 1).unwrap();
        cpu.reg_write(RegisterARM::R0, 4).unwrap();
        cpu.reg_write(RegisterARM::R1, 0).unwrap();

        let report = machine.run(14).unwrap();
        assert_eq!(report.stop_reason, super::StopReason::InstructionBudget);
        assert_eq!(report.r1, 1);
        assert_ne!(report.cpsr & super::CPSR_THUMB, 0);
        assert!(matches!(u64::from(report.pc), pc if pc == entry + 8 || pc == entry + 6));
    }

    #[test]
    fn fast_forwards_only_the_remaining_pure_loop_iterations() {
        assert_eq!(
            fast_forward_counting_delay(10, 30, 20, 100),
            Some((29, 39, 76))
        );
        assert_eq!(
            fast_forward_counting_delay(10, 30, 20, 75),
            Some((28, 38, 72))
        );
        assert_eq!(fast_forward_counting_delay(10, 30, 20, 3), None);
        assert_eq!(fast_forward_counting_delay(29, 30, 39, 100), None);
        assert_eq!(fast_forward_counting_delay(30, 30, 39, 100), None);
    }

    #[test]
    fn finds_nested_delay_loop_inner_entry() {
        let mut image = vec![0; 32];
        image[8..24].copy_from_slice(&NESTED_DELAY_LOOP);
        assert_eq!(
            find_nested_delay_loop_pcs(&image),
            [u64::from(FLASH_BASE) + 10].into()
        );
    }

    #[test]
    fn fast_forwards_only_complete_nested_inner_loops() {
        assert_eq!(fast_forward_nested_delay(100, 1, 300), Some(299));
        assert_eq!(fast_forward_nested_delay(100, 1, 299), None);
        assert_eq!(fast_forward_nested_delay(100, 0, 1_000), None);
        assert_eq!(fast_forward_nested_delay(0, 1, 1_000), None);
    }

    #[test]
    fn pending_rf_work_prevents_all_loop_collapses() {
        use super::RegisterARM;
        for (kind, pattern, entry_offset, cpsr) in [
            ("counting", COUNTING_DELAY_LOOP.as_slice(), 0, 0xbf),
            ("nested", NESTED_DELAY_LOOP.as_slice(), 2, 0xbf),
            ("fade", FADE_DELAY_LOOP.as_slice(), 0, 0xbf),
            ("zero-fill", super::ZERO_FILL_LOOP.as_slice(), 0, 0x2000001f),
        ] {
            let mut cla = vec![0xff; 0x20_000];
            cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
            cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
            cla[0x100..0x100 + pattern.len()].copy_from_slice(pattern);
            let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
            let mut machine = E830Machine::new(firmware, false).unwrap();
            let cpu = machine.cpu.raw_engine();
            let entry = u64::from(FLASH_BASE) + 0x100 + entry_offset;
            cpu.reg_write(RegisterARM::CPSR, cpsr).unwrap();
            cpu.reg_write(RegisterARM::PC, entry | ((cpsr >> 5) & 1)).unwrap();
            let fill = kind == "zero-fill";
            cpu.reg_write(RegisterARM::R0, if fill { 0x30773614 } else { 100 }).unwrap();
            cpu.reg_write(RegisterARM::R1, if fill { 0x1887e0 } else { 200 }).unwrap();
            cpu.reg_write(RegisterARM::R2, if fill { 0 } else { 100 }).unwrap();
            cpu.reg_write(RegisterARM::R3, 0).unwrap();
            cpu.reg_write(RegisterARM::R12, 0).unwrap();
            cpu.reg_write(RegisterARM::LR, 0).unwrap();
            cpu.mem_write(0x30773614, &[0x5a; 64]).unwrap();
            let state = cpu.get_data_mut();
            state.dsp_boot.hle_phase = super::DspHlePhase::E830RuntimeReady;
            state.dsp_rf = crate::dsp_rf::RfHle::new(true);
            assert_eq!(state.dsp_rf.submit(crate::dsp_rf::tests::schedule(), 0, 0, 32), 5);
            // Give the optimizer enough room, but break after one real loop
            // instruction. Without the guard each eligible loop skips work.
            state.breakpoint = Some((entry + if fill { 4 } else { 2 }, 0));
            let report = machine.run(300000).unwrap();
            assert_eq!(report.stop_reason, super::StopReason::Breakpoint, "{kind}");
            assert_eq!(report.instructions, 1, "{kind}");
            assert_eq!(report.fast_forwarded_instructions, 0, "{kind}");
            let cpu = machine.cpu.raw_engine();
            assert!(cpu.get_data().dsp_rf.busy(), "{kind}");
            assert_eq!(cpu.get_data().registers.get(&super::DSP_RESULT_SIGNAL).copied().unwrap_or(0), 0);
            assert_eq!(cpu.reg_read(RegisterARM::CPSR).unwrap(), cpsr, "{kind}");
            if fill {
                assert_eq!(report.r0, 0x30773624);
                assert_eq!(cpu.mem_read_as_vec(0x30773614, 16).unwrap(), [0; 16]);
                assert_eq!(cpu.mem_read_as_vec(0x30773624, 48).unwrap(), [0x5a; 48]);
            }
        }
    }

    #[test]
    fn run_resumes_from_the_current_program_counter() {
        let mut cla = vec![0xff; 0x20_000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        cla[0x100..0x10c].copy_from_slice(&[
            0x00, 0x00, 0xa0, 0xe3, 0x01, 0x00, 0x80, 0xe2, 0xfd, 0xff, 0xff, 0xea,
        ]);
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();

        let first = machine.run(20).unwrap();
        let second = machine.run(20).unwrap();

        assert!(
            second.r0 > first.r0,
            "R0 did not advance across slices: {} -> {}, PC 0x{:08x} -> 0x{:08x}, instructions {} -> {}",
            first.r0,
            second.r0,
            first.pc,
            second.pc,
            first.instructions,
            second.instructions
        );
        assert!(second.instructions > first.instructions);
    }

    #[test]
    fn cla_working_image_writes_do_not_change_the_read_only_mirror() {
        let mut cla = vec![0xff; 0x20_000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let words = [
            0xe59f_0008u32, // ldr r0, [pc, #8]
            0xe3a0_1005,    // mov r1, #5
            0xe580_1000,    // str r1, [r0]
            0xeaff_fffe,    // b .
            FLASH_BASE + 0x180,
        ];
        for (index, word) in words.into_iter().enumerate() {
            cla[0x100 + index * 4..0x104 + index * 4].copy_from_slice(&word.to_le_bytes());
        }
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();

        let report = machine.run(4).unwrap();
        assert!(report.memory_fault.is_none());
        assert_eq!(machine.peek_u32(FLASH_BASE + 0x180), Some(5));
        let mirror = machine
            .cpu
            .raw_engine()
            .mem_read_as_vec(super::E830_BOOT_ROM_MIRROR_BASE + 0x180, 4)
            .unwrap();
        assert_eq!(mirror, vec![0xff; 4]);
    }

    #[test]
    fn run_resumes_in_thumb_mode_after_budget_stop() {
        let mut cla = vec![0xff; 0x20_000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        cla[0x100..0x110].copy_from_slice(&[
            0x04, 0x00, 0x9f, 0xe5, 0x10, 0xff, 0x2f, 0xe1, 0x00, 0x00, 0xa0, 0xe1, 0x01, 0x02,
            0x00, 0x20,
        ]);
        cla[0x200..0x206].copy_from_slice(&[0x00, 0x20, 0x01, 0x30, 0xfe, 0xe7]);
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();

        let first = machine.run(3).unwrap();
        assert_eq!(first.pc, FLASH_BASE + 0x200);
        assert_ne!(first.cpsr & (1 << 5), 0);

        let second = machine.run(2).unwrap();
        assert_eq!(second.r0, 1);
        assert_eq!(second.pc, FLASH_BASE + 0x204);
        assert_ne!(second.cpsr & (1 << 5), 0);
    }

    #[test]
    fn firmware_reads_tcm_capacity_and_writes_data_tcm() {
        let mut cla = vec![0xff; 0x20_000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let words: [u32; 10] = [
            0xee10_0f50, // MRC p15,0,r0,c0,c0,2: board TCMTR
            0xe3a0_102a, // MOV r1,#42
            0xee09_1f11, // MCR p15,0,r1,c9,c1,0: data TCM region
            0xee19_2f11, // MRC p15,0,r2,c9,c1,0
            0xe59f_3008, // LDR r3,[pc,#8]: data TCM address
            0xe583_2000, // STR r2,[r3]
            0xe593_4000, // LDR r4,[r3]
            0xeaff_fffe, // B .
            super::E830_DTCM_BASE as u32,
            0,
        ];
        for (index, word) in words.iter().enumerate() {
            cla[0x100 + index * 4..0x104 + index * 4].copy_from_slice(&word.to_le_bytes());
        }
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let report = machine.run(12).unwrap();
        assert_eq!(report.r0, (5 << 18) | (5 << 6));
        assert_eq!(report.r2, 42);
        assert_eq!(report.r4, 42);
        assert!(report.memory_fault.is_none());
    }

    #[test]
    fn breakpoint_stops_before_instruction_and_honors_threshold() {
        let mut cla = vec![0xff; 0x20_000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        cla[0x100..0x10c].copy_from_slice(&[
            0x00, 0x00, 0xa0, 0xe3, 0x01, 0x00, 0x80, 0xe2, 0xfd, 0xff, 0xff, 0xea,
        ]);
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        machine.set_breakpoint(FLASH_BASE + 0x104, 5);
        let report = machine.run(20).unwrap();
        assert_eq!(report.stop_reason, super::StopReason::Breakpoint);
        assert_eq!(report.pc, FLASH_BASE + 0x104);
        assert_eq!(report.instructions, 6);
        assert_eq!(report.r0, 2);
    }

    #[test]
    fn firmware_call_restores_arm_caller_and_stack_after_thumb_function_writes_data() {
        let mut cla = vec![0xff; 0x20_000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        cla[0x100..0x10c].copy_from_slice(&[
            0x12, 0x00, 0xa0, 0xe3, 0x01, 0x00, 0x80, 0xe2, 0xfd, 0xff, 0xff, 0xea,
        ]);
        // Thumb: r0 = fifth argument + r1; store r0 through r2; return.
        cla[0x200..0x208].copy_from_slice(&[0x00, 0x98, 0x40, 0x18, 0x10, 0x60, 0x70, 0x47]);
        let firmware = crate::FirmwareSet::from_bytes(cla, vec![1], None, None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        machine.set_breakpoint(FLASH_BASE + 0x104, 0);
        let caller = machine.run(20).unwrap();
        machine.write_installer_buffer(caller.sp, &0xfeed1234u32.to_le_bytes()).unwrap();
        let called = machine.call_firmware(FLASH_BASE + 0x201,
            &[3, 7, crate::RAM_BASE + 0x180, 13, 17], 20).unwrap();
        assert_eq!(called.stop_reason, super::StopReason::Breakpoint, "{called:#?}");
        assert!(called.memory_fault.is_none());
        assert_eq!(called.r0, 24);
        assert_eq!(machine.peek_u32(crate::RAM_BASE + 0x180), Some(24));
        assert_eq!(machine.peek_u32(caller.sp), Some(0xfeed1234));
        machine.set_breakpoint(FLASH_BASE + 0x108, 0);
        let resumed = machine.run(20).unwrap();
        assert_eq!(resumed.r0, 0x13);
        assert_eq!(resumed.cpsr, caller.cpsr);
        assert_eq!(resumed.sp, caller.sp);
        assert_eq!(resumed.lr, caller.lr);
    }

    #[test]
    fn tfs_package_is_a_source_image_and_flash_starts_erased() {
        let mut cla = vec![0; 0x20_000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let source = vec![0x12, 0x34, 0x56, 0x78];
        let firmware =
            crate::FirmwareSet::from_bytes(cla, vec![1], Some(source.clone()), None).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        let cpu = machine.cpu.raw_engine();
        let mut source_readback = vec![0; source.len()];
        cpu.mem_read(E830_TFS_SOURCE_BASE, &mut source_readback)
            .unwrap();
        assert_eq!(source_readback, source);
        assert_eq!(cpu.get_data_mut().nor.read(0, 2), 0xffff);
    }

    #[test]
    fn nsf_is_mapped_to_nor_and_tfs_package_does_not_overwrite_it() {
        let mut cla = vec![0; 0x20_000];
        cla[..4].copy_from_slice(&[0x54, 0xf1, 0x9f, 0xe5]);
        cla[0x15c..0x160].copy_from_slice(&(FLASH_BASE + 0x100).to_le_bytes());
        let payload = vec![0x12, 0x34, 0x56, 0x78];
        let package = vec![0xff, 0xd8, 0xff, 0xe0];
        let mut nsf = vec![0; 20];
        nsf[..3].copy_from_slice(b"NF4");
        nsf.extend_from_slice(&payload);
        let firmware = crate::FirmwareSet::from_bytes(cla, nsf, None, Some(package.clone())).unwrap();
        let mut machine = E830Machine::new(firmware, false).unwrap();
        assert_eq!(machine.peek_u32(0x21c20000).unwrap(), 0x78563412);
        assert_eq!(machine.peek_u32(0x21f40000).unwrap(), u32::MAX);
        assert_eq!(
            machine.peek_u32(super::DSP_AUX_RAM_BASE as u32).unwrap(),
            DspBootInterface::TX_READY
        );
        let cpu = machine.cpu.raw_engine();
        let mut readback = vec![0; package.len()];
        cpu.mem_read(E830_TFS_SOURCE_BASE, &mut readback).unwrap();
        assert_eq!(readback, package);
    }
}
