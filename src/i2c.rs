//! Philips-style I2C master FIFO at 0x50405000 (E830 device 3, IRQ 28).
//! Register bits match the firmware and the documented PNX I2C controller.
//! The E830 PMIC is a PCF5060X-compatible slave at address 0x09.  Other
//! addresses NACK instead of fabricating a successful transfer.
use std::collections::VecDeque;

const FIFO_CAPACITY: usize = 8;
const DONE: u32 = 1;
const NACK: u32 = 4;
const DATA_REQUEST: u32 = 8;
const ACTIVE: u32 = 1 << 5;
const RX_FULL: u32 = 1 << 8;
const RX_EMPTY: u32 = 1 << 9;
const TX_FULL: u32 = 1 << 10;
const TX_EMPTY: u32 = 1 << 11;
const START: u16 = 1 << 8;
const STOP: u16 = 1 << 9;
const RESET: u32 = 1 << 8;
const RX_FULL_IRQ_ENABLE: u32 = 1 << 5;
const RX_DATA_IRQ_ENABLE: u32 = 1 << 6;
const PCF5060X_ADDRESS: u8 = 0x09;
const PCF5060X_REG_OOCS: usize = 0x01;
const PCF5060X_REG_RTCSC: usize = 0x0a;
const PCF5060X_REG_RTCMN: usize = 0x0b;
const PCF5060X_REG_RTCHR: usize = 0x0c;
const PCF5060X_REG_RTCWD: usize = 0x0d;
const PCF5060X_REG_RTCDT: usize = 0x0e;
const PCF5060X_REG_RTCMT: usize = 0x0f;
const PCF5060X_REG_RTCYR: usize = 0x10;
const PCF5060X_OOCS_BATOK: u8 = 1 << 3;
const PCF5060X_OOCS_BACKOK: u8 = 1 << 4;
const PCF5060X_OOCS_TEMPOK: u8 = 1 << 6;
// The E830 ARM runs at eight times the PNX peripheral clock used by this
// controller.  The firmware's 52/103 dividers therefore produce an I2C byte
// in roughly 11,300 ARM instruction slots, not 1,400.
const CPU_PERIPHERAL_CLOCK_RATIO: u64 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    Write,
    Read,
}

#[derive(Clone)]
struct Pcf5060x {
    registers: [u8; 256],
    pointer: u8,
    expect_pointer: bool,
}

impl Default for Pcf5060x {
    fn default() -> Self {
        let mut registers = [0; 256];
        registers[PCF5060X_REG_OOCS] =
            PCF5060X_OOCS_BATOK | PCF5060X_OOCS_BACKOK | PCF5060X_OOCS_TEMPOK;
        // Hardware RTC registers are packed BCD. Use the firmware build time
        // printed by the original CLA (2007-07-18 13:45:53) as a deterministic
        // valid power-on state instead of the impossible all-zero date that
        // sends the guest into its emergency RTC-reset path.
        registers[PCF5060X_REG_RTCSC] = 0x53;
        registers[PCF5060X_REG_RTCMN] = 0x45;
        registers[PCF5060X_REG_RTCHR] = 0x13;
        registers[PCF5060X_REG_RTCWD] = 0x03;
        registers[PCF5060X_REG_RTCDT] = 0x18;
        registers[PCF5060X_REG_RTCMT] = 0x07;
        registers[PCF5060X_REG_RTCYR] = 0x07;
        Self {
            registers,
            pointer: 0,
            expect_pointer: true,
        }
    }
}

impl Pcf5060x {
    fn start(&mut self, direction: Direction) {
        if direction == Direction::Write {
            self.expect_pointer = true;
        }
    }

    fn write(&mut self, value: u8) {
        if self.expect_pointer {
            self.pointer = value;
            self.expect_pointer = false;
        } else {
            self.registers[usize::from(self.pointer)] = value;
            self.pointer = self.pointer.wrapping_add(1);
        }
    }

    fn read(&mut self) -> u8 {
        let value = self.registers[usize::from(self.pointer)];
        self.pointer = self.pointer.wrapping_add(1);
        value
    }
}

#[derive(Default)]
pub struct I2c {
    control: u32,
    flags: u32,
    clock_low: u32,
    clock_high: u32,
    address: u32,
    tx_fifo: VecDeque<u16>,
    rx_fifo: VecDeque<u8>,
    deadline: Option<u64>,
    active: bool,
    direction: Option<Direction>,
    transmitted: u32,
    received: u32,
    pcf5060x: Pcf5060x,
}

impl I2c {
    fn status(&self) -> u32 {
        self.flags | 0xc0
            | if self.rx_fifo.is_empty() { RX_EMPTY } else { 0 }
            | if self.rx_fifo.len() == FIFO_CAPACITY { RX_FULL } else { 0 }
            | if self.tx_fifo.is_empty() { TX_EMPTY } else { 0 }
            | if self.tx_fifo.len() == FIFO_CAPACITY { TX_FULL } else { 0 }
            | if self.active {
                ACTIVE | if self.tx_fifo.is_empty() { DATA_REQUEST } else { 0 }
            } else {
                0
            }
    }

    pub fn irq_pending(&self) -> bool {
        let status = self.status();
        status & self.control & 0x0f != 0
            || !self.rx_fifo.is_empty()
                && self.control & (RX_FULL_IRQ_ENABLE | RX_DATA_IRQ_ENABLE) != 0
    }

    pub fn read(&mut self, offset: u32) -> u32 {
        match offset {
            0 => self.rx_fifo.pop_front().map(u32::from).unwrap_or(0),
            0x04 => self.status(),
            0x08 => self.control,
            0x0c => self.clock_low,
            0x10 => self.clock_high,
            0x14 => self.address,
            0x18 => self.rx_fifo.len() as u32,
            0x1c => self.tx_fifo.len() as u32,
            0x20 => self.received,
            0x24 => self.transmitted,
            _ => 0,
        }
    }

    fn byte_period(&self) -> u64 {
        // Deterministic virtual bus clock. CPU/bus clock ratio is provisional;
        // guest polling never advances time by itself.
        (u64::from(self.clock_low) + u64::from(self.clock_high) + 2).max(16)
            * 9
            * CPU_PERIPHERAL_CLOCK_RATIO
    }

    pub fn write(&mut self, offset: u32, value: u32, now: u64) {
        match offset {
            0 => {
                if self.tx_fifo.len() < FIFO_CAPACITY {
                    self.tx_fifo.push_back(value as u16);
                    if self.deadline.is_none() {
                        self.deadline = Some(now.saturating_add(self.byte_period()));
                    }
                }
            }
            4 => self.flags &= !(value & 7), // TDI, arbitration failure, NACK: W1C
            8 => {
                self.control = value & !RESET;
                if value & RESET != 0 {
                    self.flags = 0;
                    self.tx_fifo.clear();
                    self.rx_fifo.clear();
                    self.deadline = None;
                    self.active = false;
                    self.direction = None;
                    self.transmitted = 0;
                    self.received = 0;
                }
            }
            0x0c => self.clock_low = value & 0xffff,
            0x10 => self.clock_high = value & 0xffff,
            0x14 => self.address = value & 0xff,
            _ => {}
        }
    }

    pub fn advance(&mut self, now: u64) {
        while let Some(deadline) = self.deadline.filter(|deadline| now >= *deadline) {
            let Some(word) = self.tx_fifo.pop_front() else { self.deadline = None; break; };
            self.transmitted = self.transmitted.wrapping_add(1);
            if word & START != 0 {
                let slave = ((word & 0xfe) >> 1) as u8;
                let direction = if word & 1 == 0 { Direction::Write } else { Direction::Read };
                if slave == PCF5060X_ADDRESS {
                    self.active = true;
                    self.direction = Some(direction);
                    self.pcf5060x.start(direction);
                    if std::env::var_os("E830_TRACE_I2C").is_some() {
                        eprintln!(
                            "[e830-i2c] start slave=0x{slave:02x} direction={direction:?} insns={deadline}"
                        );
                    }
                } else {
                    self.flags |= NACK;
                    self.active = false;
                    self.direction = None;
                    self.tx_fifo.clear();
                }
            } else if self.active {
                match self.direction {
                    Some(Direction::Write) => {
                        let value = word as u8;
                        self.pcf5060x.write(value);
                        if std::env::var_os("E830_TRACE_I2C").is_some() {
                            eprintln!(
                                "[e830-i2c] write slave=0x{PCF5060X_ADDRESS:02x} value=0x{value:02x} stop={} insns={deadline}",
                                word & STOP != 0,
                            );
                        }
                    }
                    Some(Direction::Read) => {
                        let register = self.pcf5060x.pointer;
                        let value = self.pcf5060x.read();
                        if self.rx_fifo.len() < FIFO_CAPACITY {
                            self.rx_fifo.push_back(value);
                            self.received = self.received.wrapping_add(1);
                        }
                        if std::env::var_os("E830_TRACE_I2C").is_some() {
                            eprintln!(
                                "[e830-i2c] read slave=0x{PCF5060X_ADDRESS:02x} register=0x{register:02x} value=0x{value:02x} stop={} insns={deadline}",
                                word & STOP != 0,
                            );
                        }
                    }
                    None => {}
                }
            }
            if word & STOP != 0 {
                self.flags |= DONE;
                self.active = false;
                self.direction = None;
            }
            self.deadline = (!self.tx_fifo.is_empty())
                .then(|| deadline.saturating_add(self.byte_period()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_and_reset_have_empty_fifos_without_interrupts() {
        let mut i2c = I2c::default();
        assert_eq!(i2c.read(4) & (RX_EMPTY | TX_EMPTY | TX_FULL), RX_EMPTY | TX_EMPTY);
        i2c.write(8, 0x2f, 0);
        assert!(!i2c.irq_pending());
        i2c.write(0, 0x112, 0);
        i2c.write(8, RESET | 0x2f, 1);
        i2c.advance(10_000);
        assert_eq!(i2c.read(8), 0x2f);
        assert_eq!(i2c.read(0x24), 0);
        assert!(!i2c.irq_pending());
    }

    #[test]
    fn missing_slave_nacks_after_bus_time_and_acknowledgement_clears_irq() {
        let mut i2c = I2c::default();
        i2c.write(8, 0x2f, 0);
        i2c.write(0, 0x114, 10);
        i2c.advance(11);
        assert!(!i2c.irq_pending());
        i2c.advance(2_000);
        assert!(i2c.irq_pending());
        assert_eq!(i2c.read(4) & 7, NACK);
        assert_eq!(i2c.read(0x24), 1);
        i2c.write(8, 0, 2_000);
        assert!(!i2c.irq_pending());
        i2c.write(8, 0x2f, 2_000);
        assert!(i2c.irq_pending());
        i2c.write(4, NACK, 2_000);
        assert!(!i2c.irq_pending());
    }

    #[test]
    fn pcf5060x_register_pointer_and_rx_fifo_follow_pnx_transactions() {
        let mut i2c = I2c::default();
        i2c.write(8, 0x2f, 0);

        i2c.write(
            0,
            u32::from(START | u16::from(PCF5060X_ADDRESS << 1)),
            10,
        );
        i2c.advance(10_000);
        assert_eq!(i2c.status() & (ACTIVE | DATA_REQUEST), ACTIVE | DATA_REQUEST);
        assert!(i2c.irq_pending());

        i2c.write(0, u32::from(STOP | 5), 10_000);
        i2c.advance(20_000);
        assert_eq!(i2c.pcf5060x.pointer, 5);
        assert_eq!(i2c.status() & ACTIVE, 0);

        i2c.write(4, DONE, 20_000);
        i2c.write(
            0,
            u32::from(START | u16::from(PCF5060X_ADDRESS << 1) | 1),
            20_000,
        );
        i2c.advance(30_000);
        i2c.write(8, RX_FULL_IRQ_ENABLE | RX_DATA_IRQ_ENABLE, 30_000);
        i2c.write(0, u32::from(STOP), 30_000);
        i2c.advance(40_000);

        assert_eq!(i2c.read(0x18), 1);
        assert!(i2c.irq_pending());
        assert_eq!(i2c.read(0), 0);
        assert_eq!(i2c.read(0x18), 0);
    }

    #[test]
    fn pcf5060x_starts_with_valid_power_and_rtc_status() {
        let pcf = Pcf5060x::default();
        assert_eq!(
            pcf.registers[PCF5060X_REG_OOCS],
            PCF5060X_OOCS_BATOK | PCF5060X_OOCS_BACKOK | PCF5060X_OOCS_TEMPOK
        );
        assert_eq!(
            &pcf.registers[PCF5060X_REG_RTCSC..=PCF5060X_REG_RTCYR],
            &[0x53, 0x45, 0x13, 0x03, 0x18, 0x07, 0x07]
        );
    }
}
