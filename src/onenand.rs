//! 32 MiB Samsung OneNAND profile at 0x48000000.
//! Capacity supplied by the user. ID ec/14/version 0 selects the firmware's
//! 512-block, 64-page, two-sector/page addressing; the exact chip is unverified.
//! The physical dump/partition placement is unverified;
//! cells begin erased and guest writes stay in memory, separate from input files.
use std::collections::{HashMap, HashSet};

const PAGE_SIZE: usize = 1024;
const SPARE_SIZE: usize = 32;
const SECTOR_SIZE: usize = 512;
const SECTOR_SPARE_SIZE: usize = 16;
const PAGE_SECTORS: usize = PAGE_SIZE / SECTOR_SIZE;
const BLOCK_PAGES: u32 = 64;
const BLOCKS: u32 = 512;
const COMMAND: u32 = 0x1e440;
const STATUS: u32 = 0x1e480;
const INTERRUPT: u32 = 0x1e482;

/// Paired row/column Hamming parity for a 512-byte NAND sector (SMC order).
/// Compute the parity groups directly, independently of reference drivers.
/// Physical OneNAND OOB puts main ECC at bytes 8..10 of each 16-byte sector.
/// The exact fitted chip's byte order still needs a hardware capture.
fn main_ecc(data: &[u8]) -> [u8; 3] {
    debug_assert_eq!(data.len(), SECTOR_SIZE);
    let mut ecc = [0xff; 3];
    for (row, byte) in data.iter().copied().enumerate() {
        if byte.count_ones() & 1 != 0 {
            for bit in 0..9 {
                let (index, shift) = if bit < 8 { (bit / 4, (bit % 4) * 2) }
                    else { (2, 0) };
                ecc[index] ^= 1 << (shift + ((row >> bit) & 1));
            }
        }
        for bit in 0..3 {
            for half in 0..2 {
                let parity = (0..8).filter(|column| (column >> bit) & 1 == half)
                    .fold(0, |parity, column| parity ^ ((byte >> column) & 1));
                ecc[2] ^= parity << (2 + bit * 2 + half);
            }
        }
    }
    ecc
}

pub struct OneNand {
    window: Vec<u8>,
    pages: HashMap<u32, Vec<u8>>,
    image: Vec<u8>,
    image_stride: usize,
    erased_blocks: HashSet<u32>,
    pending_erases: HashSet<u32>,
}

impl Default for OneNand {
    fn default() -> Self {
        let mut device = Self { window: vec![0xff; 0x20000], pages: HashMap::new(),
            image: Vec::new(), image_stride: PAGE_SIZE, erased_blocks: HashSet::new(),
            pending_erases: HashSet::new() };
        for (offset, value) in [
            (0x1e000, 0x00ec), (0x1e002, 0x0014), (0x1e004, 0),
            (0x1e006, PAGE_SIZE as u16), (0x1e008, 512), (0x1e00a, 2),
            (0x1e00c, 0), (0x1e200, 0), (0x1e202, 0), (0x1e20e, 0),
            (0x1e400, 0x0800), (COMMAND, 0), (0x1e442, 0x40c0),
            (STATUS, 0), (INTERRUPT, 0x8000), (0x1e498, 0),
            (0x1e49a, (BLOCKS - 1) as u16), (0x1e49c, 4), (0x1fe00, 0),
        ] { device.put(offset, value); }
        // Hardware ECC result/status registers reset to zero. The erased
        // backing has no injected bit errors.
        device.window[0x1fe00..0x1fe12].fill(0);
        device
    }
}

impl OneNand {
    /// Load a physical 32 MiB dump, either main-only or page+spare interleaved.
    /// All subsequent guest modifications use a separate in-memory overlay.
    pub fn load_image(&mut self, image: Vec<u8>) -> Result<(), usize> {
        let page_count = (BLOCKS * BLOCK_PAGES) as usize;
        let stride = match image.len() {
            size if size == page_count * PAGE_SIZE => PAGE_SIZE,
            size if size == page_count * (PAGE_SIZE + SPARE_SIZE) => PAGE_SIZE + SPARE_SIZE,
            size => return Err(size),
        };
        self.image = image;
        self.image_stride = stride;
        self.pages.clear();
        self.erased_blocks.clear();
        self.pending_erases.clear();
        Ok(())
    }

    /// Export physical pages including spare, source cells and guest changes.
    pub fn snapshot(&self) -> Vec<u8> {
        let stride = PAGE_SIZE + SPARE_SIZE;
        let page_count = BLOCKS * BLOCK_PAGES;
        let mut image = Vec::with_capacity(page_count as usize * stride);
        for key in 0..page_count {
            image.extend_from_slice(&self.page_contents(key));
        }
        image
    }

    fn page_contents(&self, key: u32) -> Vec<u8> {
        if let Some(contents) = self.pages.get(&key) { return contents.clone(); }
        let mut contents = vec![0xff; PAGE_SIZE + SPARE_SIZE];
        if !self.erased_blocks.contains(&(key / BLOCK_PAGES)) {
            let start = key as usize * self.image_stride;
            if let Some(source) = self.image.get(start..start + self.image_stride) {
                contents[..self.image_stride].copy_from_slice(source);
            }
        }
        contents
    }

    fn get(&self, offset: u32) -> u16 {
        let index = offset as usize;
        u16::from_le_bytes([self.window[index], self.window[index + 1]])
    }

    fn put(&mut self, offset: u32, value: u16) {
        self.window[offset as usize..offset as usize + 2].copy_from_slice(&value.to_le_bytes());
    }

    pub fn read(&self, offset: u32, size: usize) -> u64 {
        let mut value = 0;
        for byte in 0..size.min(8) {
            value |= u64::from(self.window.get(offset as usize + byte).copied().unwrap_or(0xff)) << (byte * 8);
        }
        value
    }

    pub fn write(&mut self, offset: u32, size: usize, value: u64) {
        if (0x1e000..0x1e00e).contains(&offset) { return; }
        for byte in 0..size.min(8) {
            if let Some(cell) = self.window.get_mut(offset as usize + byte) {
                *cell = (value >> (byte * 8)) as u8;
            }
        }
        if offset == COMMAND && size >= 2 { self.execute(value as u16); }
    }

    fn execute(&mut self, command: u16) {
        let block = u32::from(self.get(0x1e200) & 0x7fff);
        let page = u32::from((self.get(0x1e20e) >> 2) & 63);
        let key = block * BLOCK_PAGES + page;
        let buffer = usize::from((self.get(0x1e400) >> 10) & 1);
        let data_offset = 0x400 + buffer * PAGE_SIZE;
        let spare_offset = 0x10020 + buffer * SPARE_SIZE;
        self.put(STATUS, 0);
        let mut interrupt = 0x8000;
        // In the two-sector profile SA8 bit 0 is the sector within the page.
        // Retail 0x20d27350..64 encodes ((logical_sector << 1) & 0xfc)
        // | (logical_sector & 1). The shift is for the page field, not FSA.
        let flash_sector = usize::from(self.get(0x1e20e) & 1);
        let buffer_sector = usize::from((self.get(0x1e400) >> 8) & 3);
        let encoded_count = usize::from(self.get(0x1e400) & 3);
        let sectors = if encoded_count == 0 { PAGE_SECTORS } else { encoded_count };
        let is_transfer = matches!(command, 0x00 | 0x13 | 0x80 | 0x1a);
        // The firmware selects matching page/buffer sector offsets. Transfers
        // spanning a page boundary are not implemented by this device profile.
        if is_transfer && (flash_sector + sectors > PAGE_SECTORS || buffer_sector + sectors > PAGE_SECTORS) {
            self.put(STATUS, 0x0400);
            self.put(INTERRUPT, interrupt);
            return;
        }
        let flash_data = flash_sector * SECTOR_SIZE;
        let flash_spare = PAGE_SIZE + flash_sector * SECTOR_SPARE_SIZE;
        let buffer_data = data_offset + buffer_sector * SECTOR_SIZE;
        let buffer_spare = spare_offset + buffer_sector * SECTOR_SPARE_SIZE;
        let data_bytes = sectors * SECTOR_SIZE;
        let spare_bytes = sectors * SECTOR_SPARE_SIZE;
        match command {
            0x00 | 0x13 if block < BLOCKS => {
                let contents = self.page_contents(key);
                if command == 0x00 {
                    self.window[buffer_data..buffer_data + data_bytes].fill(0xff);
                }
                self.window[buffer_spare..buffer_spare + spare_bytes].fill(0xff);
                if command == 0x00 {
                    self.window[buffer_data..buffer_data + data_bytes]
                        .copy_from_slice(&contents[flash_data..flash_data + data_bytes]);
                }
                self.window[buffer_spare..buffer_spare + spare_bytes]
                    .copy_from_slice(&contents[flash_spare..flash_spare + spare_bytes]);
                interrupt |= 0x80;
            }
            0x80 | 0x1a if block < BLOCKS => {
                if command == 0x80 && self.get(0x1e442) & 0x0100 == 0 {
                    for sector in 0..sectors {
                        let start = buffer_data + sector * SECTOR_SIZE;
                        let ecc = main_ecc(&self.window[start..start + SECTOR_SIZE]);
                        let start = buffer_spare + sector * SECTOR_SPARE_SIZE + 8;
                        self.window[start..start + 3].copy_from_slice(&ecc);
                    }
                }
                let initial = self.page_contents(key);
                let contents = self.pages.entry(key).or_insert(initial);
                if command == 0x80 {
                    for (cell, byte) in contents[flash_data..flash_data + data_bytes].iter_mut()
                        .zip(&self.window[buffer_data..buffer_data + data_bytes]) {
                        *cell &= *byte;
                    }
                }
                for (cell, byte) in contents[flash_spare..flash_spare + spare_bytes].iter_mut()
                    .zip(&self.window[buffer_spare..buffer_spare + spare_bytes]) {
                    *cell &= *byte;
                }
                interrupt |= 0x40;
            }
            0x94 if block < BLOCKS => {
                // 0x95 queues up to 63 preceding blocks; the final 0x94
                // erases the whole group. Linux onenand_multiblock_erase
                // and the original Samsung loader use this sequence.
                self.pending_erases.insert(block);
                for erased in self.pending_erases.drain() {
                    self.pages.retain(|page, _| *page / BLOCK_PAGES != erased);
                    self.erased_blocks.insert(erased);
                }
                interrupt |= 0x20;
            }
            0x95 if block < BLOCKS &&
                (self.pending_erases.contains(&block) || self.pending_erases.len() < 63) => {
                self.pending_erases.insert(block);
            }
            0x71 if block < BLOCKS => {
                // Verification observes cells, including spare; it never erases.
                if (block * BLOCK_PAGES..(block + 1) * BLOCK_PAGES)
                    .any(|key| self.page_contents(key).iter().any(|byte| *byte != 0xff)) {
                    self.put(STATUS, 0x0c00); // controller + erase error
                }
            }
            0xf0 => { self.pending_erases.clear(); self.put(0x1fe00, 0); interrupt |= 0x10; }
            0x23 | 0x27 => self.put(0x1e49c, 4),
            _ => self.put(STATUS, 0x0400), // unsupported/out-of-range command
        }
        self.put(INTERRUPT, interrupt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sector_hamming_parity_identifies_single_bit_positions() {
        let mut data = [0xff; SECTOR_SIZE];
        assert_eq!(main_ecc(&data), [0xff; 3]);
        data[0] ^= 1;
        assert_eq!(main_ecc(&data), [0xaa; 3]);
        data[0] ^= 1;
        data[511] ^= 0x80;
        assert_eq!(main_ecc(&data), [0x55; 3]);
        // Separate row bits from bit positions within a byte.
        data[511] ^= 0x80;
        data[16] ^= 1;
        assert_eq!(main_ecc(&data), [0xaa, 0xa9, 0xaa]);
    }

    #[test]
    fn page_program_read_and_block_erase_use_buffers_and_report_completion() {
        let mut nand = OneNand::default();
        nand.write(0x1e200, 2, 2);
        nand.write(0x1e20e, 2, 3 << 2);
        nand.write(0x400, 2, 0x1234);
        nand.write(COMMAND, 2, 0x80);
        assert_eq!(nand.read(INTERRUPT, 2), 0x8040);
        nand.write(0x400, 2, 0xffff);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 2), 0x1234);
        assert_eq!(nand.read(INTERRUPT, 2), 0x8080);
        nand.write(COMMAND, 2, 0x94);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 2), 0xffff);
    }

    #[test]
    fn invalid_commands_report_failure_and_identification_is_read_only() {
        let mut nand = OneNand::default();
        nand.write(0x1e000, 2, 0);
        assert_eq!(nand.read(0x1e000, 2), 0xec);
        nand.write(COMMAND, 2, 0x7fff);
        assert_eq!(nand.read(STATUS, 2), 0x400);
        assert_eq!(nand.read(INTERRUPT, 2), 0x8000);
    }

    #[test]
    fn grouped_erase_waits_for_final_command_and_verify_checks_main_and_spare() {
        let mut nand = OneNand::default();
        for block in [2, 4, 7] {
            nand.write(0x1e200, 2, block);
            nand.write(0x400, 2, 0x1234);
            nand.write(0x10020, 2, 0xabcd);
            nand.write(COMMAND, 2, 0x80);
        }
        nand.write(0x1e200, 2, 2);
        nand.write(COMMAND, 2, 0x71);
        assert_eq!(nand.read(STATUS, 2), 0x0c00);
        nand.write(COMMAND, 2, 0x95);
        assert_eq!(nand.read(STATUS, 2), 0);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 2), 0x1234);
        nand.write(0x1e200, 2, 4);
        nand.write(COMMAND, 2, 0x94);
        for block in [2, 4] {
            nand.write(0x1e200, 2, block);
            nand.write(COMMAND, 2, 0x71);
            assert_eq!(nand.read(STATUS, 2), 0);
            nand.write(COMMAND, 2, 0);
            assert_eq!(nand.read(0x400, 2), 0xffff);
            assert_eq!(nand.read(0x10020, 2), 0xffff);
        }
        // An unrelated block remains programmed, and verification is read-only.
        nand.write(0x1e200, 2, 7);
        nand.write(COMMAND, 2, 0x71);
        assert_eq!(nand.read(STATUS, 2), 0x0c00);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 2), 0x1234);
        // Spare-only programming also makes erase verification fail.
        nand.write(0x1e200, 2, 2);
        nand.write(0x10020, 2, 0);
        nand.write(COMMAND, 2, 0x1a);
        nand.write(COMMAND, 2, 0x71);
        assert_eq!(nand.read(STATUS, 2), 0x0c00);
    }

    #[test]
    fn reset_cancels_queued_erase_and_rejects_invalid_blocks_and_oversize_groups() {
        let mut nand = OneNand::default();
        nand.write(0x400, 2, 0);
        nand.write(COMMAND, 2, 0x80);
        nand.write(COMMAND, 2, 0x95);
        nand.write(COMMAND, 2, 0xf0);
        nand.write(0x1e200, 2, 1);
        nand.write(COMMAND, 2, 0x94);
        nand.write(0x1e200, 2, 0);
        nand.write(COMMAND, 2, 0x71);
        assert_eq!(nand.read(STATUS, 2), 0x0c00);
        for block in 0..63 {
            nand.write(0x1e200, 2, block);
            nand.write(COMMAND, 2, 0x95);
            assert_eq!(nand.read(STATUS, 2), 0);
        }
        nand.write(0x1e200, 2, 63);
        nand.write(COMMAND, 2, 0x95);
        assert_eq!(nand.read(STATUS, 2), 0x400);
        nand.write(0x1e200, 2, 512);
        for command in [0x95, 0x94, 0x71] {
            nand.write(COMMAND, 2, command);
            assert_eq!(nand.read(STATUS, 2), 0x400);
        }
        assert_eq!(nand.pending_erases.len(), 63);
    }

    #[test]
    fn capacity_and_addressing_match_the_32_mib_profile() {
        let mut nand = OneNand::default();
        assert_eq!(nand.read(0x1e002, 2), 0x14);
        assert_eq!(nand.read(0x1e006, 2), 1024);
        assert_eq!(nand.read(0x1e49a, 2), 511);
        assert_eq!((BLOCKS * BLOCK_PAGES) as usize * PAGE_SIZE, 33_554_432);
        assert_eq!((BLOCKS * BLOCK_PAGES) as usize * (PAGE_SIZE + SPARE_SIZE), 34_603_008);
        // Last sector of the last page, in BufferRAM bank 1.
        nand.write(0x1e200, 2, 511);
        nand.write(0x1e20e, 2, (63 << 2) | 1);
        nand.write(0x1e400, 2, 0x0d01);
        nand.write(0xa00, 2, 0x5678);
        nand.write(COMMAND, 2, 0x80);
        assert_eq!(nand.read(STATUS, 2), 0);
        // Read the entire two-sector page back into bank 0.
        nand.write(0x1e20e, 2, 63 << 2);
        nand.write(0x1e400, 2, 0x0800);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 2), 0xffff);
        assert_eq!(nand.read(0x600, 2), 0x5678);
        nand.write(0x1e200, 2, 512);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(STATUS, 2), 0x400);
        nand.write(0x1e200, 2, 511);
        nand.write(0x1e20e, 2, (63 << 2) | 1);
        nand.write(0x1e400, 2, 0x0902); // Two sectors starting at the last one.
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(STATUS, 2), 0x400);
    }

    #[test]
    fn partial_program_leaves_other_flash_sectors_erased() {
        let mut nand = OneNand::default();
        nand.window[0x800..0xc00].fill(0);
        // Page 3, sector 1; buffer 1, sector 1, count 1.
        nand.write(0x1e20e, 2, 0x0d);
        nand.write(0x1e400, 2, 0x0d01);
        nand.write(COMMAND, 2, 0x80);
        nand.write(0x1e20e, 2, 3 << 2);
        nand.write(0x1e400, 2, 0x0800);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 2), 0xffff);
        assert_eq!(nand.read(0x5fe, 2), 0xffff);
        assert_eq!(nand.read(0x600, 2), 0);
        assert_eq!(nand.read(0x7fe, 2), 0);
    }

    #[test]
    fn retail_logical_sector_encoding_reads_distinct_sectors_and_spare() {
        let mut nand = OneNand::default();
        let mut image = vec![0xff; (BLOCKS * BLOCK_PAGES) as usize * (PAGE_SIZE + SPARE_SIZE)];
        let page_start = 7 * (PAGE_SIZE + SPARE_SIZE);
        image[page_start..page_start + 4].copy_from_slice(b"MBR0");
        image[page_start + SECTOR_SIZE..page_start + SECTOR_SIZE + 4].copy_from_slice(b"FAT1");
        image[page_start + PAGE_SIZE..page_start + PAGE_SIZE + 2].copy_from_slice(&[0x11, 0x22]);
        image[page_start + PAGE_SIZE + SECTOR_SPARE_SIZE..page_start + PAGE_SIZE + SECTOR_SPARE_SIZE + 2]
            .copy_from_slice(&[0x33, 0x44]);
        nand.load_image(image).unwrap();
        for (logical, expected, spare) in [(14u64, b"MBR0", 0x2211), (15, b"FAT1", 0x4433)] {
            nand.write(0x1e20e, 2, ((logical << 1) & 0xfc) | (logical & 1));
            nand.write(0x1e400, 2, 0x0801 | ((logical & 1) << 8));
            nand.write(COMMAND, 2, 0);
            assert_eq!(nand.read(STATUS, 2), 0);
            assert_eq!(nand.read(0x400 + (logical as u32 & 1) * 512, 4),
                u64::from(u32::from_le_bytes(*expected)));
            assert_eq!(nand.read(0x10020 + (logical as u32 & 1) * 16, 2), spare);
        }
    }

    #[test]
    fn sector_read_preserves_unselected_buffer_data_and_spare_only_preserves_main_data() {
        let mut nand = OneNand::default();
        nand.write(0x400, 2, 0x1234);
        nand.write(0x600, 2, 0x5678);
        nand.write(0x1e20e, 2, 1);
        nand.write(0x1e400, 2, 0x0901);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 2), 0x1234);
        assert_eq!(nand.read(0x600, 2), 0xffff);
        nand.write(0x600, 2, 0x5678);
        nand.write(COMMAND, 2, 0x13);
        assert_eq!(nand.read(0x600, 2), 0x5678);
    }

    #[test]
    fn physical_interleaved_dump_preserves_page_spare_and_erase_overrides_input() {
        let mut nand = OneNand::default();
        let stride = PAGE_SIZE + SPARE_SIZE;
        let mut image = vec![0xff; (BLOCKS * BLOCK_PAGES) as usize * stride];
        let key = 498 * BLOCK_PAGES + 3;
        let start = key as usize * stride;
        image[start..start + 4].copy_from_slice(b"TEST");
        image[start + PAGE_SIZE..start + PAGE_SIZE + 2].copy_from_slice(&[0x12, 0x34]);
        nand.load_image(image).unwrap();
        nand.write(0x1e200, 2, 498);
        nand.write(0x1e20e, 2, 3 << 2);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 4), 0x54534554);
        assert_eq!(nand.read(0x10020, 2), 0x3412);
        nand.write(COMMAND, 2, 0x94);
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 4), u64::from(u32::MAX));
        assert_eq!(nand.read(0x10020, 2), 0xffff);
        assert_eq!(&nand.image[start..start + 4], b"TEST");
    }

    #[test]
    fn main_only_dump_has_erased_spare_and_invalid_size_keeps_loaded_image() {
        let mut nand = OneNand::default();
        let mut image = vec![0xff; (BLOCKS * BLOCK_PAGES) as usize * PAGE_SIZE];
        image[..2].copy_from_slice(&[0x12, 0x34]);
        nand.load_image(image).unwrap();
        assert_eq!(nand.load_image(vec![0; 100]), Err(100));
        nand.write(COMMAND, 2, 0);
        assert_eq!(nand.read(0x400, 2), 0x3412);
        assert_eq!(nand.read(0x10020, 2), 0xffff);
    }

    #[test]
    fn physical_export_roundtrips_program_erase_and_original_spare() {
        let mut nand = OneNand::default();
        let stride = PAGE_SIZE + SPARE_SIZE;
        let mut source = vec![0xff; (BLOCKS * BLOCK_PAGES) as usize * stride];
        source[..4].copy_from_slice(b"KEEP");
        source[PAGE_SIZE..PAGE_SIZE + 2].copy_from_slice(&[0x12, 0x34]);
        source[BLOCK_PAGES as usize * stride] = 0;
        nand.load_image(source).unwrap();
        nand.write(0x1e200, 2, 1);
        nand.write(COMMAND, 2, 0x94);
        nand.write(0x1e200, 2, u64::from(BLOCKS - 1));
        nand.write(0x1e20e, 2, ((BLOCK_PAGES - 1) << 2 | 1) as u64);
        nand.write(0x1e400, 2, 0x0901);
        nand.write(0x600, 4, 0x12345678);
        nand.write(0x10030, 2, 0xabcd);
        nand.write(COMMAND, 2, 0x80);
        let exported = nand.snapshot();
        assert_eq!(exported.len(), 34_603_008);
        assert!(exported[BLOCK_PAGES as usize * stride..2 * BLOCK_PAGES as usize * stride]
            .iter().all(|byte| *byte == 0xff));
        let mut reloaded = OneNand::default();
        reloaded.load_image(exported).unwrap();
        reloaded.write(COMMAND, 2, 0);
        assert_eq!(reloaded.read(0x400, 4), 0x5045454b);
        assert_eq!(reloaded.read(0x10020, 2), 0x3412);
        reloaded.write(0x1e200, 2, u64::from(BLOCKS - 1));
        reloaded.write(0x1e20e, 2, ((BLOCK_PAGES - 1) << 2 | 1) as u64);
        reloaded.write(0x1e400, 2, 0x0901);
        reloaded.write(COMMAND, 2, 0);
        assert_eq!(reloaded.read(0x600, 4), 0x12345678);
        assert_eq!(reloaded.read(0x10030, 2), 0xabcd);
        assert_eq!(&nand.image[..4], b"KEEP");
    }
}
