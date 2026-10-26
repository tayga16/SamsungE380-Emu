//! Philips-style I2C master FIFO at 0x50405000 (E830 device 3, IRQ 28).
//! Register bits match the firmware and the documented PNX I2C controller.
//! Unattached slaves NACK; no peripheral response data is fabricated.
use std::collections::VecDeque;

const FIFO_CAPACITY: usize = 8;
const DONE: u32 = 1;
const NACK: u32 = 4;
const DATA_REQUEST: u32 = 8;
const ACTIVE: u32 = 1 << 5;
const RX_EMPTY: u32 = 1 << 9;
const TX_FULL: u32 = 1 << 10;
const TX_EMPTY: u32 = 1 << 11;
const START: u16 = 1 << 8;
const STOP: u16 = 1 << 9;
const RESET: u32 = 1 << 8;

#[derive(Default)]
pub struct I2c {
    control: u32,
    flags: u32,
    clock_low: u32,
    clock_high: u32,
    address: u32,
    fifo: VecDeque<u16>,
    deadline: Option<u64>,
    active: bool,
    transmitted: u32,
}

impl I2c {
    fn status(&self) -> u32 {
        self.flags | RX_EMPTY | 0xc0
            | if self.fifo.is_empty() { TX_EMPTY } else { 0 }
            | if self.fifo.len() == FIFO_CAPACITY { TX_FULL } else { 0 }
            | if self.active { ACTIVE | if self.fifo.is_empty() { DATA_REQUEST } else { 0 } } else { 0 }
    }

    pub fn irq_pending(&self) -> bool {
        self.status() & self.control & 0x0f != 0
    }

    pub fn read(&self, offset: u32) -> u32 {
        match offset {
            0x04 => self.status(),
            0x08 => self.control,
            0x0c => self.clock_low,
            0x10 => self.clock_high,
            0x14 => self.address,
            0x1c => self.fifo.len() as u32,
            0x24 => self.transmitted,
            _ => 0,
        }
    }

    fn byte_period(&self) -> u64 {
        // Deterministic virtual bus clock. CPU/bus clock ratio is provisional;
        // guest polling never advances time by itself.
        (u64::from(self.clock_low) + u64::from(self.clock_high) + 2).max(16) * 9
    }

    pub fn write(&mut self, offset: u32, value: u32, now: u64) {
        match offset {
            0 => {
                if self.fifo.len() < FIFO_CAPACITY {
                    self.fifo.push_back(value as u16);
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
                    self.fifo.clear();
                    self.deadline = None;
                    self.active = false;
                    self.transmitted = 0;
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
            let Some(word) = self.fifo.pop_front() else { self.deadline = None; break; };
            self.transmitted = self.transmitted.wrapping_add(1);
            if word & START != 0 {
                // No slave has been connected yet. A real empty bus returns a
                // negative acknowledgement instead of hanging the CPU forever.
                self.flags |= NACK;
                self.active = false;
                self.fifo.clear();
            } else if word & STOP != 0 {
                self.flags |= DONE;
                self.active = false;
            }
            self.deadline = (!self.fifo.is_empty()).then(|| deadline.saturating_add(self.byte_period()));
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
        i2c.write(0, 0x112, 10);
        i2c.advance(11);
        assert!(!i2c.irq_pending());
        i2c.advance(200);
        assert!(i2c.irq_pending());
        assert_eq!(i2c.read(4) & 7, NACK);
        assert_eq!(i2c.read(0x24), 1);
        i2c.write(8, 0, 200);
        assert!(!i2c.irq_pending());
        i2c.write(8, 0x2f, 200);
        assert!(i2c.irq_pending());
        i2c.write(4, NACK, 200);
        assert!(!i2c.irq_pending());
    }
}
