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
// Shared DSP data/mailbox addresses used by the firmware (including
// 0x60000008 and 0x600007e0). Storage only; DSP execution is not implemented.
const DSP_SHARED_RAM_BASE: u64 = 0x6000_0000;
const DSP_SHARED_RAM_SIZE: u64 = 0x0001_0000;
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
const I2C_BASE: u32 = 0x5040_5000;
const I2C_IRQ: u32 = 28;
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
const KEYBOARD_INIT_PC: u64 = 0x20e8_2eac;
const STANDBY_LOOK_PC: u64 = 0x2171_7f98;
// Interrupt controller (base 0x50302000), derived from the firmware's ARM IRQ
// vector handler at 0x20cb0cd0: it loads r4 = 0x50302000, reads the pending
// line number as a byte at the base (already multiplied by four), filters it
// against the pending bitmask at +0x1c, dispatches through a RAM table, then
// writes 0x80 to the end-of-interrupt register at +0x08 to acknowledge.
const INTC_PENDING_NUMBER_REGISTER: u32 = 0x5030_2000;
const INTC_PENDING_MASK_REGISTER: u32 = 0x5030_201c;
const INTC_MASKED_PENDING_REGISTER: u32 = 0x5030_2014;
// Interrupt-line mask: a set bit masks the source. +0x14 reports unmasked
// pending sources; +0x1c reports pending sources regardless of this mask.
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

fn env_u64(name: &str) -> Option<u64> {
    let raw = std::env::var(name).ok()?;
    let trimmed = raw.trim();
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse().ok()
    }
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
    pub keyboard_init_entries: u64,
    pub standby_look_entries: u64,
    pub irqs_taken: u64,
}

struct E830State {
    registers: HashMap<u32, u32>,
    nor: crate::nor::NorFlash,
    lcd_bus_mode: u16,
    lcd_register_index: u16,
    lcd_framebuffer: Vec<u16>,
    lcd_pixel_cursor: usize,
    lcd_frames: u64,
    keyboard_init_entries: u64,
    standby_look_entries: u64,
    timer: u32,
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
    i2c: crate::i2c::I2c,
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
    irqs_taken: u64,
    // Diagnostic: when `pc_log_remaining` is non-zero and the executed
    // instruction count has passed `pc_log_after`, each real (non
    // fast-forwarded) instruction's PC is logged to stderr and the counter
    // decremented. Driven by E830_PC_LOG / _AFTER; off by default.
    pc_log_after: u64,
    pc_log_remaining: u64,
    // Optional read-only register trace at selected instruction addresses.
    watch_pcs: HashSet<u64>,
    watch_remaining: u64,
    // Diagnostic: when set (E830_TRACE_IRQEN), log the first PC at
    // which the CPSR IRQ mask (I) and FIQ mask (F) are observed cleared. This
    // pins down whether and where early boot ever unmasks interrupts.
    trace_irqen: bool,
    irqen_logged_i: bool,
    irqen_logged_f: bool,
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
            nor: crate::nor::NorFlash::new(flash),
            lcd_bus_mode: 0,
            lcd_register_index: 0,
            lcd_framebuffer: vec![0; LCD_PIXEL_COUNT],
            lcd_pixel_cursor: 0,
            lcd_frames: 0,
            keyboard_init_entries: 0,
            standby_look_entries: 0,
            timer: 0,
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
            i2c: Default::default(),
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
            irqs_taken: 0,
            pc_log_after: env_u64("E830_PC_LOG_AFTER").unwrap_or(0),
            pc_log_remaining: env_u64("E830_PC_LOG").unwrap_or(0),
            watch_pcs: std::env::var("E830_WATCH_PC").unwrap_or_default()
                .split(',').filter_map(|raw| {
                    let raw = raw.trim();
                    raw.strip_prefix("0x").map(|hex| u64::from_str_radix(hex, 16))
                        .unwrap_or_else(|| raw.parse()).ok().map(|pc| pc & !1)
                }).collect(),
            watch_remaining: env_u64("E830_WATCH_LIMIT").unwrap_or(1000),
            trace_irqen: std::env::var("E830_TRACE_IRQEN").is_ok(),
            irqen_logged_i: false,
            irqen_logged_f: false,
        }
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
        self.recent_mmio.push_back(access);
    }

    // Device interrupt levels persist until their own status is cleared.
    fn pending_lines(&self) -> u32 {
        let diagnostic = if self.timer_irq_pending { 1 << (self.timer_irq_index & 31) } else { 0 };
        let i2c = if self.i2c.irq_pending() { 1 << I2C_IRQ } else { 0 };
        diagnostic | i2c
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
        if u64::from(address) >= ONENAND_BASE && u64::from(address) < ONENAND_BASE + ONENAND_SIZE {
            return self.onenand.read(address - ONENAND_BASE as u32, size);
        }
        let aligned = address & !3;
        let raw = if let Some((channel, base)) = UART_BASES.iter().enumerate()
            .find(|(_, base)| aligned >= **base && aligned < **base + 0x20) {
            self.uarts[channel].read(aligned - base)
        } else if aligned >= I2C_BASE && aligned < I2C_BASE + 0x30 {
            self.i2c.read(aligned - I2C_BASE)
        } else if SOC_KEYPAD_SCAN_READ_PCS.contains(&pc)
            && SOC_KEYPAD_INPUT_REGISTERS.contains(&aligned)
        {
            u32::MAX
        } else if address == SOC_TIMER {
            let value = self.timer;
            self.timer = self.timer.wrapping_add(4);
            value
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
            // The handler reads this twice and XORs the pair as a glitch filter;
            // a stable value keeps the fast path that trusts the number register.
            self.pending_lines()
        } else if aligned == INTC_MASKED_PENDING_REGISTER {
            self.enabled_pending_lines()
        } else {
            self.registers.get(&aligned).copied().unwrap_or(0)
        };
        let shift = (address & 3) * 8;
        let mask = width_mask(size);
        u64::from((raw >> shift) & mask)
    }

    fn write_register(&mut self, address: u32, size: usize, value: u64) {
        if u64::from(address) >= ONENAND_BASE && u64::from(address) < ONENAND_BASE + ONENAND_SIZE {
            self.onenand.write(address - ONENAND_BASE as u32, size, value);
            return;
        }
        let aligned = address & !3;
        if aligned >= I2C_BASE && aligned < I2C_BASE + 0x30 {
            let now = self.instructions.saturating_add(self.fast_forwarded_instructions);
            self.i2c.write(aligned - I2C_BASE, value as u32, now);
            return;
        }
        if let Some((channel, base)) = UART_BASES.iter().enumerate()
            .find(|(_, base)| aligned >= **base && aligned < **base + 0x20) {
            self.uarts[channel].write(aligned - base, value as u32);
            return;
        }
        let shift = (address & 3) * 8;
        let value_mask = width_mask(size);
        let mask = value_mask.wrapping_shl(shift);
        let updated = {
            let register = self.registers.entry(aligned).or_default();
            *register = (*register & !mask) | (((value as u32).wrapping_shl(shift)) & mask);
            *register
        };
        if aligned == SOC_INIT_CONTROL_REGISTER && updated & 0x80 != 0 {
            *self.registers.entry(SOC_INIT_STATUS_REGISTER).or_default() |= 0x07;
            // The IRQ vector handler writes 0x80 here as its end-of-interrupt
            // acknowledge (`strb` of 0x80 to 0x50302008). Drop the raised line
            // so the next tick is a fresh edge rather than an immediate re-entry.
            if self.active_irq_line == self.timer_irq_index {
                self.timer_irq_pending = false;
            }
            self.timer_irq_in_service = false;
        }
        self.write_lcd_register(address, size, value);
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
        } else if self.lcd_bus_mode == 1 && self.lcd_register_index == LCD_GRAM_REGISTER {
            self.lcd_framebuffer[self.lcd_pixel_cursor] = value;
            self.lcd_pixel_cursor += 1;
            if self.lcd_pixel_cursor == LCD_PIXEL_COUNT {
                self.lcd_frames = self.lcd_frames.saturating_add(1);
                self.lcd_pixel_cursor = 0;
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
            "mapping CLA flash",
        )?;
        cpu.mem_write(u64::from(FLASH_BASE), firmware.cla())
            .map_err(|error| unicorn_error("loading CLA flash", error))?;
        cpu.mem_protect(u64::from(FLASH_BASE), flash_size, Prot::READ | Prot::EXEC)
            .map_err(|error| unicorn_error("protecting CLA flash", error))?;
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
        map_memory(cpu, DSP_AUX_RAM_BASE, DSP_AUX_RAM_SIZE,
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
            keyboard_init_entries: state.keyboard_init_entries,
            standby_look_entries: state.standby_look_entries,
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
                let state = uc.get_data_mut();
                state.write_register(address, access_size, value);
                state.record_mmio(MmioAccess {
                    write: true,
                    address,
                    size: access_size,
                    value,
                    pc,
                });
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
                match address {
                    KEYBOARD_INIT_PC => {
                        state.keyboard_init_entries = state.keyboard_init_entries.saturating_add(1)
                    }
                    STANDBY_LOOK_PC => {
                        state.standby_look_entries = state.standby_look_entries.saturating_add(1)
                    }
                    _ => {}
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

        if uc.get_data().watch_remaining > 0 && uc.get_data().watch_pcs.contains(&address) {
            let total = uc.get_data().instructions.saturating_add(uc.get_data().fast_forwarded_instructions);
            let registers = [RegisterARM::R0, RegisterARM::R1, RegisterARM::R2, RegisterARM::R3,
                RegisterARM::R4, RegisterARM::R5, RegisterARM::R6, RegisterARM::R7,
                RegisterARM::SP, RegisterARM::LR, RegisterARM::CPSR];
            let values: Vec<String> = registers.into_iter().map(|register|
                format!("{:?}=0x{:08x}", register, uc.reg_read(register).unwrap_or(0))).collect();
            eprintln!("[e830-watch] pc=0x{address:08x} insns={total} {}", values.join(" "));
            uc.get_data_mut().watch_remaining -= 1;
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
        let irq_is_pending = {
            let state = uc.get_data_mut();
            let total = state.instructions.saturating_add(state.fast_forwarded_instructions);
            state.i2c.advance(total);
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
            let irqs_enabled = cpsr & CPSR_IRQ_DISABLE == 0;
            let already_in_irq = cpsr & CPSR_MODE_MASK == CPSR_MODE_IRQ;
            let in_service = uc.get_data().timer_irq_in_service;
            let pending = uc.get_data().enabled_pending_lines();
            let line_enabled = pending != 0;
            let line = 31 - pending.leading_zeros();
            if irqs_enabled && line_enabled && !already_in_irq && !in_service {
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
        find_fade_delay_loop_pcs, find_nested_delay_loop_pcs, E830Machine, E830State,
        COUNTING_DELAY_LOOP, FADE_DELAY_LOOP,
        E830_TFS_SOURCE_BASE, FLASH_BASE,
        NESTED_DELAY_LOOP, SOC_BOOT_READY_INPUT_MASK, SOC_BOOT_READY_INPUT_REGISTER,
        SOC_KEYPAD_INPUT_REGISTERS, SOC_KEYPAD_SCAN_READ_PCS,
    };

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
        assert_eq!(machine.peek_u32(super::DSP_AUX_RAM_BASE as u32).unwrap(), 0);
        let cpu = machine.cpu.raw_engine();
        let mut readback = vec![0; package.len()];
        cpu.mem_read(E830_TFS_SOURCE_BASE, &mut readback).unwrap();
        assert_eq!(readback, package);
    }
}
