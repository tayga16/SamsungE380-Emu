use std::collections::{HashMap, HashSet, VecDeque};

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
// 0x60000008 and 0x600007e0). Storage only; DSP execution is not implemented.
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
const DMA_BYTE_INSTRUCTIONS: u64 = 32;
const I2C_BASE: u32 = 0x5040_5000;
const I2C_IRQ: u32 = 28;
const SERIAL_BASE: u32 = 0x5040_8000;
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
const RTOS_TASK_TRACE_PCS: [u64; 8] = [
    0x2084_7660, // receive: task group in r7, request in r5
    0x2084_7686, // event-mask wait has been stored in current task
    0x2084_76c4, // empty message queue wait
    0x2084_7724, // receive result: message in r7
    0x2084_7d14, // semaphore decrement: object in r4
    0x204b_01d8, // context switch: next task in r1
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

fn advance_dma(uc: &mut Unicorn<'_, E830State>) {
    let now = uc.get_data().instructions.saturating_add(uc.get_data().fast_forwarded_instructions);
    for channel in 0..8 {
        if !uc.get_data().dma_deadlines[channel].is_some_and(|deadline| now >= deadline) {
            continue;
        }
        uc.get_data_mut().dma_deadlines[channel] = None;
        let Some((source, count)) = uc.get_data().dma_uart0_tx(channel) else { continue; };
        // DMA reads ordinary RAM only; never consume an RX/MMIO register to
        // manufacture data. Invalid/unsupported descriptors get no completion.
        let Some(bytes) = diagnostic_ram_read(uc, u64::from(source), count) else { continue; };
        let state = uc.get_data_mut();
        for byte in bytes { state.uarts[0].write(0, u32::from(byte)); }
        let base = DMA_BASE + 0x100 + channel as u32 * 0x20;
        state.registers.insert(base, source + count as u32);
        *state.registers.entry(base + 0x0c).or_default() &= !0xfff;
        *state.registers.entry(base + 0x10).or_default() &= !1;
        *state.registers.entry(DMA_BASE + 0x14).or_default() |= 1 << channel;
        // IRQ29 owns all CPU/queue/callback changes, including SL3 ready flags.
    }
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
    registers: HashMap<u32, u32>,
    dsp_boot: DspBootInterface,
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
    timer_counter: u16,
    timer_last_update: u64,
    timer_clock_remainder: u64,
    system_timer_irq_pending: bool,
    system_timer_match_irq_pending: bool,
    instructions: u64,
    fast_forwarded_instructions: u64,
    instruction_budget: u64,
    delay_loop_pcs: HashSet<u64>,
    nested_delay_loop_pcs: HashSet<u64>,
    memset_loop_pcs: HashSet<u64>,
    fade_delay_loop_pcs: HashSet<u64>,
    trace_mmio: bool,
    recent_mmio: VecDeque<MmioAccess>,
    memory_fault: Option<MemoryFault>,
    breakpoint: Option<(u64, u64)>,
    breakpoint_hit: bool,
    tcm_registers: TcmRegisters,
    uarts: [crate::uart::Uart; 2],
    dma_deadlines: [Option<u64>; 8],
    i2c: crate::i2c::I2c,
    serial: crate::serial::SerialController,
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
    // Optional read-only register trace at selected instruction addresses.
    watch_pcs: HashSet<u64>,
    watch_after: u64,
    watch_remaining: u64,
    // Bounded, read-only task/event snapshots; never changes guest state.
    trace_tasks: bool,
    trace_tasks_after: u64,
    trace_tasks_remaining: u64,
    trace_tasks_pcs: HashSet<u64>,
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
            dsp_boot: DspBootInterface::new(std::env::var_os("E830_DSP_BOOT_HLE").is_some()),
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
            timer_counter: 0,
            timer_last_update: 0,
            timer_clock_remainder: 0,
            system_timer_irq_pending: false,
            system_timer_match_irq_pending: false,
            instructions: 0,
            fast_forwarded_instructions: 0,
            instruction_budget: u64::MAX,
            delay_loop_pcs,
            nested_delay_loop_pcs,
            memset_loop_pcs,
            fade_delay_loop_pcs,
            trace_mmio,
            recent_mmio: VecDeque::with_capacity(MAX_RECENT_MMIO),
            memory_fault: None,
            breakpoint: None,
            breakpoint_hit: false,
            tcm_registers: TcmRegisters::default(),
            uarts: Default::default(),
            dma_deadlines: [None; 8],
            i2c: Default::default(),
            serial: Default::default(),
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
            trace_irq_service: std::env::var("E830_TRACE_IRQ_SERVICE").is_ok(),
            trace_irq_service_after: env_u64("E830_TRACE_IRQ_SERVICE_AFTER").unwrap_or(0),
            trace_irq_service_remaining: env_u64("E830_TRACE_IRQ_SERVICE_LIMIT").unwrap_or(1000),
            trace_irq_service_last: None,
            pc_log_after: env_u64("E830_PC_LOG_AFTER").unwrap_or(0),
            pc_log_remaining: env_u64("E830_PC_LOG").unwrap_or(0),
            watch_pcs: std::env::var("E830_WATCH_PC").unwrap_or_default()
                .split(',').filter_map(|raw| {
                    let raw = raw.trim();
                    raw.strip_prefix("0x").map(|hex| u64::from_str_radix(hex, 16))
                        .unwrap_or_else(|| raw.parse()).ok().map(|pc| pc & !1)
                }).collect(),
            watch_after: env_u64("E830_WATCH_AFTER").unwrap_or(0),
            watch_remaining: env_u64("E830_WATCH_LIMIT").unwrap_or(1000),
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
        if access.write
            && ((SOC_TIMER..=SOC_TIMER + 0x14).contains(&access.address)
                || access.address == INTC_ENABLE_MASK_REGISTER)
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
            self.trace_timer_line(format!(
                "mmio {action} addr=0x{:08x} size={} value=0x{:08x} pc=0x{:08x} irq20_masked={irq20_masked} irq21_masked={irq21_masked} insns={total}",
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
        diagnostic | system_timer | system_timer_match | i2c | dsp_serial | dma
    }

    fn dma_interrupt_status(&self) -> u32 {
        let raw = self.registers.get(&(DMA_BASE + 0x14)).copied().unwrap_or(0);
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

    fn advance_dsp_serial(&mut self, now: u64) {
        if let DspQueueHlePhase::CompletePending { deadline } = self.dsp_queue_hle_phase {
            if now >= deadline {
                // The exact-image HLE supplies only the observed transfer-done
                // status. IRQ9 and all callbacks/queue changes belong to the
                // original serial ISR (210d4fc0 -> 21122084 -> 215a7cb8).
                *self.registers.entry(DSP_SERIAL_BASE + 0x14).or_default()
                    |= DSP_SERIAL_TX_COMPLETE;
                self.dsp_queue_hle_phase = DspQueueHlePhase::CompleteSignaled;
            }
        }
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

    fn enabled_pending_lines(&self) -> u32 {
        let mask = self
            .registers
            .get(&INTC_ENABLE_MASK_REGISTER)
            .copied()
            .unwrap_or(u32::MAX);
        self.pending_lines() & !mask
    }

    fn read_register(&mut self, address: u32, size: usize, pc: u32) -> u64 {
        if u64::from(address) >= DSP_CONTROL_BASE
            && u64::from(address) < DSP_CONTROL_BASE + DSP_BOOT_MMIO_SIZE
        {
            return self.dsp_boot.read(address, size);
        }
        if u64::from(address) >= ONENAND_BASE && u64::from(address) < ONENAND_BASE + ONENAND_SIZE {
            return self.onenand.read(address - ONENAND_BASE as u32, size);
        }
        let aligned = address & !3;
        let raw = if let Some((channel, base)) = UART_BASES.iter().enumerate()
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
        } else if address == SOC_TIMER {
            u32::from(self.timer_counter)
        } else if aligned == INTC_PENDING_NUMBER_REGISTER {
            // The IRQ vector handler reads this byte and uses it directly as the
            // pre-multiplied dispatch index, stable while a handler runs.
            if self.timer_irq_in_service {
                self.active_irq_line * 4
            } else if self.enabled_pending_lines() != 0 {
                (31 - self.enabled_pending_lines().leading_zeros()) * 4
            } else {
                self.registers.get(&aligned).copied().unwrap_or(0)
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
        let shift = (address & 3) * 8;
        let value_mask = width_mask(size);
        let mask = value_mask.wrapping_shl(shift);
        if matches!(aligned, DSP_START_SIGNAL | DSP_RUNTIME_SIGNAL) {
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
                if let Some((_, count)) = self.dma_uart0_tx(channel) {
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
                (
                    state.delay_loop_pcs.contains(&address),
                    state.nested_delay_loop_pcs.contains(&address),
                    state.memset_loop_pcs.contains(&address),
                    state.fade_delay_loop_pcs.contains(&address),
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
            eprintln!(
                "[e830-task] pc=0x{address:08x} insns={total} current=0x{current:08x} task=[{}] object=[{}] node=[{}] arg=[{}] {}",
                task_words.join(","), object_words.join(","), node_words.join(","),
                argument_words.join(","), values.join(" ")
            );
            uc.get_data_mut().trace_tasks_remaining -= 1;
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
        if total >= uc.get_data().watch_after
            && uc.get_data().watch_remaining > 0
            && uc.get_data().watch_pcs.contains(&address)
        {
            let registers = [RegisterARM::R0, RegisterARM::R1, RegisterARM::R2, RegisterARM::R3,
                RegisterARM::R4, RegisterARM::R5, RegisterARM::R6, RegisterARM::R7,
                RegisterARM::SP, RegisterARM::LR, RegisterARM::CPSR];
            let values: Vec<String> = registers.into_iter().map(|register|
                format!("{:?}=0x{:08x}", register, uc.reg_read(register).unwrap_or(0))).collect();
            eprintln!("[e830-watch] pc=0x{address:08x} insns={total} {}", values.join(" "));
            uc.get_data_mut().watch_remaining -= 1;
        }
        if DSP_TRACE_PCS.contains(&address) {
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

        let wake_pc = (WAKE_WAIT_TRACE_START..=WAKE_WAIT_TRACE_END).contains(&address)
            || WAKE_IRQ_TRACE_PCS.contains(&address);
        if wake_pc && uc.get_data().trace_wake {
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

        if TIMER_TRACE_PCS.contains(&address) && uc.get_data().trace_timer {
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

        // Advance device time even while the CPU masks interrupts.
        advance_dma(uc);
        let service_cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
        let service_nesting = uc
            .mem_read_as_vec(RTOS_NESTING_DEPTH, 1)
            .ok()
            .and_then(|bytes| bytes.first().copied())
            .unwrap_or(u8::MAX);
        let service_returned = interrupt_service_returned(
            service_cpsr,
            uc.get_data().timer_irq_in_service,
            uc.get_data().irq_eoi_seen,
        );
        {
            let state = uc.get_data();
            let total = state.instructions.saturating_add(state.fast_forwarded_instructions);
            let key = (
                state.timer_irq_in_service,
                state.irq_eoi_seen,
                service_nesting,
                state.irq_entry_nesting,
                service_cpsr & (CPSR_MODE_MASK | CPSR_IRQ_DISABLE | CPSR_THUMB),
                state.pending_lines(),
                state.enabled_pending_lines(),
            );
            if state.trace_irq_service
                && total >= state.trace_irq_service_after
                && state.trace_irq_service_remaining > 0
                && (state.timer_irq_in_service || state.irq_eoi_seen || key.6 != 0)
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
        let irq_is_pending = {
            let state = uc.get_data_mut();
            let total = state.instructions.saturating_add(state.fast_forwarded_instructions);
            state.advance_dsp_serial(total);
            state.i2c.advance(total);
            state.advance_system_timer(total);
            if state.timer_irq_period == 0 {
                state.enabled_pending_lines() != 0
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
                    }
                }
                state.enabled_pending_lines() != 0
            }
        };
        if irq_is_pending {
            let cpsr = uc.reg_read(RegisterARM::CPSR).unwrap_or(0) as u32;
            let in_service = uc.get_data().timer_irq_in_service;
            let pending = uc.get_data().enabled_pending_lines();
            let line_enabled = pending != 0;
            let line = 31 - pending.leading_zeros();
            if line_enabled && interrupt_delivery_allowed(cpsr, in_service) {
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
            if uc.reg_write(RegisterARM::R0, 0).is_err()
                || uc.reg_write(RegisterARM::PC, address + 6).is_err()
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

    if let Some(start) = env_u64("E830_TRACE_RAM_START") {
        let end = env_u64("E830_TRACE_RAM_END").unwrap_or(start);
        let after = env_u64("E830_TRACE_RAM_AFTER").unwrap_or(0);
        let mut remaining = env_u64("E830_TRACE_RAM_LIMIT").unwrap_or(1000);
        cpu.add_mem_hook(
            HookType::MEM_WRITE,
            start.min(end),
            start.max(end),
            move |uc, _, address, size, value| {
                let total = uc.get_data().instructions
                    .saturating_add(uc.get_data().fast_forwarded_instructions);
                if total >= after && remaining > 0 {
                    let pc = uc.reg_read(RegisterARM::PC).unwrap_or(0) as u32;
                    eprintln!(
                        "[e830-ram] write addr=0x{address:08x} size={size} value=0x{value:08x} pc=0x{pc:08x} insns={total}"
                    );
                    remaining -= 1;
                }
                false
            },
        )
        .map_err(|error| unicorn_error("installing the focused RAM trace", error))?;
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
    fn state_dispatch_counters_classify_tables_not_execution_at_data_symbols() {
        let pc = super::STATE_TABLE_DISPATCH_PC;
        assert_eq!(super::state_dispatch_kind(pc, super::KEYBOARD_INIT_TABLE), (true, false));
        assert_eq!(super::state_dispatch_kind(pc, super::STANDBY_LOOK_TABLE), (false, true));
        assert_eq!(super::state_dispatch_kind(pc, 0x3197_f3bc), (false, false));
        assert_eq!(super::state_dispatch_kind(u64::from(super::KEYBOARD_INIT_TABLE),
            super::KEYBOARD_INIT_TABLE), (false, false));
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
        assert_eq!(cpu.get_data().enabled_pending_lines(), 1 << super::DMA_IRQ);
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
        assert_eq!(cpu.get_data().enabled_pending_lines(), 1 << super::DSP_SERIAL_IRQ);
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
    fn finds_the_thumb_fade_delay_loop() {
        let mut image = vec![0; 16];
        image[4..10].copy_from_slice(&FADE_DELAY_LOOP);
        assert_eq!(
            find_fade_delay_loop_pcs(&image),
            [u64::from(FLASH_BASE) + 4].into()
        );
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
