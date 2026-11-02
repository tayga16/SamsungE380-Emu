//! Experimental E830 SIM serial block (device 0x0c, IRQ30).
//! Observed reset/ATR receive and RAM-to-SIM TX paths. This is a test card,
//! not a network subscription. The automatic SELECT/GET RESPONSE/STATUS/READ BINARY DMA path
//! uses a small fictional card; other serial/controller modes remain TODO.
use std::collections::VecDeque;

const RESET_DONE: u32 = 1 << 5;
const RX_READY: u32 = 1;
const LATENCY: u64 = 4096; // provisional instruction slots, not measured ETUs
const ATR: [u8; 2] = [0x3b, 0x00]; // direct convention, default T=0, no history
const RESPONSE_LATENCY: u64 = 256; // provisional, not measured card ETUs

pub struct SimController {
    registers: [u32; 1024],
    rx: VecDeque<u8>,
    reset_deadline: Option<u64>,
    atr_deadline: Option<u64>,
    atr_pending: bool,
    powered: bool,
    command: Vec<u8>,
    response_deadline: Option<u64>,
    selected_file: u16,
    directory: u16,
    dma_response: Option<Vec<u8>>,
    receive_exchange: bool,
}

impl Default for SimController {
    fn default() -> Self {
        Self {
            registers: [0; 1024], rx: VecDeque::new(),
            reset_deadline: None, atr_deadline: None,
            atr_pending: false, powered: false,
            command: Vec::new(),
            response_deadline: None, selected_file: 0x3f00,
            directory: 0x3f00,
            dma_response: None, receive_exchange: false,
        }
    }
}

impl SimController {
    pub fn next_deadline(&self) -> Option<u64> {
        [self.reset_deadline, self.atr_deadline, self.response_deadline]
            .into_iter().flatten().min()
    }

    pub fn transmit_dma(&mut self, bytes: &[u8], now: u64) -> bool {
        // The original driver supplies the five APDU header bytes in
        // +0x808..+0x818, then starts automatic-header mode and TX at +0x0c.
        // Support only that observed mode, not arbitrary serial loopback.
        if !self.powered || self.registers[0x0c / 4] & 0x50 != 0x50
            || self.registers[0x800 / 4] & 1 == 0
            || bytes.len() != (self.registers[0x804 / 4] & 0x1ff) as usize
            || !self.command.is_empty()
        { return false; }
        self.command.clear();
        self.clear_response();
        for offset in [0x808, 0x80c, 0x810, 0x814, 0x818] {
            self.command.push(self.registers[offset / 4] as u8);
        }
        self.command.extend_from_slice(bytes);
        // DMA completion is not card completion. A separate delayed test-card
        // exchange supplies the procedure/status response before any SIM IRQ.
        self.response_deadline = Some(now.saturating_add(RESPONSE_LATENCY));
        true
    }

    // Automatic receive descriptors request data, not raw T=0 status bytes.
    // Status is latched in +0x820/+0x824 by the controller. DMA consumes the
    // data itself; it must not also expose it as a second PIO RX interrupt.
    pub fn receive_dma(&mut self, count: usize, now: u64) -> Option<Vec<u8>> {
        if !self.powered || self.registers[0x0c / 4] & 0xd0 != 0xd0
            || count != (self.registers[0x804 / 4] & 0x1ff) as usize
        { return None; }
        if let Some(data) = self.dma_response.as_ref() {
            if data.len() != count { return None; }
            return self.dma_response.take();
        }
        if self.response_deadline.is_none() && self.command.is_empty() && !self.receive_exchange {
            self.clear_response();
            for offset in [0x808, 0x80c, 0x810, 0x814, 0x818] {
                self.command.push(self.registers[offset / 4] as u8);
            }
            self.receive_exchange = true;
            self.response_deadline = Some(now.saturating_add(RESPONSE_LATENCY));
        }
        None
    }

    fn clear_response(&mut self) {
        self.dma_response = None;
        self.receive_exchange = false;
        for offset in [0x81c, 0x820, 0x824] { self.registers[offset / 4] = 0; }
    }

    fn directory_response(&self) -> Vec<u8> {
        // GSM 11.11 SELECT response: directory type, nine following bytes,
        // CHV1 disabled, initialized retry counters. No real subscriber data.
        let [hi, lo] = self.directory.to_be_bytes();
        vec![0, 0, 0, 0, hi, lo, if self.directory == 0x3f00 { 1 } else { 2 },
            0, 0, 0, 0, 0, 9, 0x80, if self.directory == 0x3f00 { 2 } else { 0 },
            if self.directory == 0x3f00 { 2 } else if self.directory == 0x7f20 { 11 } else { 0 },
            2, 0, 0x83, 0x8a, 0x83, 0x8a]
    }

    fn elementary_file(&self, file: u16) -> Option<Vec<u8>> {
        // Fictional GSM test profile, MCC 001 / MNC 01. No authentication key,
        // real subscriber identity, contacts, SMS or network access is supplied.
        match (self.directory, file) {
            (0x3f00, 0x2fe2) => Some(vec![0x98, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf1]),
            (0x3f00, 0x2f05) => Some(b"en".to_vec()),
            (0x7f20, 0x6f05) => Some(vec![1, 0xff]), // language: English
            (0x7f20, 0x6f07) => Some(vec![8, 0x09, 0x10, 0x10, 0, 0, 0, 0, 0x10]),
            (0x7f20, 0x6f20) => Some(vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 7]),
            (0x7f20, 0x6f30) => Some(vec![0xff; 24]), // no preferred networks
            (0x7f20, 0x6f31) => Some(vec![10]),
            (0x7f20, 0x6f38) => Some(vec![0; 16]), // optional services not allocated
            (0x7f20, 0x6f78) => Some(vec![0, 1]), // access class 0
            (0x7f20, 0x6f7b) => Some(vec![0xff; 12]), // no forbidden networks
            (0x7f20, 0x6f7e) => Some(vec![0xff, 0xff, 0xff, 0xff, 0x00, 0xf1, 0x10, 0xff, 0xff, 0xff, 1]),
            (0x7f20, 0x6fad) => Some(vec![0, 0, 0, 2]),
            (0x7f20, 0x6fae) => Some(vec![2]), // phase 2
            _ => None,
        }
    }

    fn select_response(&self) -> Vec<u8> {
        if let Some(data) = self.elementary_file(self.selected_file) {
            let [hi, lo] = self.selected_file.to_be_bytes();
            let [size_hi, size_lo] = (data.len() as u16).to_be_bytes();
            // GSM 11.11 transparent EF, always readable; writes/rehabilitation
            // unavailable. File status is valid and not invalidated.
            vec![0, 0, size_hi, size_lo, hi, lo, 4, 0, 0x0f, 0xff, 0xff, 1, 2, 0, 0]
        } else { self.directory_response() }
    }

    fn receive_status(&self) -> u32 {
        let threshold = (self.registers[0x30 / 4] & 7) as usize + 1;
        if self.rx.len() >= threshold { RX_READY } else { 0 }
    }

    pub fn irq_pending(&self) -> bool {
        (self.registers[0xfe0 / 4] | self.receive_status())
            & self.registers[0xfe4 / 4] != 0
    }

    pub fn read(&self, offset: u32, size: usize) -> u64 {
        let aligned = offset & !3;
        let value = match aligned {
            0x30 => (self.registers[0x30 / 4] & !0x60) | 0x20
                | if self.rx.is_empty() { 0x40 } else { 0 },
            0x34 => u32::from(self.rx.front().copied().unwrap_or(0)),
            0xfe0 => self.registers[0xfe0 / 4] | self.receive_status(),
            _ => self.registers[(aligned / 4) as usize],
        };
        u64::from((value >> ((offset & 3) * 8)) & width_mask(size))
    }

    pub fn write(&mut self, offset: u32, size: usize, value: u64, now: u64) {
        let aligned = offset & !3;
        let shift = (offset & 3) * 8;
        let mask = width_mask(size) << shift;
        let written = ((value as u32) << shift) & mask;
        let index = (aligned / 4) as usize;
        let previous = self.registers[index];
        match aligned {
            // Original ISR 0x200ea654 clears interrupt latches through +0xfe8
            // and mirrored core flags through +0. These are W1C, not storage.
            0 => self.registers[0] &= !written,
            0xfe0 => {},
            0xfe8 => self.registers[0xfe0 / 4] &= !written,
            // RX byte is latched until the original driver's +0x40 ACK.
            0x40 => { if written & 1 != 0 { self.rx.pop_front(); } },
            // +0x34 writes are TX data; do not echo them into the RX FIFO.
            // The reset routine writes one before releasing card reset.
            0x34 => {},
            0x30 => self.registers[index] = (previous & !mask) | (written & !0x60),
            _ => self.registers[index] = (previous & !mask) | written,
        }
        if aligned == 0x20 {
            let current = self.registers[index];
            // Reset routine 0x2154bb1c selects timer action 2 (bits 3:2),
            // then IRQ handler changes it to action 3. Do not repeat completion.
            if self.powered && current & 0x0c == 8 && previous & 0x0c != 8 {
                self.rx.clear();
                self.atr_pending = false;
                self.atr_deadline = None;
                self.command.clear();
                self.response_deadline = None;
                self.selected_file = 0x3f00;
                self.directory = 0x3f00;
                self.clear_response();
                self.reset_deadline = Some(now.saturating_add(LATENCY));
            }
        }
        if aligned == 0x0c && self.atr_pending
            && self.registers[index] & 0x80 != 0 && self.atr_deadline.is_none()
        {
            self.atr_deadline = Some(now.saturating_add(LATENCY));
        }
        if aligned == 0x0c && self.registers[index] & 0x80 == 0 {
            self.atr_deadline = None;
        }
        if aligned == 0x40 && written & 2 != 0 {
            self.command.clear();
            self.response_deadline = None;
            self.clear_response();
        }
    }

    pub fn advance(&mut self, now: u64, powered: bool) {
        self.powered = powered;
        if !powered {
            self.rx.clear();
            self.reset_deadline = None;
            self.atr_deadline = None;
            self.atr_pending = false;
            self.command.clear();
            self.response_deadline = None;
            self.selected_file = 0x3f00;
            self.directory = 0x3f00;
            self.clear_response();
            self.registers[0xfe0 / 4] = 0;
            self.registers[0] = 0;
            return;
        }
        if self.reset_deadline.is_some_and(|deadline| now >= deadline) {
            self.reset_deadline = None;
            self.registers[0xfe0 / 4] |= RESET_DONE;
            self.registers[0] |= 0x100;
            self.atr_pending = true;
        }
        if self.atr_deadline.is_some_and(|deadline| now >= deadline) {
            self.atr_deadline = None;
            self.atr_pending = false;
            self.rx.extend(ATR);
        }
        if self.response_deadline.is_some_and(|deadline| now >= deadline) {
            self.response_deadline = None;
            let mut data = None;
            let (sw1, sw2) = match self.command.as_slice() {
                [0xa0, 0xa4, 0, 0, 2, hi, lo] => {
                    let file = u16::from_be_bytes([*hi, *lo]);
                    if matches!(file, 0x3f00 | 0x7f20 | 0x7f10)
                        || self.elementary_file(file).is_some() {
                        self.selected_file = file;
                        if matches!(file, 0x3f00 | 0x7f20 | 0x7f10) { self.directory = file; }
                        (0x9f, self.select_response().len() as u32)
                    } else { (0x94, 0x04) } // file not found
                }
                [0xa0, instruction @ (0xc0 | 0xf2), 0, 0, length] if self.receive_exchange => {
                    let response = if *instruction == 0xf2 { self.directory_response() }
                        else { self.select_response() };
                    if usize::from(*length) <= response.len() && *length != 0 {
                        data = Some(response[..usize::from(*length)].to_vec());
                        (0x90, 0)
                    } else { (0x67, 0) }
                }
                [0xa0, 0xb0, hi, lo, length] if self.receive_exchange => {
                    if let Some(contents) = self.elementary_file(self.selected_file) {
                        let offset = usize::from(u16::from_be_bytes([*hi, *lo]));
                        let end = offset + usize::from(*length);
                        if *length != 0 && end <= contents.len() {
                            data = Some(contents[offset..end].to_vec());
                            (0x90, 0)
                        } else { (0x94, 0x02) }
                    } else { (0x94, 0x00) }
                }
                _ => (0x6d, 0x00), // unsupported instruction, never success
            };
            self.registers[0x820 / 4] = sw1;
            self.registers[0x824 / 4] = sw2;
            self.registers[0x81c / 4] = data.as_ref().map_or(
                if self.receive_exchange { 0 } else { (self.command.len() - 5) as u32 },
                |bytes| bytes.len() as u32);
            self.dma_response = data;
            // The observed DMA route completes its queued TX descriptor via
            // IRQ29. IRQ11 would deliver that same descriptor a second time,
            // after the guest unlinks it. Keep this route's status passive.
            self.command.clear();
        }
    }
}

fn width_mask(size: usize) -> u32 {
    match size { 1 => 0xff, 2 => 0xffff, _ => u32::MAX }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_completion_is_delayed_masked_and_w1c() {
        let mut sim = SimController::default();
        sim.advance(0, true);
        sim.write(0x20, 1, 9, 10);
        sim.advance(4105, true);
        assert_eq!(sim.read(0xfe0, 2), 0);
        sim.advance(4106, true);
        assert_eq!(sim.read(0xfe0, 2), 0x20);
        assert!(!sim.irq_pending());
        sim.write(0xfe4, 2, 0x20, 4106);
        assert!(sim.irq_pending());
        sim.write(0xfe8, 1, 0x20, 4106);
        assert!(!sim.irq_pending());
        sim.write(0x20, 1, 9, 4107);
        sim.advance(99999, true);
        assert!(!sim.irq_pending());
    }

    #[test]
    fn atr_uses_latched_fifo_and_guest_acknowledgement() {
        let mut sim = SimController::default();
        sim.advance(0, true);
        sim.write(0x34, 1, 1, 0); // reset write is not a received byte
        assert_eq!(sim.read(0x30, 1) & 0x40, 0x40);
        sim.write(0x20, 1, 9, 0);
        sim.advance(LATENCY, true);
        sim.write(0xfe8, 2, RESET_DONE as u64, LATENCY);
        sim.write(0xfe4, 2, 1, LATENCY);
        sim.write(0x30, 1, 0x31, LATENCY); // threshold two bytes
        sim.advance(2 * LATENCY, true);
        assert_eq!(sim.read(0x34, 1), 0); // receiver still disabled
        sim.write(0x0c, 1, 0xa0, 2 * LATENCY);
        sim.advance(3 * LATENCY, true);
        assert!(sim.irq_pending());
        assert_eq!(sim.read(0x34, 1), 0x3b);
        assert_eq!(sim.read(0x34, 1), 0x3b); // no diagnostic read side effect
        sim.write(0xfe8, 2, 1, 3 * LATENCY);
        assert!(sim.irq_pending()); // device data is not consumed by IRQ ACK
        sim.write(0x40, 1, 1, 3 * LATENCY);
        assert!(!sim.irq_pending());
        assert_eq!(sim.read(0x34, 1), 0);
        assert_eq!(sim.read(0x30, 1) & 0x40, 0);
        sim.write(0x40, 1, 1, 3 * LATENCY);
        assert_eq!(sim.read(0x30, 1) & 0x40, 0x40);
        sim.advance(99999, true);
        assert!(!sim.irq_pending());
    }

    #[test]
    fn automatic_select_waits_for_card_response_before_completion() {
        let mut sim = SimController::default();
        sim.advance(0, true);
        sim.write(0x0c, 1, 0xf0, 0);
        sim.write(0x800, 1, 3, 0);
        sim.write(0x804, 2, 2, 0);
        for (offset, byte) in [(0x808, 0xa0), (0x80c, 0xa4), (0x818, 2)] {
            sim.write(offset, 1, byte, 0);
        }
        assert!(!sim.transmit_dma(&[0x7f], 0));
        assert!(sim.transmit_dma(&[0x7f, 0x20], 0));
        assert_eq!(sim.command, [0xa0, 0xa4, 0, 0, 2, 0x7f, 0x20]);
        assert!(!sim.transmit_dma(&[0x7f, 0x20], 0));
        sim.write(0xfe4, 2, 0xffff, 0);
        sim.advance(RESPONSE_LATENCY - 1, true);
        assert_eq!(sim.read(0xfe0, 2), 0);
        assert_eq!(sim.read(0x81c, 2), 0);
        assert!(!sim.irq_pending());
        assert!(sim.rx.is_empty());
        sim.advance(RESPONSE_LATENCY, true);
        assert_eq!(sim.read(0x820, 2), 0x9f);
        assert_eq!(sim.read(0x824, 2), 22);
        assert_eq!(sim.read(0x81c, 2), 2);
        assert!(!sim.irq_pending());
        assert_eq!(sim.selected_file, 0x7f20);
        sim.advance(1_000_001, false);
        assert!(sim.command.is_empty());
        assert!(!sim.transmit_dma(&[0x7f, 0x20], 1_000_001));
    }

    #[test]
    fn receiver_disable_cancels_pending_atr_until_reenabled() {
        let mut sim = SimController::default();
        sim.advance(0, true);
        sim.write(0x20, 1, 9, 0);
        sim.advance(LATENCY, true);
        sim.write(0x0c, 1, 0x80, LATENCY);
        sim.write(0x0c, 1, 0, LATENCY + 1);
        sim.advance(3 * LATENCY, true);
        assert!(sim.rx.is_empty());
        sim.write(0x0c, 1, 0x80, 3 * LATENCY);
        sim.advance(4 * LATENCY, true);
        assert_eq!(sim.read(0x34, 1), 0x3b);
    }

    #[test]
    fn absent_or_unpowered_card_never_answers_reset() {
        let mut sim = SimController::default();
        sim.write(0x20, 1, 9, 0);
        sim.write(0x0c, 1, 0x80, 0);
        sim.advance(99999, false);
        assert_eq!(sim.read(0xfe0, 2), 0);
        sim.advance(100000, true);
        sim.write(0x20, 1, 0, 100000);
        sim.write(0x20, 1, 9, 100001);
        sim.advance(100002, false);
        sim.advance(999999, true);
        assert_eq!(sim.read(0xfe0, 2), 0);
        assert_eq!(sim.read(0x34, 1), 0);
    }

    fn automatic_card() -> SimController {
        let mut sim = SimController::default();
        sim.advance(0, true);
        sim.write(0x0c, 1, 0xf0, 0);
        sim.write(0x800, 1, 3, 0);
        sim
    }

    fn header(sim: &mut SimController, bytes: [u8; 5], count: u32, now: u64) {
        sim.write(0x40, 1, 2, now);
        sim.write(0x804, 2, u64::from(count), now);
        for (offset, byte) in [0x808, 0x80c, 0x810, 0x814, 0x818].into_iter().zip(bytes) {
            sim.write(offset, 1, u64::from(byte), now);
        }
    }

    #[test]
    fn get_response_is_delayed_single_consumption_and_has_gsm_field_offsets() {
        let mut sim = automatic_card();
        header(&mut sim, [0xa0, 0xa4, 0, 0, 2], 2, 0);
        assert!(sim.transmit_dma(&[0x7f, 0x20], 0));
        sim.advance(RESPONSE_LATENCY, true);
        assert_eq!(sim.read(0x820, 1), 0x9f);
        header(&mut sim, [0xa0, 0xc0, 0, 0, 22], 22, 1000);
        assert_eq!(sim.read(0x820, 1), 0); // stale SELECT status cleared
        sim.write(0x800, 1, 2, 1000); // header already sent by original driver
        assert_eq!(sim.receive_dma(22, 1000), None);
        sim.advance(1000 + RESPONSE_LATENCY - 1, true);
        assert_eq!(sim.receive_dma(22, 1255), None);
        sim.advance(1000 + RESPONSE_LATENCY, true);
        assert_eq!(sim.receive_dma(21, 1256), None); // must not consume
        let response = sim.receive_dma(22, 1256).unwrap();
        assert_eq!(response.len(), 22);
        assert_eq!(&response[4..7], &[0x7f, 0x20, 2]);
        assert_eq!(response[12], 9);
        assert_eq!(response[13], 0x80); // CHV1 disabled
        assert_eq!(&response[18..22], &[0x83, 0x8a, 0x83, 0x8a]);
        assert_eq!(sim.read(0x81c, 2), 22);
        assert_eq!(sim.read(0x820, 1), 0x90);
        assert_eq!(sim.receive_dma(22, 1257), None); // no repeat exchange
        assert!(sim.rx.is_empty());
        assert!(!sim.irq_pending());
    }

    #[test]
    fn unknown_file_and_instruction_do_not_report_success() {
        let mut sim = automatic_card();
        header(&mut sim, [0xa0, 0xa4, 0, 0, 2], 2, 0);
        assert!(sim.transmit_dma(&[0x6f, 0xff], 0));
        sim.advance(RESPONSE_LATENCY, true);
        assert_eq!(sim.selected_file, 0x3f00);
        assert_eq!(sim.read(0x820, 1), 0x94);
        assert_eq!(sim.read(0x824, 1), 4);
        header(&mut sim, [0xa0, 0xff, 0, 0, 2], 2, 1000);
        assert!(sim.transmit_dma(&[0, 0], 1000));
        sim.advance(1000 + RESPONSE_LATENCY, true);
        assert_eq!(sim.read(0x820, 1), 0x6d);
    }

    #[test]
    fn reset_poweroff_and_tx_flush_cancel_responses_and_clear_stale_status() {
        for action in 0..3 {
            let mut sim = automatic_card();
            header(&mut sim, [0xa0, 0xc0, 0, 0, 22], 22, 0);
            assert_eq!(sim.receive_dma(22, 0), None);
            match action {
                0 => sim.advance(1, false),
                1 => sim.write(0x20, 1, 9, 1),
                _ => sim.write(0x40, 1, 2, 1),
            }
            sim.advance(RESPONSE_LATENCY, action != 0);
            assert!(sim.dma_response.is_none());
            assert_eq!(sim.read(0x81c, 2), 0);
            assert_eq!(sim.read(0x820, 1), 0);
        }
    }

    #[test]
    fn fictional_files_have_consistent_directory_counts_sizes_and_access_rules() {
        let mut sim = automatic_card();
        for (directory, files) in [(0x3f00, vec![0x2fe2, 0x2f05]),
            (0x7f20, vec![0x6f05, 0x6f07, 0x6f20, 0x6f30, 0x6f31,
                0x6f38, 0x6f78, 0x6f7b, 0x6f7e, 0x6fad, 0x6fae]),
            (0x7f10, vec![])] {
            sim.directory = directory;
            sim.selected_file = directory;
            assert_eq!(usize::from(sim.directory_response()[15]), files.len());
            for file in files {
                sim.selected_file = file;
                let contents = sim.elementary_file(file).unwrap();
                let response = sim.select_response();
                assert_eq!(response.len(), 15);
                assert_eq!(u16::from_be_bytes([response[2], response[3]]) as usize, contents.len());
                assert_eq!(&response[4..7], &[file.to_be_bytes()[0], file.to_be_bytes()[1], 4]);
                assert_eq!(&response[8..15], &[0x0f, 0xff, 0xff, 1, 2, 0, 0]);
            }
        }
        assert!(sim.elementary_file(0x6f07).is_none()); // wrong parent
    }

    #[test]
    fn select_read_binary_status_and_missing_files_follow_card_state() {
        let mut sim = automatic_card();
        header(&mut sim, [0xa0, 0xa4, 0, 0, 2], 2, 0);
        assert!(sim.transmit_dma(&[0x2f, 0x05], 0));
        sim.advance(256, true);
        assert_eq!(sim.read(0x824, 1), 15);
        header(&mut sim, [0xa0, 0xc0, 0, 0, 15], 15, 1000);
        assert_eq!(sim.receive_dma(15, 1000), None);
        sim.advance(1256, true);
        assert_eq!(sim.receive_dma(15, 1256).unwrap(), sim.select_response());
        header(&mut sim, [0xa0, 0xb0, 0, 0, 2], 2, 2000);
        assert_eq!(sim.receive_dma(2, 2000), None);
        sim.advance(2256, true);
        assert_eq!(sim.receive_dma(2, 2256).unwrap(), b"en");
        header(&mut sim, [0xa0, 0xf2, 0, 0, 22], 22, 3000);
        assert_eq!(sim.receive_dma(22, 3000), None);
        sim.advance(3256, true);
        assert_eq!(sim.receive_dma(22, 3256).unwrap(), sim.directory_response());
        header(&mut sim, [0xa0, 0xb0, 0, 1, 2], 2, 4000);
        assert_eq!(sim.receive_dma(2, 4000), None);
        sim.advance(4256, true);
        assert_eq!(sim.read(0x820, 1), 0x94);
        assert_eq!(sim.read(0x824, 1), 2);
        assert_eq!(sim.read(0x81c, 2), 0);
        assert_eq!(sim.receive_dma(2, 4256), None); // no padding on error
        header(&mut sim, [0xa0, 0xa4, 0, 0, 2], 2, 5000);
        assert!(sim.transmit_dma(&[0x6f, 0x07], 5000));
        sim.advance(5256, true);
        assert_eq!(sim.read(0x820, 1), 0x94); // absent from MF
        assert_eq!(sim.selected_file, 0x2f05);
        header(&mut sim, [0xa0, 0xa4, 0, 0, 2], 2, 6000);
        assert!(sim.transmit_dma(&[0x7f, 0x20], 6000));
        sim.advance(6256, true);
        header(&mut sim, [0xa0, 0xa4, 0, 0, 2], 2, 7000);
        assert!(sim.transmit_dma(&[0x6f, 0x07], 7000));
        sim.advance(7256, true);
        assert_eq!(sim.selected_file, 0x6f07);
        assert_eq!(sim.read(0x820, 1), 0x9f);
    }
}
