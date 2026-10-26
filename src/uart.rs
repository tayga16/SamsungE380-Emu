use std::collections::VecDeque;

const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// Polled serial port with an immediately drained transmitter and no incoming
/// cable data. E830 firmware uses the 16550 register spacing (four bytes).
#[derive(Default)]
pub(crate) struct Uart {
    registers: [u8; 8],
    divisor: [u8; 2],
    output: VecDeque<u8>,
}

impl Uart {
    fn divisor_latched(&self) -> bool { self.registers[3] & 0x80 != 0 }

    pub fn read(&self, offset: u32) -> u32 {
        let index = (offset / 4) as usize;
        match index {
            0 | 1 if self.divisor_latched() => u32::from(self.divisor[index]),
            0 => 0, // No received bytes.
            2 => 1 | if self.registers[2] & 1 != 0 { 0xc0 } else { 0 },
            5 => 0x60, // THRE and TEMT; RX/error flags stay clear.
            _ => self.registers.get(index).copied().map(u32::from).unwrap_or(0),
        }
    }

    pub fn write(&mut self, offset: u32, value: u32) {
        let index = (offset / 4) as usize;
        match index {
            0 | 1 if self.divisor_latched() => self.divisor[index] = value as u8,
            0 => {
                if self.output.len() == MAX_OUTPUT_BYTES { self.output.pop_front(); }
                self.output.push_back(value as u8);
            }
            5 | 6 => {}, // Read-only status registers.
            _ => { if let Some(register) = self.registers.get_mut(index) { *register = value as u8; } }
        }
    }

    pub fn output(&self) -> Vec<u8> { self.output.iter().copied().collect() }
}

#[cfg(test)]
mod tests {
    use super::Uart;

    #[test]
    fn idle_transmitter_is_ready_without_inventing_received_data() {
        let mut uart = Uart::default();
        assert_eq!(uart.read(0x14), 0x60);
        assert_eq!(uart.read(0x08), 1);
        uart.write(0, b'I' as u32);
        assert_eq!(uart.output(), b"I");
        assert_eq!(uart.read(0), 0);
        assert_eq!(uart.read(0x14), 0x60);
        uart.write(0x14, 1);
        assert_eq!(uart.read(0x14), 0x60);
    }

    #[test]
    fn baud_divisor_does_not_become_transmitted_data() {
        let mut uart = Uart::default();
        uart.write(0x0c, 0x80);
        uart.write(0, 0x12);
        uart.write(4, 0x34);
        assert_eq!(uart.read(0), 0x12);
        assert_eq!(uart.read(4), 0x34);
        assert!(uart.output().is_empty());
        uart.write(0x0c, 3);
        uart.write(0, b'A' as u32);
        assert_eq!(uart.output(), b"A");
    }
}
