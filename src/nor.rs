//! AMD-style x16 commands observed at 0x2115a062..0x2115a0e4.
//! NOR data area starts at 0x21c00000; NF4 payload starts at +0x20000.
//! Commands complete immediately. Exact chip ID and erase timing are unverified.

const SECTOR_SIZE: usize = 0x20000;
const BUFFER_BYTES: usize = 64;

#[derive(Default)]
enum CommandState {
    #[default]
    Array,
    Unlock1,
    Unlock2,
    Erase1,
    Erase2,
    Erase3,
    Program,
    BufferCount { command: usize },
    BufferData { command: usize, count: usize, words: Vec<(usize, u16)> },
}

pub struct NorFlash {
    cells: Vec<u8>,
    state: CommandState,
}

impl NorFlash {
    pub fn new(cells: Vec<u8>) -> Self { Self { cells, state: CommandState::Array } }

    pub fn len(&self) -> usize { self.cells.len() }

    pub fn read(&self, offset: u64, size: usize) -> u64 {
        let mut value = 0;
        for byte in 0..size.min(8) {
            value |= u64::from(self.cells.get(offset as usize + byte).copied().unwrap_or(0xff)) << (8 * byte);
        }
        value
    }

    fn program(&mut self, offset: usize, value: u16) {
        for (index, byte) in value.to_le_bytes().into_iter().enumerate() {
            if let Some(cell) = self.cells.get_mut(offset + index) { *cell &= byte; }
        }
    }

    pub fn write(&mut self, offset: u64, size: usize, value: u64) {
        if size != 2 || offset & 1 != 0 { return; }
        let offset = offset as usize;
        let value = value as u16;
        let previous = std::mem::take(&mut self.state);
        // Program/count/data cycles consume the value as data, including F0/29.
        match previous {
            CommandState::Program => { self.program(offset, value); return; }
            CommandState::BufferCount { command } => {
                let count = usize::from(value) + 1;
                if offset == command && count <= BUFFER_BYTES / 2 {
                    self.state = CommandState::BufferData { command, count, words: Vec::with_capacity(count) };
                }
                return;
            }
            CommandState::BufferData { command, count, mut words } => {
                if words.len() == count {
                    if offset == command && value == 0x29 {
                        for (address, word) in words { self.program(address, word); }
                    }
                } else if offset / BUFFER_BYTES == command / BUFFER_BYTES {
                    if let Some((_, word)) = words.iter_mut().find(|(address, _)| *address == offset) {
                        *word = value;
                    } else { words.push((offset, value)); }
                    self.state = CommandState::BufferData { command, count, words };
                }
                return;
            }
            _ if value == 0xf0 => return,
            CommandState::Array if offset == 0xaaa && value == 0xaa => self.state = CommandState::Unlock1,
            CommandState::Unlock1 if offset == 0x554 && value == 0x55 => self.state = CommandState::Unlock2,
            CommandState::Unlock2 => self.state = match (offset, value) {
                (0xaaa, 0xa0) => CommandState::Program,
                (0xaaa, 0x80) => CommandState::Erase1,
                (_, 0x25) => CommandState::BufferCount { command: offset },
                _ => CommandState::Array,
            },
            CommandState::Erase1 if offset == 0xaaa && value == 0xaa => self.state = CommandState::Erase2,
            CommandState::Erase2 if offset == 0x554 && value == 0x55 => self.state = CommandState::Erase3,
            CommandState::Erase3 if value == 0x30 && offset < self.cells.len() => {
                let start = offset / SECTOR_SIZE * SECTOR_SIZE;
                let end = (start + SECTOR_SIZE).min(self.cells.len());
                self.cells[start..end].fill(0xff);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn unlock(nor: &mut NorFlash) {
        nor.write(0xaaa, 2, 0xaa);
        nor.write(0x554, 2, 0x55);
    }

    #[test]
    fn firmware_buffer_sequence_includes_first_word_at_command_address() {
        let mut nor = NorFlash::new(vec![0xff; SECTOR_SIZE]);
        let start = 0x180;
        unlock(&mut nor);
        nor.write(start, 2, 0x25);
        nor.write(start, 2, 31);
        for index in 0..32 { nor.write(start + index * 2, 2, index); }
        assert_eq!(nor.read(start, 2), 0xffff);
        nor.write(start, 2, 0x29);
        assert_eq!(nor.read(start, 2), 0);
        assert_eq!(nor.read(start + 62, 2), 31);
        assert_eq!(nor.read(start + 64, 2), 0xffff);
    }

    #[test]
    fn erase_changes_only_selected_sector_and_unlock_commands_preserve_cells() {
        let mut nor = NorFlash::new(vec![0; SECTOR_SIZE * 2]);
        unlock(&mut nor);
        nor.write(0xaaa, 2, 0x80);
        unlock(&mut nor);
        nor.write(0x180, 2, 0x30);
        assert_eq!(nor.read(0xaaa, 2), 0xffff);
        assert_eq!(nor.read(SECTOR_SIZE as u64, 2), 0);
    }

    #[test]
    fn programming_requires_command_and_cannot_restore_erased_bits() {
        let mut nor = NorFlash::new(vec![0xff; SECTOR_SIZE]);
        nor.write(0x180, 2, 0);
        assert_eq!(nor.read(0x180, 2), 0xffff);
        unlock(&mut nor);
        nor.write(0xaaa, 2, 0xa0);
        nor.write(0x180, 2, 0x1234);
        unlock(&mut nor);
        nor.write(0xaaa, 2, 0xa0);
        nor.write(0x180, 2, 0xffff);
        assert_eq!(nor.read(0x180, 2), 0x1234);
        nor.write(0xaaa, 2, 0xf0);
        assert_eq!(nor.read(0xaaa, 2), 0xffff);
    }
}
