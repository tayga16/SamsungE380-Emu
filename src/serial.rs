use std::collections::VecDeque;

/// Small synchronous serial controller used by the late E830 hardware probe.
/// The retail firmware issues one write transaction followed by two one-byte
/// reads and accepts device IDs BE/15, BE/16 or BE/18.  The fitted device is
/// still unidentified; expose the first accepted profile without inventing
/// data beyond that observed identification exchange.
#[derive(Debug)]
pub(crate) struct SerialController {
    status: u32,
    command: u32,
    identification: [u8; 2],
    identification_index: usize,
    rx_fifo: VecDeque<u8>,
}

impl Default for SerialController {
    fn default() -> Self {
        Self {
            status: 0,
            command: 0,
            identification: [0xbe, 0x15],
            identification_index: 0,
            rx_fifo: VecDeque::new(),
        }
    }
}

impl SerialController {
    const TX_COMPLETE: u32 = 1 << 10;
    const RX_READY: u32 = 1 << 11;

    pub(crate) fn read(&mut self, offset: u32, size: usize) -> u64 {
        let aligned = offset & !3;
        let raw = match aligned {
            0x00 => self.status,
            0x10 => self.command,
            0x14 => {
                let value = self.rx_fifo.pop_front().unwrap_or(0);
                if self.rx_fifo.is_empty() {
                    self.status &= !Self::RX_READY;
                }
                u32::from(value)
            }
            _ => 0,
        };
        let shift = (offset & 3) * 8;
        let bits = size.min(4) * 8;
        let mask = if bits == 32 { u32::MAX } else { (1u32 << bits) - 1 };
        u64::from((raw >> shift) & mask)
    }

    pub(crate) fn write(&mut self, offset: u32, size: usize, value: u64) {
        let aligned = offset & !3;
        let shift = (offset & 3) * 8;
        let bits = size.min(4) * 8;
        let value_mask = if bits == 32 { u32::MAX } else { (1u32 << bits) - 1 };
        let mask = value_mask << shift;
        match aligned {
            0x08 => {
                let acknowledged = ((value as u32) & value_mask) << shift;
                self.status &= !(acknowledged & (Self::TX_COMPLETE | Self::RX_READY));
            }
            0x10 => {
                self.command = (self.command & !mask) | (((value as u32) << shift) & mask);
                match self.command {
                    0x00fd_0501 => {
                        self.identification_index = 0;
                        self.rx_fifo.clear();
                        self.status |= Self::TX_COMPLETE;
                    }
                    0x00fd_0201 => {
                        let value = self.identification
                            .get(self.identification_index)
                            .copied()
                            .unwrap_or(0xff);
                        self.identification_index = self.identification_index.saturating_add(1);
                        self.rx_fifo.push_back(value);
                        self.status |= Self::RX_READY;
                    }
                    _ => {
                        self.status |= Self::TX_COMPLETE;
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SerialController;

    #[test]
    fn identification_exchange_reports_the_retail_accepted_profile() {
        let mut serial = SerialController::default();
        serial.write(0x10, 4, 0x00fd_0501);
        assert_ne!(serial.read(0, 4) & (1 << 10), 0);
        serial.write(8, 2, 1 << 10);
        serial.write(0x10, 4, 0x00fd_0201);
        assert_ne!(serial.read(0, 4) & (1 << 11), 0);
        serial.write(8, 2, 1 << 11);
        assert_eq!(serial.read(0x14, 1), 0xbe);
        serial.write(0x10, 4, 0x00fd_0201);
        serial.write(8, 2, 1 << 11);
        assert_eq!(serial.read(0x14, 1), 0x15);
    }
}
