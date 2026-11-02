//! Experimental RF-measurement HLE, not an RD16024 interpreter.
//!
//! Request packing and slot/result indexing come from the original ARM code.
//! The timer-frame origin, one-frame pipeline, integer units and empty-air raw
//! level are MODEL ASSUMPTIONS, not recovered DSP/hardware behavior. Enable
//! only with E830_DSP_RF_HLE=empty plus the exact-image boot HLE.

pub(crate) const SNAPSHOT_BASE: u64 = 0x6000_0658;
pub(crate) const SNAPSHOT_SIZE: usize = 0x1a4; // mask, 16 records, eight 40-byte commands
pub(crate) const RESULT_BASE: u64 = 0x6000_00dc;
const COMMAND_OFFSET: usize = 0x64;
const FRAME_UNITS: u64 = 5000;
// Synthetic empty air: observed ARM corrections -140 (0335) and -139
// (0333/channel512, including traced band8/tag set), -160 (033f/band2), -159 (033d/band4),
// -159 also verified for tagged033d/band8; -163 (033f/band4 and tagged band8)
// give -240/-239/-260/-259/-263 half-dB
// before clamp to -220 / RXLEV 0.
// Not zero (-70 dBm), not an ARM unavailable sentinel, not a proven DSP unit.
const EMPTY_AIR_RAW: i16 = -400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Measurement {
    slot: u8,
    channel: u16,
    calibration: u16,
    finish: u16,
}

fn halfword(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

#[cfg(test)]
fn decode(bytes: &[u8]) -> Vec<Measurement> { decode_with_residue(bytes, None) }

fn decode_with_residue(bytes: &[u8], previous_command: Option<&[u8; 8]>) -> Vec<Measurement> {
    if bytes.len() != SNAPSHOT_SIZE { return Vec::new(); }
    let mask = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    if mask == 0 || mask >> 16 != 0 { return Vec::new(); }
    let mut records: [Vec<(u8, u8, u16, u16)>; 8] = Default::default();
    for index in 0..16 {
        if mask & (1 << index) == 0 { continue; }
        let offset = 4 + 6 * index;
        let header = halfword(bytes, offset);
        records[(header >> 13) as usize].push(((header >> 8) as u8 & 0x1f,
            header as u8, halfword(bytes, offset + 2), halfword(bytes, offset + 4)));
    }
    records.iter().enumerate().filter_map(|(slot, records)| {
        // Do not complete partial, duplicated, reordered or mixed operations.
        if records.len() != 2 { return None; }
        let (class0, op0, start, arg0) = records[0];
        let (class1, op1, finish, arg1) = records[1];
        if (class0, op0, class1, op1, arg0, arg1) != (5, 0, 2, 10, 0, 0)
            || finish <= start || finish >= 5000 { return None; }
        let offset = COMMAND_OFFSET + slot * 40;
        let first = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let second = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        if first & 0x7ff != 0 || second & 0x7ff != 0 { return None; }
        let (first, second) = (first >> 11, second >> 11);
        // Original packer sets bit16 for band8/16. Tagged0333/033d/033f are
        // verified by original inputs/packer/correction; other tags reject.
        let tagged = first & 0x10000 != 0;
        if first & 0x3f != 0x21 || first >> 17 != 0
            || second & 0x3f != 0x20 || second >> 16 != 0
            || !matches!(second >> 6, 0x335 | 0x333 | 0x33f | 0x33d) // observed gain profiles only
            || (tagged && !matches!(second >> 6, 0x333 | 0x33d | 0x33f))
        { return None; }
        let channel = (first >> 6) as u16 & 0x3ff;
        let extension = &bytes[offset + 8..offset + 40];
        // Original 2098d854 writes eight bytes of reused ARM scratch;
        // 20143dea copies forty. Restarts at 279.8M and 288M retain the
        // preceding accepted slot3/channel522 or707 pair at +8. Validate
        // against OUR previous accepted command, not arbitrary guest bytes.
        // Same0333/033d/033f restarts and captured885/0333-or-033d-or-033f
        // ->0..4/033f scan-range transitions qualify.
        // Captured885/033f/tag-clear ->512..516/0333/tag-set also qualifies.
        // Captured810/0333 ->512..516/033d and810/033d ->512..516/033f,
        // with both tags set, qualify only
        // with the accepted four-request tail's exact eight-byte history.
        // Captured443366255 wraps tagged810/033f to512..516/033f;
        // same-profile wraps for other profiles/ranges remain unsupported.
        // Captured908166322 drops the tag and wraps tagged810/033f to
        // clear0..4/0335 after opening the application menu.
        // No arbitrary profile/channel/tag transition.
        // This residue is NOT another scheduled measurement.
        let observed_restart_residue = previous_command.is_some_and(|previous| {
            let previous_first = u32::from_le_bytes(previous[..4].try_into().unwrap()) >> 11;
            let previous_channel = ((previous_first >> 6) & 0x3ff) as u16;
            let previous_tagged = previous_first & 0x10000 != 0;
            let previous_profile = u32::from_le_bytes(previous[4..].try_into().unwrap()) >> 17;
            let same_profile_restart = channel > previous_channel && channel - previous_channel <= 5
                && matches!(second >> 6, 0x333 | 0x33d | 0x33f) && previous_profile == second >> 6
                && tagged == previous_tagged;
            let observed_band_transition = !tagged && !previous_tagged
                && previous_channel == 885 && matches!(previous_profile, 0x333 | 0x33d | 0x33f)
                && channel <= 4 && second >> 6 == 0x33f;
            let observed_tagged_transition = !previous_tagged && tagged && previous_channel == 885
                && previous_profile == 0x33f && (512..=516).contains(&channel) && second >> 6 == 0x333;
            let observed_tagged_profile_transition = previous_tagged && tagged && previous_channel == 810
                && (512..=516).contains(&channel)
                && matches!((previous_profile, second >> 6), (0x333, 0x33d) | (0x33d, 0x33f));
            let observed_tagged033f_wrap = previous_tagged && tagged && previous_channel == 810
                && previous_profile == 0x33f && second >> 6 == 0x33f && (512..=516).contains(&channel);
            let observed_menu_band_transition = previous_tagged && !tagged && previous_channel == 810
                && previous_profile == 0x33f && second >> 6 == 0x335 && channel <= 4;
            (same_profile_restart || observed_band_transition || observed_tagged_transition
                || observed_tagged_profile_transition || observed_tagged033f_wrap
                || observed_menu_band_transition)
                && extension[..8] == previous[..]
                && extension[8..].iter().all(|byte| *byte == 0)
        });
        if extension.iter().any(|byte| *byte != 0) && !observed_restart_residue {
            return None;
        }
        Some(Measurement { slot: slot as u8, channel,
            calibration: (second >> 6) as u16, finish })
    }).collect()
}

fn observed_tagged_scan_tail(bytes: &[u8], requests: &[Measurement], previous: Option<&[u8; 8]>) -> bool {
    // Original405768363/422088363/438569060: slots7..4, tagged807..810,
    // profiles0333/033d/033f respectively, after accepted802..806 of the SAME
    // profile. Exact zero extensions and record timing; not generic count4.
    if requests.len() != 4 || bytes[..4] != 0xffu32.to_le_bytes() { return false; }
    let profile = requests[0].calibration;
    if !matches!(profile, 0x333 | 0x33d | 0x33f) { return false; }
    let mut expected_previous = [0x00, 0x08, 0x4d, 0x0e, 0x00, 0x00, 0x00, 0x00];
    expected_previous[4..].copy_from_slice(&((0x20u32 | u32::from(profile) << 6) << 11).to_le_bytes());
    if previous != Some(&expected_previous) { return false; }
    requests.iter().enumerate().all(|(index, request)| {
        let channel = 810 - index as u16;
        let command = COMMAND_OFFSET + (4 + index) * 40;
        let record = 4 + (3 - index) * 12;
        request.slot == 4 + index as u8 && request.channel == channel && request.calibration == profile
            && bytes[command..command + 4] == ((0x10021u32 | u32::from(channel) << 6) << 11).to_le_bytes()
            && bytes[command + 8..command + 40].iter().all(|byte| *byte == 0)
            && halfword(bytes, record + 2) == 37 + (3 - index as u16) * 910
            && request.finish == 536 + (3 - index as u16) * 910
    })
}

#[derive(Default)]
pub(crate) struct RfHle {
    pub(crate) enabled: bool,
    generation: Option<Vec<u8>>,
    pending: Vec<(Measurement, u64)>,
    previous_command: Option<[u8; 8]>,
}

impl RfHle {
    pub(crate) fn new(enabled: bool) -> Self { Self { enabled, ..Default::default() } }
    pub(crate) fn busy(&self) -> bool { !self.pending.is_empty() }
    pub(crate) fn next_deadline(&self) -> Option<u64> {
        self.pending.iter().map(|(_, deadline)| *deadline).min()
    }

    // The ARM clears/reuses the producer table every frame, but its four-bank
    // descriptor queue consumes a submitted group one frame later (20584d64,
    // 2099a9e0). Release the producer generation, not accepted DSP work.
    pub(crate) fn clear(&mut self) {
        self.generation = None;
    }

    pub(crate) fn reset(&mut self) {
        self.clear();
        self.pending.clear();
        self.previous_command = None;
    }

    pub(crate) fn submit(&mut self, bytes: Vec<u8>, now: u64, phase_clocks: u64, units: u64) -> usize {
        if !self.enabled { return 0; }
        // An idle control cycle is not a producer generation. The driver can
        // restart packing after idle without first clearing an already-zero mask.
        if bytes.len() != SNAPSHOT_SIZE || bytes[..4] == [0; 4] { return 0; }
        let mask = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let requests = decode_with_residue(&bytes, self.previous_command.as_ref());
        let count = requests.len();
        // Ordinarily a guest mask clear opens another nonempty generation;
        // changed commands plus a control re-write are not a second submission.
        // The captured post-Menu0..4/0335 generation is the one exception: it
        // replaces the completed807..810/033f producer table without a zero-mask
        // cycle. Its decoder already requires that exact accepted history and
        // all32 extension bytes, and pending work must have fully drained.
        let observed_menu_reuse = self.generation.as_ref().is_some_and(|generation|
                generation[..4] == 0xffu32.to_le_bytes())
            && self.pending.is_empty()
            && requests.len() == 5
            && requests.iter().zip((0u16..=4).rev()).all(|(request, channel)|
                request.slot == 7 - channel as u8 && request.channel == channel
                    && request.calibration == 0x335);
        if std::env::var_os("E830_TRACE_RF_REJECT").is_some() && count != 5 {
            eprintln!("[e830-dsp-rf-hle] reject mask=0x{mask:08x} decoded={count} generation_mask={:?} pending={} previous={:02x?}",
                self.generation.as_ref().map(|generation| u32::from_le_bytes(generation[..4].try_into().unwrap())),
                self.pending.len(), self.previous_command);
        }
        if self.generation.is_some() && !observed_menu_reuse { return 0; }
        // Cache only the final slot of a fully recognized observed producer
        // batch: five requests, its three-request scan tail at293.6M, or
        // the separately verified tagged807..810/0333-or-033d-or-033f tail.
        // Every active record must be accounted for; unsupported partial
        // decoding must not replace history. Keep across clear;
        // the original scratch buffer also survives producer-table reuse.
        let supported_history = matches!(count, 3 | 5)
            || observed_tagged_scan_tail(&bytes, &requests, self.previous_command.as_ref());
        if supported_history && mask == (1u32 << (2 * count)) - 1
            && requests.iter().map(|request| request.slot).eq((8 - count as u8)..=7) {
            let offset = COMMAND_OFFSET + (8 - count) * 40;
            self.previous_command = Some(bytes[offset..offset + 8].try_into().unwrap());
        }
        self.pending.extend(requests.into_iter().map(|request| {
            // Snapshot once at submission, then execute in the NEXT timer
            // frame. This is an explicit HLE pipeline assumption constrained
            // by the original descriptor producer/consumer, not a DSP ISA fact.
            // Never re-read stale commands when the guest submits an empty mask.
            let remaining = (FRAME_UNITS + u64::from(request.finish)).saturating_mul(units)
                .saturating_sub(phase_clocks);
            let deadline = now.saturating_add(remaining);
            (request, deadline)
        }));
        self.generation = Some(bytes);
        count
    }

    // Publication remains the board's responsibility: payload first, then flag.
    pub(crate) fn take_ready(&mut self, now: u64) -> Vec<(u8, i16)> {
        let mut results = Vec::new();
        self.pending.retain(|(request, deadline)| {
            if now < *deadline { return true; }
            results.push((request.slot, EMPTY_AIR_RAW));
            false
        });
        results
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    fn put(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    pub(crate) fn schedule() -> Vec<u8> {
        let mut bytes = vec![0; SNAPSHOT_SIZE];
        put(&mut bytes, 0, 0x3ff);
        for index in 0..5 {
            let slot = 7 - index;
            let offset = 4 + index * 12;
            put(&mut bytes, offset, (slot as u16) << 13 | 0x500);
            put(&mut bytes, offset + 2, 37 + index as u16 * 910);
            put(&mut bytes, offset + 6, (slot as u16) << 13 | 0x20a);
            put(&mut bytes, offset + 8, 536 + index as u16 * 910);
            let command = COMMAND_OFFSET + slot * 40;
            let first = (0x21u32 | (index as u32) << 6) << 11;
            let second = (0x20u32 | 0x335 << 6) << 11;
            bytes[command..command + 4].copy_from_slice(&first.to_le_bytes());
            bytes[command + 4..command + 8].copy_from_slice(&second.to_le_bytes());
        }
        bytes
    }
    pub(crate) fn profile0333_schedule() -> Vec<u8> {
        let mut bytes = schedule();
        for (index, channel) in [1020u32, 1021, 1022, 1023, 512].into_iter().enumerate() {
            let command = COMMAND_OFFSET + (7 - index) * 40;
            let first = (0x21 | channel << 6) << 11;
            bytes[command..command + 4].copy_from_slice(&first.to_le_bytes());
        }
        // Exact active slot3 words from rf-profile0333-gain; same ordered
        // operation pair and clear band tag. Calibration is not a band ID.
        let command = COMMAND_OFFSET + 3 * 40;
        bytes[command + 4..command + 8].copy_from_slice(&0x06670000u32.to_le_bytes());
        bytes
    }
    fn restart_schedule() -> Vec<u8> {
        let mut bytes = schedule();
        for index in 0..5 {
            let offset = COMMAND_OFFSET + (7 - index) * 40;
            let first = (0x21u32 | (523 + index as u32) << 6) << 11;
            bytes[offset..offset + 4].copy_from_slice(&first.to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&0x06670000u32.to_le_bytes());
            bytes[offset + 8..offset + 12].copy_from_slice(&0x04150800u32.to_le_bytes());
            bytes[offset + 12..offset + 16].copy_from_slice(&0x06670000u32.to_le_bytes());
        }
        bytes
    }
    fn scan_tail_schedule() -> Vec<u8> {
        let mut bytes = schedule();
        bytes[..4].copy_from_slice(&0x3fu32.to_le_bytes());
        for index in 0..3 {
            let offset = COMMAND_OFFSET + (7 - index) * 40;
            let first = (0x21u32 | (883 + index as u32) << 6) << 11;
            bytes[offset..offset + 4].copy_from_slice(&first.to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&0x06670000u32.to_le_bytes());
        }
        bytes
    }
    fn tagged0333_schedule(first_channel: u32, residue: Option<[u8; 8]>) -> Vec<u8> {
        let mut bytes = schedule();
        for index in 0..5 {
            let offset = COMMAND_OFFSET + (7 - index) * 40;
            bytes[offset..offset + 4].copy_from_slice(
                &((0x10021u32 | (first_channel + index as u32) << 6) << 11).to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&0x06670000u32.to_le_bytes());
            if let Some(previous) = residue {
                bytes[offset + 8..offset + 16].copy_from_slice(&previous);
            }
        }
        bytes
    }
    #[test]
    fn captured_tagged0333_zero_extension_preserves_level_and_timing() {
        let bytes = tagged0333_schedule(517, None);
        let requests = decode(&bytes);
        assert_eq!(requests.iter().map(|request| request.channel).collect::<Vec<_>>(),
            vec![521, 520, 519, 518, 517]);
        assert!(requests.iter().all(|request| request.calibration == 0x333));
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(bytes, 394_245_410, 11554, 32), 5);
        assert!(hle.take_ready(394_411_007).is_empty());
        assert_eq!(hle.take_ready(394_411_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(394_527_488).len(), 4);
        assert!(!hle.busy());
    }
    #[test]
    fn clear033f_tail_authorizes_only_exact_tagged0333_transition() {
        let mut tail = scan_tail_schedule();
        for slot in 5..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            tail[offset + 4..offset + 8].copy_from_slice(&0x067f0000u32.to_le_bytes());
        }
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(tail, 392_007_511, 13655, 32), 3);
        let previous = hle.previous_command.unwrap();
        assert_eq!(previous, [0x00, 0x08, 0xeb, 0x06, 0x00, 0x00, 0x7f, 0x06]);
        hle.clear();
        assert_eq!(hle.take_ready(392_229_248).len(), 3);
        let bytes = tagged0333_schedule(512, Some(previous));
        assert!(decode(&bytes).is_empty());
        assert_eq!(decode_with_residue(&bytes, Some(&previous)).len(), 5);
        for position in COMMAND_OFFSET + 3 * 40 + 8..COMMAND_OFFSET + 4 * 40 {
            let mut wrong = bytes.clone();
            wrong[position] ^= 1;
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        for wrong_previous in [
            [0x00, 0x08, 0xeb, 0x06, 0x00, 0x00, 0x67, 0x06], // other prior profile
            [0x00, 0x08, 0xe9, 0x06, 0x00, 0x00, 0x7f, 0x06], // channel884
            [0x00, 0x08, 0xeb, 0x0e, 0x00, 0x00, 0x7f, 0x06], // tagged prior
        ] {
            assert!(decode_with_residue(&tagged0333_schedule(512, Some(wrong_previous)),
                Some(&wrong_previous)).is_empty());
        }
        for (channel, count) in [(511, 4), (517, 0)] {
            assert_eq!(decode_with_residue(&tagged0333_schedule(channel, Some(previous)),
                Some(&previous)).len(), count);
        }
        assert_eq!(hle.submit(bytes.clone(), 394_085_629, 11773, 32), 5);
        assert_eq!(hle.previous_command.unwrap(),
            [0x00, 0x08, 0x09, 0x0c, 0x00, 0x00, 0x67, 0x06]);
        hle.reset();
        assert_eq!(hle.submit(bytes, 400_000_000, 0, 32), 0);
    }
    #[test]
    fn tagged_restart_masks_channel_bits_and_requires_matching_tag() {
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(tagged0333_schedule(517, None), 0, 0, 32), 5);
        let previous = hle.previous_command.unwrap();
        hle.clear();
        let restart = tagged0333_schedule(522, Some(previous));
        assert_eq!(decode_with_residue(&restart, Some(&previous)).len(), 5);
        let mut clear = restart.clone();
        for slot in 3..=7 { clear[COMMAND_OFFSET + slot * 40 + 3] &= !8; }
        assert!(decode_with_residue(&clear, Some(&previous)).is_empty());
        let mut clear_previous = previous;
        clear_previous[3] &= !8;
        assert!(decode_with_residue(&tagged0333_schedule(522, Some(clear_previous)),
            Some(&clear_previous)).is_empty());
        assert_eq!(hle.submit(restart, 320_000, 0, 32), 5);
    }
    #[test]
    fn tagged_unknown_profiles_and_reserved_bits_still_reject() {
        for profile in [0x335u32, 0x332, 0x334, 0x33b] {
            let mut bytes = tagged0333_schedule(517, None);
            let offset = COMMAND_OFFSET + 3 * 40;
            bytes[offset + 4..offset + 8].copy_from_slice(&((profile << 6 | 0x20) << 11).to_le_bytes());
            assert_eq!(decode(&bytes).len(), 4);
        }
        let mut bytes = tagged0333_schedule(517, None);
        bytes[COMMAND_OFFSET + 3 * 40 + 3] |= 0x10;
        assert_eq!(decode(&bytes).len(), 4);
    }
    fn tagged0333_four_request_tail() -> Vec<u8> {
        let mut bytes = tagged0333_schedule(807, None);
        bytes[..4].copy_from_slice(&0xffu32.to_le_bytes());
        bytes
    }
    fn tagged033d_schedule(first_channel: u32, residue: Option<[u8; 8]>) -> Vec<u8> {
        let mut bytes = tagged0333_schedule(first_channel, residue);
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            bytes[offset + 4..offset + 8].copy_from_slice(&0x067b0000u32.to_le_bytes());
        }
        bytes
    }
    fn tagged033f_schedule(first_channel: u32, residue: Option<[u8; 8]>) -> Vec<u8> {
        let mut bytes = tagged0333_schedule(first_channel, residue);
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            bytes[offset + 4..offset + 8].copy_from_slice(&0x067f0000u32.to_le_bytes());
        }
        bytes
    }
    fn menu_band_transition_schedule(residue: [u8; 8]) -> Vec<u8> {
        let mut bytes = schedule();
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            bytes[offset + 8..offset + 16].copy_from_slice(&residue);
        }
        bytes
    }
    #[test]
    fn captured_tagged033d_tail_and033f_transition_keep_overlapping_deadlines() {
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(tagged033d_schedule(802, None), 421_930_768, 16912, 32), 5);
        assert_eq!(hle.previous_command, Some([0x00, 0x08, 0x4d, 0x0e, 0x00, 0x00, 0x7b, 0x06]));
        hle.clear();
        let mut tail = tagged033d_schedule(807, None);
        tail[..4].copy_from_slice(&0xffu32.to_le_bytes());
        assert_eq!(hle.submit(tail.clone(), 422_088_363, 14507, 32), 4);
        let previous = [0x00, 0x08, 0x55, 0x0e, 0x00, 0x00, 0x7b, 0x06];
        assert_eq!(hle.previous_command, Some(previous));
        for (slot, deadline) in [(7, 422_091_008), (6, 422_120_128), (5, 422_149_248),
            (4, 422_178_368), (3, 422_207_488), (7, 422_251_008), (6, 422_280_128),
            (5, 422_309_248), (4, 422_338_368)] {
            assert!(hle.take_ready(deadline - 1).is_empty());
            assert_eq!(hle.take_ready(deadline), vec![(slot, -400)]);
        }
        assert!(!hle.busy());
        hle.clear();
        tail[..4].fill(0);
        assert_eq!(hle.submit(tail, 426_700_000, 0, 32), 0);
        assert_eq!(hle.previous_command, Some(previous));
        let transition = tagged033f_schedule(512, Some(previous));
        assert!(decode(&transition).is_empty());
        // Captured mask/commands; phase follows the unchanged timer origin.
        assert_eq!(hle.submit(transition.clone(), 426_886_248, 12392, 32), 5);
        assert_eq!(hle.submit(transition.clone(), 426_886_249, 12393, 32), 0);
        assert_eq!(hle.previous_command, Some([0x00, 0x08, 0x09, 0x0c, 0x00, 0x00, 0x7f, 0x06]));
        hle.clear();
        assert_eq!(hle.submit(tagged033f_schedule(517, None), 427_046_014, 12158, 32), 5);
        for (slot, deadline) in [(7, 427_051_008), (6, 427_080_128), (5, 427_109_248),
            (4, 427_138_368), (3, 427_167_488), (7, 427_211_008), (6, 427_240_128),
            (5, 427_269_248), (4, 427_298_368), (3, 427_327_488)] {
            assert!(hle.take_ready(deadline - 1).is_empty());
            assert_eq!(hle.take_ready(deadline), vec![(slot, -400)]);
        }
        hle.reset();
        assert_eq!(hle.submit(transition, 428_000_000, 0, 32), 0);
    }
    #[test]
    fn tagged033d_tail_requires_exact_history_records_and_matching_profile() {
        for mutation in 0..13 {
            let mut hle = RfHle::new(true);
            let prior = if mutation == 0 { 801 } else { 802 };
            let prior_bytes = if mutation == 11 { tagged0333_schedule(prior, None) }
                else if mutation == 12 { tagged033f_schedule(prior, None) }
                else { tagged033d_schedule(prior, None) };
            assert_eq!(hle.submit(prior_bytes, 0, 0, 32), 5);
            let previous = hle.previous_command;
            hle.clear();
            let mut tail = tagged033d_schedule(807, None);
            tail[..4].copy_from_slice(&0xffu32.to_le_bytes());
            let command = COMMAND_OFFSET + 4 * 40;
            match mutation {
                0 | 11 | 12 => {},
                1 => { tail[..4].copy_from_slice(&0x3ffu32.to_le_bytes()); tail[COMMAND_OFFSET + 3 * 40 + 8] = 1; },
                2 => tail[..4].copy_from_slice(&0xff0u32.to_le_bytes()),
                3 => tail[command + 8] = 1,
                4 => tail[command + 3] &= !8,
                5 => tail[command + 4..command + 8].copy_from_slice(&0x06670000u32.to_le_bytes()),
                6 => tail[command..command + 4].copy_from_slice(&0x0e570800u32.to_le_bytes()),
                7 => put(&mut tail, 4 + 3 * 12 + 2, 2768),
                8 => put(&mut tail, 4 + 3 * 12 + 8, 3267),
                9 => put(&mut tail, 4 + 3 * 12, 0xa500),
                _ => tail[command + 3] |= 0x10,
            }
            hle.submit(tail, 320_000, 0, 32);
            assert_eq!(hle.previous_command, previous, "mutation {mutation}");
            hle.clear();
            assert_eq!(hle.submit(tagged033f_schedule(512,
                Some([0x00, 0x08, 0x55, 0x0e, 0x00, 0x00, 0x7b, 0x06])), 640_000, 0, 32), 0);
        }
    }
    #[test]
    fn tagged033f_transition_requires_exact033d_tail_and_all_extension_bytes() {
        let previous = [0x00, 0x08, 0x55, 0x0e, 0x00, 0x00, 0x7b, 0x06];
        let bytes = tagged033f_schedule(512, Some(previous));
        assert_eq!(decode_with_residue(&bytes, Some(&previous)).len(), 5);
        for index in 0..8 {
            let mut wrong = previous;
            wrong[index] ^= 1;
            assert!(decode_with_residue(&bytes, Some(&wrong)).is_empty());
        }
        for (channel, profile, tag) in [(809u32, 0x33du32, true), (811, 0x33d, true),
            (810, 0x333, true), (810, 0x335, true), (810, 0x33d, false)] {
            let mut wrong = previous;
            wrong[..4].copy_from_slice(&((0x21 | if tag { 0x10000 } else { 0 } | channel << 6) << 11).to_le_bytes());
            wrong[4..].copy_from_slice(&((0x20 | profile << 6) << 11).to_le_bytes());
            assert!(decode_with_residue(&tagged033f_schedule(512, Some(wrong)), Some(&wrong)).is_empty());
        }
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            for byte in offset + 8..offset + 40 {
                let mut wrong = bytes.clone();
                wrong[byte] ^= 1;
                assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
            }
            for channel in [0u32, 511, 517, 810, 1023] {
                let mut wrong = bytes.clone();
                wrong[offset..offset + 4].copy_from_slice(&((0x10021 | channel << 6) << 11).to_le_bytes());
                assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
            }
        }
        let mut clear = bytes;
        for slot in 3..=7 { clear[COMMAND_OFFSET + slot * 40 + 3] &= !8; }
        assert!(decode_with_residue(&clear, Some(&previous)).is_empty());
    }
    #[test]
    fn tagged033f_zero_extension_and_restart_keep_ten_bit_channel_and_strict_bits() {
        let bytes = tagged033f_schedule(517, None);
        let requests = decode(&bytes);
        assert_eq!(requests.iter().map(|request| request.channel).collect::<Vec<_>>(),
            vec![521, 520, 519, 518, 517]);
        assert!(requests.iter().all(|request| request.calibration == 0x33f));
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(bytes.clone(), 0, 0, 32), 5);
        let previous = hle.previous_command.unwrap();
        hle.clear();
        assert_eq!(hle.submit(tagged033f_schedule(522, Some(previous)), 320_000, 0, 32), 5);
        let offset = COMMAND_OFFSET + 3 * 40;
        for (position, bit) in [(offset, 1), (offset + 3, 0x10), (offset + 4, 1), (offset + 7, 0x08)] {
            let mut malformed = bytes.clone();
            malformed[position] |= bit;
            assert_eq!(decode(&malformed).len(), 4);
        }
        let mut untagged_previous = previous;
        untagged_previous[3] &= !8;
        assert!(decode_with_residue(&tagged033f_schedule(522, Some(untagged_previous)),
            Some(&untagged_previous)).is_empty());
    }
    #[test]
    fn captured_tagged033f_tail_and_same_profile_wrap_preserve_each_deadline() {
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(tagged033f_schedule(802, None), 438_410_791, 16935, 32), 5);
        assert_eq!(hle.previous_command, Some([0, 8, 0x4d, 0x0e, 0, 0, 0x7f, 6]));
        hle.clear();
        let mut tail = tagged033f_schedule(807, None);
        tail[..4].copy_from_slice(&0xffu32.to_le_bytes());
        assert_eq!(hle.submit(tail.clone(), 438_569_060, 15204, 32), 4);
        let previous = [0, 8, 0x55, 0x0e, 0, 0, 0x7f, 6];
        assert_eq!(hle.previous_command, Some(previous));
        assert_eq!(hle.submit(tail.clone(), 438_569_061, 15205, 32), 0);
        for (slot, deadline) in [(7, 438_571_008), (6, 438_600_128), (5, 438_629_248),
            (4, 438_658_368), (3, 438_687_488), (7, 438_731_008), (6, 438_760_128),
            (5, 438_789_248), (4, 438_818_368)] {
            assert!(hle.take_ready(deadline - 1).is_empty());
            assert_eq!(hle.take_ready(deadline), vec![(slot, -400)]);
        }
        hle.clear();
        tail[..4].fill(0);
        assert_eq!(hle.submit(tail, 443_195_000, 0, 32), 0);
        assert_eq!(hle.previous_command, Some(previous));
        let wrap = tagged033f_schedule(512, Some(previous));
        assert!(decode(&wrap).is_empty());
        assert_eq!(hle.submit(wrap.clone(), 443_366_255, 12399, 32), 5);
        assert_eq!(hle.previous_command, Some([0, 8, 9, 0x0c, 0, 0, 0x7f, 6]));
        assert_eq!(hle.submit(wrap.clone(), 443_366_256, 12400, 32), 0);
        hle.clear();
        assert_eq!(hle.submit(tagged033f_schedule(517, None), 443_526_019, 12163, 32), 5);
        for (slot, deadline) in [(7, 443_531_008), (6, 443_560_128), (5, 443_589_248),
            (4, 443_618_368), (3, 443_647_488), (7, 443_691_008), (6, 443_720_128),
            (5, 443_749_248), (4, 443_778_368), (3, 443_807_488)] {
            assert!(hle.take_ready(deadline - 1).is_empty());
            assert_eq!(hle.take_ready(deadline), vec![(slot, -400)]);
        }
        assert!(!hle.busy());
        hle.reset();
        assert_eq!(hle.previous_command, None);
        assert_eq!(hle.submit(wrap, 444_000_000, 0, 32), 0);
    }
    #[test]
    fn menu_band_transition_requires_exact_tagged033f_history_and_residue() {
        // Accepted tagged802..806/033f followed by the captured807..810 tail
        // establishes the exact command retained by the post-Menu group.
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(tagged033f_schedule(802, None), 438_410_791, 16935, 32), 5);
        hle.clear();
        let mut tail = tagged033f_schedule(807, None);
        tail[..4].copy_from_slice(&0xffu32.to_le_bytes());
        assert_eq!(hle.submit(tail, 438_569_060, 15204, 32), 4);
        let previous = [0x00, 0x08, 0x55, 0x0e, 0x00, 0x00, 0x7f, 0x06];
        assert_eq!(hle.previous_command, Some(previous));
        let transition = menu_band_transition_schedule(previous);
        assert_eq!(hle.submit(transition.clone(), 438_569_061, 15205, 32), 0);
        assert_eq!(hle.take_ready(439_000_000).len(), 9);

        assert!(decode(&transition).is_empty());
        assert_eq!(decode_with_residue(&transition, Some(&previous)).len(), 5);
        assert_eq!(hle.submit(transition.clone(), 908_166_322, 12466, 32), 5);
        assert_eq!(hle.previous_command, Some([0x00, 0x08, 0x09, 0x00, 0x00, 0x00, 0x6b, 0x06]));
        for (slot, deadline) in [(7, 908_331_008), (6, 908_360_128), (5, 908_389_248),
            (4, 908_418_368), (3, 908_447_488)] {
            assert!(hle.take_ready(deadline - 1).is_empty());
            assert_eq!(hle.take_ready(deadline), vec![(slot, -400)]);
        }
        assert!(!hle.busy());

        for (channel, profile, tagged) in [(809u32, 0x33fu32, true), (811, 0x33f, true),
            (810, 0x333, true), (810, 0x335, true), (810, 0x33f, false)] {
            let mut wrong = previous;
            wrong[..4].copy_from_slice(&((0x21 | if tagged { 0x10000 } else { 0 }
                | channel << 6) << 11).to_le_bytes());
            wrong[4..].copy_from_slice(&((0x20 | profile << 6) << 11).to_le_bytes());
            assert!(decode_with_residue(&menu_band_transition_schedule(wrong), Some(&wrong)).is_empty());
        }

        let mut wrong_profile = transition.clone();
        let mut wrong_tag = transition.clone();
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            wrong_profile[offset + 4..offset + 8].copy_from_slice(&0x06670000u32.to_le_bytes());
            wrong_tag[offset + 3] |= 8;
        }
        assert!(decode_with_residue(&wrong_profile, Some(&previous)).is_empty());
        assert!(decode_with_residue(&wrong_tag, Some(&previous)).is_empty());

        let mut outside_range = transition.clone();
        for (index, slot) in (3..=7).rev().enumerate() {
            let offset = COMMAND_OFFSET + slot * 40;
            outside_range[offset..offset + 4].copy_from_slice(
                &((0x21u32 | (5 + index as u32) << 6) << 11).to_le_bytes());
        }
        assert!(decode_with_residue(&outside_range, Some(&previous)).is_empty());
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            for byte in offset + 8..offset + 40 {
                let mut wrong = transition.clone();
                wrong[byte] ^= 1;
                assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4,
                    "slot {slot} extension byte {byte}");
            }
            let mut wrong = transition.clone();
            wrong[offset..offset + 4].copy_from_slice(&((0x21u32 | 5 << 6) << 11).to_le_bytes());
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }

        // A partial or noncontiguous tagged tail cannot establish channel810
        // as trusted history for the otherwise exact post-Menu commands.
        for mask in [0x0ff0u32, 0x00f3] {
            let mut untrusted = RfHle::new(true);
            assert_eq!(untrusted.submit(tagged033f_schedule(802, None), 0, 0, 32), 5);
            let accepted806 = untrusted.previous_command;
            untrusted.clear();
            let mut malformed_tail = tagged033f_schedule(807, None);
            malformed_tail[..4].copy_from_slice(&mask.to_le_bytes());
            untrusted.submit(malformed_tail, 320_000, 0, 32);
            assert_eq!(untrusted.previous_command, accepted806);
            untrusted.clear();
            assert_eq!(untrusted.submit(transition.clone(), 640_000, 0, 32), 0);
        }
    }
    #[test]
    fn tagged033f_tail_rejects_untrusted_history_and_partial_or_mutated_records() {
        for mutation in 0..15 {
            let mut hle = RfHle::new(true);
            let prior = if mutation == 0 { 801 } else { 802 };
            let prior_bytes = match mutation {
                11 => tagged0333_schedule(prior, None),
                12 => tagged033d_schedule(prior, None),
                _ => tagged033f_schedule(prior, None),
            };
            assert_eq!(hle.submit(prior_bytes, 0, 0, 32), 5);
            hle.clear();
            let mut tail = tagged033f_schedule(807, None);
            tail[..4].copy_from_slice(&0xffu32.to_le_bytes());
            let command = COMMAND_OFFSET + 4 * 40;
            match mutation {
                0 | 11 | 12 => {},
                1 => { tail[..4].copy_from_slice(&0x3ffu32.to_le_bytes()); tail[COMMAND_OFFSET + 3 * 40 + 8] = 1; },
                2 => tail[..4].copy_from_slice(&0xff0u32.to_le_bytes()),
                3 => tail[command + 8] = 1,
                4 => tail[command + 3] &= !8,
                5 => tail[command + 4..command + 8].copy_from_slice(&0x067b0000u32.to_le_bytes()),
                6 => tail[command..command + 4].copy_from_slice(&0x0e570800u32.to_le_bytes()),
                7 => put(&mut tail, 4 + 3 * 12 + 2, 2768),
                8 => put(&mut tail, 4 + 3 * 12 + 8, 3267),
                9 => put(&mut tail, 4 + 3 * 12, 0xa500),
                10 => tail[command + 3] |= 0x10,
                13 => hle.previous_command = None,
                _ => put(&mut tail, 4 + 3 * 12 + 4, 1),
            }
            let previous = hle.previous_command;
            hle.submit(tail, 320_000, 0, 32);
            assert_eq!(hle.previous_command, previous, "mutation {mutation}");
            hle.clear();
            assert_eq!(hle.submit(tagged033f_schedule(512, Some([0, 8, 0x55, 0x0e, 0, 0, 0x7f, 6])),
                640_000, 0, 32), 0);
        }
    }
    #[test]
    fn tagged033f_wrap_requires_exact_residue_profile_range_tag_and_every_extension_byte() {
        let previous = [0, 8, 0x55, 0x0e, 0, 0, 0x7f, 6];
        let bytes = tagged033f_schedule(512, Some(previous));
        assert_eq!(decode_with_residue(&bytes, Some(&previous)).len(), 5);
        for index in 0..8 {
            let mut wrong = previous;
            wrong[index] ^= 1;
            assert!(decode_with_residue(&bytes, Some(&wrong)).is_empty());
        }
        for (channel, profile, tag) in [(806u32, 0x33fu32, true), (809, 0x33f, true),
            (811, 0x33f, true), (810, 0x333, true), (810, 0x335, true), (810, 0x33f, false)] {
            let mut wrong = previous;
            wrong[..4].copy_from_slice(&((0x21 | if tag { 0x10000 } else { 0 } | channel << 6) << 11).to_le_bytes());
            wrong[4..].copy_from_slice(&((0x20 | profile << 6) << 11).to_le_bytes());
            assert!(decode_with_residue(&tagged033f_schedule(512, Some(wrong)), Some(&wrong)).is_empty());
        }
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            for byte in offset + 8..offset + 40 {
                let mut wrong = bytes.clone();
                wrong[byte] ^= 1;
                assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
            }
            for channel in [0u32, 511, 517, 810, 1023] {
                let mut wrong = bytes.clone();
                wrong[offset..offset + 4].copy_from_slice(&((0x10021 | channel << 6) << 11).to_le_bytes());
                assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
            }
        }
        let mut clear = bytes;
        for slot in 3..=7 { clear[COMMAND_OFFSET + slot * 40 + 3] &= !8; }
        assert!(decode_with_residue(&clear, Some(&previous)).is_empty());
    }
    #[test]
    fn tagged_same_profile_wrap_is_not_enabled_for_other_profiles() {
        for profile in [0x333u32, 0x33d, 0x335, 0x332, 0x33b] {
            let mut previous = [0, 8, 0x55, 0x0e, 0, 0, 0, 0];
            previous[4..].copy_from_slice(&((0x20 | profile << 6) << 11).to_le_bytes());
            let mut bytes = tagged033f_schedule(512, Some(previous));
            for slot in 3..=7 {
                let offset = COMMAND_OFFSET + slot * 40;
                bytes[offset + 4..offset + 8].copy_from_slice(&previous[4..]);
            }
            assert!(decode_with_residue(&bytes, Some(&previous)).is_empty());
        }
    }
    #[test]
    fn captured_four_request_tail_and_tagged033d_transition_keep_each_deadline() {
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(tagged0333_schedule(802, None), 405_610_184, 16328, 32), 5);
        assert_eq!(hle.previous_command.unwrap(), [0x00, 0x08, 0x4d, 0x0e, 0x00, 0x00, 0x67, 0x06]);
        hle.clear();
        assert_eq!(hle.submit(tagged0333_four_request_tail(), 405_768_363, 14507, 32), 4);
        let previous = [0x00, 0x08, 0x55, 0x0e, 0x00, 0x00, 0x67, 0x06];
        assert_eq!(hle.previous_command, Some(previous));
        // Clearing the producer does not cancel the overlapping earlier five.
        assert!(hle.take_ready(405_771_007).is_empty());
        for (slot, deadline) in [(7, 405_771_008), (6, 405_800_128), (5, 405_829_248),
            (4, 405_858_368), (3, 405_887_488), (7, 405_931_008), (6, 405_960_128),
            (5, 405_989_248), (4, 406_018_368)] {
            assert!(hle.take_ready(deadline - 1).is_empty());
            assert_eq!(hle.take_ready(deadline), vec![(slot, -400)]);
        }
        assert!(!hle.busy());
        hle.clear();
        let mut idle = tagged0333_four_request_tail();
        idle[..4].fill(0);
        assert_eq!(hle.submit(idle, 410_235_273, 0, 32), 0);
        assert_eq!(hle.previous_command, Some(previous));
        let transition = tagged033d_schedule(512, Some(previous));
        assert!(decode(&transition).is_empty());
        assert_eq!(hle.submit(transition.clone(), 410_405_639, 11783, 32), 5);
        assert_eq!(hle.submit(transition.clone(), 410_405_640, 11784, 32), 0);
        assert_eq!(hle.previous_command.unwrap(), [0x00, 0x08, 0x09, 0x0c, 0x00, 0x00, 0x7b, 0x06]);
        hle.clear();
        assert_eq!(hle.submit(tagged033d_schedule(517, None), 410_565_420, 11564, 32), 5);
        for (slot, deadline) in [(7, 410_571_008), (6, 410_600_128), (5, 410_629_248),
            (4, 410_658_368), (3, 410_687_488), (7, 410_731_008), (6, 410_760_128),
            (5, 410_789_248), (4, 410_818_368), (3, 410_847_488)] {
            assert!(hle.take_ready(deadline - 1).is_empty());
            assert_eq!(hle.take_ready(deadline), vec![(slot, -400)]);
        }
        hle.reset();
        assert_eq!(hle.submit(transition, 411_000_000, 0, 32), 0);
    }
    #[test]
    fn four_request_history_is_not_a_generic_count_or_partial_batch_exception() {
        for mutation in 0..12 {
            let mut hle = RfHle::new(true);
            let prior = if mutation == 0 { 801 } else { 802 };
            assert_eq!(hle.submit(tagged0333_schedule(prior, None), 0, 0, 32), 5);
            let previous = hle.previous_command;
            hle.clear();
            let mut bytes = tagged0333_four_request_tail();
            let command = COMMAND_OFFSET + 4 * 40;
            match mutation {
                0 => {}, // wrong accepted prior channel805, not806
                1 => { // four recognized requests out of a malformed five
                    bytes[..4].copy_from_slice(&0x3ffu32.to_le_bytes());
                    bytes[COMMAND_OFFSET + 3 * 40 + 8] = 1;
                },
                2 => bytes[..4].copy_from_slice(&0xff0u32.to_le_bytes()), // noncontiguous mask
                3 => bytes[command + 8] = 1,
                4 => bytes[command + 3] &= !8, // clear tag
                5 => bytes[command + 4..command + 8].copy_from_slice(&0x067b0000u32.to_le_bytes()),
                6 => bytes[command..command + 4].copy_from_slice(&0x0e570800u32.to_le_bytes()), //811
                7 => put(&mut bytes, 4 + 3 * 12 + 2, 2768), // wrong start
                8 => put(&mut bytes, 4 + 3 * 12 + 8, 3267), // wrong finish
                9 => put(&mut bytes, 4 + 3 * 12, 0xa500), // duplicated slot
                10 => bytes[command + 3] |= 0x10, // reserved bit
                _ => hle.previous_command = None,
            }
            let expected = if mutation == 11 { None } else { previous };
            hle.submit(bytes, 320_000, 0, 32);
            assert_eq!(hle.previous_command, expected, "mutation {mutation}");
            hle.clear();
            let residue = [0x00, 0x08, 0x55, 0x0e, 0x00, 0x00, 0x67, 0x06];
            assert_eq!(hle.submit(tagged033d_schedule(512, Some(residue)), 640_000, 0, 32), 0);
        }
    }
    #[test]
    fn tagged033d_transition_requires_exact_tail_profile_channel_tag_and_extension() {
        let previous = [0x00, 0x08, 0x55, 0x0e, 0x00, 0x00, 0x67, 0x06];
        let bytes = tagged033d_schedule(512, Some(previous));
        assert_eq!(decode_with_residue(&bytes, Some(&previous)).len(), 5);
        for index in 0..8 {
            let mut wrong = previous;
            wrong[index] ^= 1;
            assert!(decode_with_residue(&bytes, Some(&wrong)).is_empty());
        }
        for (channel, profile, tag) in [(809u32, 0x333u32, true), (811, 0x333, true),
            (810, 0x33d, true), (810, 0x33f, true), (810, 0x333, false)] {
            let mut wrong = previous;
            wrong[..4].copy_from_slice(&((0x21 | if tag { 0x10000 } else { 0 } | channel << 6) << 11).to_le_bytes());
            wrong[4..].copy_from_slice(&((0x20 | profile << 6) << 11).to_le_bytes());
            assert!(decode_with_residue(&tagged033d_schedule(512, Some(wrong)), Some(&wrong)).is_empty());
        }
        let offset = COMMAND_OFFSET + 7 * 40;
        for byte in offset + 8..offset + 40 {
            let mut wrong = bytes.clone();
            wrong[byte] ^= 1;
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        for channel in [0u32, 511, 517, 810, 1023] {
            let mut wrong = bytes.clone();
            wrong[offset..offset + 4].copy_from_slice(&((0x10021 | channel << 6) << 11).to_le_bytes());
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        let mut clear = bytes.clone();
        for slot in 3..=7 { clear[COMMAND_OFFSET + slot * 40 + 3] &= !8; }
        assert!(decode_with_residue(&clear, Some(&previous)).is_empty());
    }
    #[test]
    fn tagged033d_zero_extension_and_same_profile_restart_preserve_ten_bit_channels() {
        let bytes = tagged033d_schedule(517, None);
        let requests = decode(&bytes);
        assert_eq!(requests.iter().map(|request| request.channel).collect::<Vec<_>>(),
            vec![521, 520, 519, 518, 517]);
        assert!(requests.iter().all(|request| request.calibration == 0x33d));
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(bytes, 0, 0, 32), 5);
        let previous = hle.previous_command.unwrap();
        hle.clear();
        assert_eq!(hle.submit(tagged033d_schedule(522, Some(previous)), 320_000, 0, 32), 5);
        let mut malformed = tagged033d_schedule(517, None);
        malformed[COMMAND_OFFSET + 3 * 40 + 3] |= 0x10;
        assert_eq!(decode(&malformed).len(), 4);
        malformed = tagged033d_schedule(517, None);
        malformed[COMMAND_OFFSET + 3 * 40] = 1;
        assert_eq!(decode(&malformed).len(), 4);
    }
    fn band_transition_schedule() -> Vec<u8> {
        let mut bytes = schedule();
        for index in 0..5 {
            let offset = COMMAND_OFFSET + (7 - index) * 40;
            bytes[offset + 4..offset + 8].copy_from_slice(&0x067f0000u32.to_le_bytes());
            bytes[offset + 8..offset + 12].copy_from_slice(&0x06eb0800u32.to_le_bytes());
            bytes[offset + 12..offset + 16].copy_from_slice(&0x06670000u32.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn fully_recognized_three_request_tail_supplies_band_transition_history() {
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(scan_tail_schedule(), 293_606_428, 12572, 32), 3);
        let expected = [0x00, 0x08, 0xeb, 0x06, 0x00, 0x00, 0x67, 0x06];
        assert_eq!(hle.previous_command, Some(expected));
        hle.clear();
        assert_eq!(hle.take_ready(293_771_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(293_829_248), vec![(5, -400), (6, -400)]);
        let bytes = band_transition_schedule();
        assert!(decode(&bytes).is_empty()); // no trusted history
        assert_eq!(hle.submit(bytes, 296_165_776, 11920, 32), 5);
        assert_eq!(hle.take_ready(296_331_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(296_447_488).len(), 4);
        assert!(!hle.busy());
        hle.reset();
        assert_eq!(hle.submit(band_transition_schedule(), 300_000_000, 0, 32), 0);
    }
    #[test]
    fn unsupported_or_noncontiguous_tail_cannot_replace_history() {
        for mutation in 0..3 {
            let mut bytes = scan_tail_schedule();
            match mutation {
                0 => bytes[COMMAND_OFFSET + 5 * 40 + 8] = 1,
                1 => bytes[..4].copy_from_slice(&0x7fu32.to_le_bytes()),
                _ => bytes[..4].copy_from_slice(&0x3f0u32.to_le_bytes()),
            }
            let previous = [0x00, 0x08, 0x15, 0x04, 0x00, 0x00, 0x67, 0x06];
            let mut hle = RfHle { enabled: true, previous_command: Some(previous), ..Default::default() };
            hle.submit(bytes, 0, 0, 32);
            assert_eq!(hle.previous_command, Some(previous), "mutation {mutation}");
        }
    }
    #[test]
    fn band_transition_requires_exact_observed_history_and_extension() {
        let bytes = band_transition_schedule();
        let previous = [0x00, 0x08, 0xeb, 0x06, 0x00, 0x00, 0x67, 0x06];
        assert_eq!(decode_with_residue(&bytes, Some(&previous)).len(), 5);
        for index in 0..8 {
            let mut wrong = previous;
            wrong[index] ^= 1;
            assert!(decode_with_residue(&bytes, Some(&wrong)).is_empty());
        }
        for index in COMMAND_OFFSET + 7 * 40 + 8..COMMAND_OFFSET + 8 * 40 {
            let mut wrong = bytes.clone();
            wrong[index] ^= 1;
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        let offset = COMMAND_OFFSET + 7 * 40;
        for channel in [5u32, 512, 885, 1023] {
            let mut wrong = bytes.clone();
            wrong[offset..offset + 4].copy_from_slice(&((0x21 | channel << 6) << 11).to_le_bytes());
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
    }
    #[test]
    fn profile033d_three_request_tail_supplies_exact_band_transition_history() {
        // Original control snapshots at318086635 and320805689: complete
        // slots7..5/channel883..885/033d, then0..4/033f with885/033d residue.
        let mut tail = scan_tail_schedule();
        for slot in 5..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            tail[offset + 4..offset + 8].copy_from_slice(&0x067b0000u32.to_le_bytes());
        }
        let mut transition = band_transition_schedule();
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            transition[offset + 12..offset + 16].copy_from_slice(&0x067b0000u32.to_le_bytes());
        }
        let previous = [0x00, 0x08, 0xeb, 0x06, 0x00, 0x00, 0x7b, 0x06];
        assert!(decode(&transition).is_empty());
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(tail.clone(), 318_086_635, 12779, 32), 3);
        assert_eq!(hle.previous_command, Some(previous));
        hle.clear();
        assert_eq!(hle.take_ready(318_251_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(318_309_248).len(), 2);
        assert_eq!(hle.submit(transition.clone(), 320_805_689, 11833, 32), 5);
        assert_eq!(hle.take_ready(320_971_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(321_087_488).len(), 4);
        assert!(!hle.busy());
        hle.reset();
        assert_eq!(hle.submit(transition.clone(), 322_000_000, 0, 32), 0);

        for index in 0..8 {
            let mut wrong = previous;
            wrong[index] ^= 1;
            assert!(decode_with_residue(&transition, Some(&wrong)).is_empty());
        }
        let offset = COMMAND_OFFSET + 7 * 40;
        for index in offset + 8..offset + 40 {
            let mut wrong = transition.clone();
            wrong[index] ^= 1;
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        for channel in [5u32, 512, 885, 1023] {
            let mut wrong = transition.clone();
            wrong[offset..offset + 4].copy_from_slice(&((0x21 | channel << 6) << 11).to_le_bytes());
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        // Authentic885 bytes of the other supported profile are still not proof
        // of a captured cross-band transition, even with matching extensions.
        for profile in [0x335u32] {
            let mut wrong = transition.clone();
            let mut history = previous;
            history[4..].copy_from_slice(&((0x20 | profile << 6) << 11).to_le_bytes());
            for slot in 3..=7 {
                let command = COMMAND_OFFSET + slot * 40;
                wrong[command + 8..command + 16].copy_from_slice(&history);
            }
            assert!(decode_with_residue(&wrong, Some(&history)).is_empty());
        }
        for mutation in 0..3 {
            let mut invalid_tail = tail.clone();
            match mutation {
                0 => invalid_tail[COMMAND_OFFSET + 5 * 40 + 8] = 1,
                1 => invalid_tail[..4].copy_from_slice(&0x7fu32.to_le_bytes()),
                _ => invalid_tail[..4].copy_from_slice(&0x3f0u32.to_le_bytes()),
            }
            let mut untrusted = RfHle::new(true);
            untrusted.submit(invalid_tail, 0, 0, 32);
            assert_eq!(untrusted.previous_command, None);
            untrusted.clear();
            assert_eq!(untrusted.submit(transition.clone(), 1_000_000, 0, 32), 0);
        }
    }
    #[test]
    fn profile033f_tail_supplies_exact_scan_range_transition_history() {
        // Original342726638 tail883..885/033f, then345445804 range0..4
        // retains885/033f. This is not a generic channel-wrap exception.
        let mut tail = scan_tail_schedule();
        for slot in 5..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            tail[offset + 4..offset + 8].copy_from_slice(&0x067f0000u32.to_le_bytes());
        }
        let mut transition = band_transition_schedule();
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            transition[offset + 12..offset + 16].copy_from_slice(&0x067f0000u32.to_le_bytes());
        }
        let previous = [0x00, 0x08, 0xeb, 0x06, 0x00, 0x00, 0x7f, 0x06];
        assert!(decode(&transition).is_empty());
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(tail.clone(), 342_726_638, 12782, 32), 3);
        assert_eq!(hle.previous_command, Some(previous));
        hle.clear();
        assert_eq!(hle.take_ready(342_891_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(342_949_248).len(), 2);
        assert_eq!(hle.submit(transition.clone(), 345_445_804, 11948, 32), 5);
        assert_eq!(hle.take_ready(345_611_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(345_727_488).len(), 4);
        assert!(!hle.busy());
        hle.reset();
        assert_eq!(hle.submit(transition.clone(), 346_000_000, 0, 32), 0);
        for index in 0..8 {
            let mut wrong = previous;
            wrong[index] ^= 1;
            assert!(decode_with_residue(&transition, Some(&wrong)).is_empty());
        }
        let offset = COMMAND_OFFSET + 7 * 40;
        for index in offset + 8..offset + 40 {
            let mut wrong = transition.clone();
            wrong[index] ^= 1;
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        for channel in [5u32, 512, 885, 1023] {
            let mut wrong = transition.clone();
            wrong[offset..offset + 4].copy_from_slice(&((0x21 | channel << 6) << 11).to_le_bytes());
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        for profile in [0x333u32, 0x335, 0x33d] {
            let mut wrong = transition.clone();
            wrong[offset + 4..offset + 8].copy_from_slice(&((0x20 | profile << 6) << 11).to_le_bytes());
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        for mutation in 0..3 {
            let mut invalid = tail.clone();
            match mutation {
                0 => invalid[COMMAND_OFFSET + 5 * 40 + 8] = 1,
                1 => invalid[..4].copy_from_slice(&0x7fu32.to_le_bytes()),
                _ => invalid[..4].copy_from_slice(&0x3f0u32.to_le_bytes()),
            }
            let mut untrusted = RfHle::new(true);
            untrusted.submit(invalid, 0, 0, 32);
            assert_eq!(untrusted.previous_command, None);
            untrusted.clear();
            assert_eq!(untrusted.submit(transition.clone(), 1_000_000, 0, 32), 0);
        }
    }
    #[test]
    fn profile033f_zero_extension_uses_verified_ten_bit_channel_packing() {
        let mut bytes = band_transition_schedule();
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            bytes[offset + 8..offset + 40].fill(0);
        }
        assert_eq!(decode(&bytes).len(), 5);
        let offset = COMMAND_OFFSET + 7 * 40;
        bytes[offset..offset + 4].copy_from_slice(&((0x21u32 | 5 << 6) << 11).to_le_bytes());
        assert_eq!(decode(&bytes).len(), 5); // next frame has no restart residue
        for channel in [10u32, 14, 124, 512, 1023] {
            bytes[offset..offset + 4].copy_from_slice(&((0x21 | channel << 6) << 11).to_le_bytes());
            assert_eq!(decode(&bytes).len(), 5);
        }
    }
    fn profile033d_schedule(mixed: bool) -> Vec<u8> {
        let mut bytes = schedule();
        for index in 0..5 {
            let offset = COMMAND_OFFSET + (7 - index) * 40;
            let channel = if mixed { 508 + index as u32 } else { 513 + index as u32 };
            let second = if mixed && index < 4 { 0x067f0000u32 } else { 0x067b0000u32 };
            bytes[offset..offset + 4].copy_from_slice(&((0x21 | channel << 6) << 11).to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&second.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn observed033f_to033d_mixed_group_is_fully_recognized() {
        let bytes = profile033d_schedule(true);
        let requests = decode(&bytes);
        assert_eq!(requests.len(), 5);
        assert_eq!((requests[0].slot, requests[0].channel, requests[0].calibration), (3, 512, 0x33d));
        assert!(requests[1..].iter().all(|request| request.calibration == 0x33f));
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(bytes.clone(), 301_610_286, 0, 32), 5);
        let offset = COMMAND_OFFSET + 3 * 40;
        assert_eq!(hle.previous_command, Some(bytes[offset..offset + 8].try_into().unwrap()));
        assert_eq!(hle.take_ready(u64::MAX).len(), 5);
        assert!(hle.take_ready(u64::MAX).is_empty());
    }
    #[test]
    fn profile033d_preserves_strict_extensions_tags_and_unknown_profile_rejection() {
        let bytes = profile033d_schedule(false);
        let requests = decode(&bytes);
        assert_eq!(requests.len(), 5);
        assert!(requests.iter().all(|request| request.calibration == 0x33d));
        assert_eq!(requests.iter().map(|request| request.channel).collect::<Vec<_>>(),
            vec![517, 516, 515, 514, 513]);
        for mutation in 0..3 {
            let mut wrong = bytes.clone();
            let offset = COMMAND_OFFSET + 3 * 40;
            match mutation {
                0 => wrong[offset + 8] = 1,
                1 => wrong[offset + 3] |= 0x10, // reserved, not verified band tag
                _ => wrong[offset + 4..offset + 8].copy_from_slice(&0x06790000u32.to_le_bytes()),
            }
            assert_eq!(decode(&wrong).len(), 4);
            let previous = [0x00, 0x08, 0xeb, 0x06, 0x00, 0x00, 0x67, 0x06];
            let mut hle = RfHle { enabled: true, previous_command: Some(previous), ..Default::default() };
            assert_eq!(hle.submit(wrong, 0, 0, 32), 4);
            assert_eq!(hle.previous_command, Some(previous)); // not a four-request tail
        }
        // Authentic prior033d bytes cannot authorize a distant channel jump.
        let mut wrong = bytes.clone();
        let mut previous = [0u8; 8];
        previous[..4].copy_from_slice(&0x03ed0800u32.to_le_bytes()); // channel502, not preceding512
        previous[4..].copy_from_slice(&0x067b0000u32.to_le_bytes());
        for slot in 3..=7 {
            let offset = COMMAND_OFFSET + slot * 40;
            wrong[offset + 8..offset + 16].copy_from_slice(&previous);
        }
        assert!(decode_with_residue(&wrong, Some(&previous)).is_empty());
    }
    #[test]
    fn profile033d_restart_requires_accepted_matching_history() {
        let mut hle = RfHle::new(true);
        let mut preceding = profile033d_schedule(false);
        let mut restart = restart_schedule();
        for index in 0..5 {
            let offset = COMMAND_OFFSET + (7 - index) * 40;
            preceding[offset..offset + 4].copy_from_slice(&((0x21u32 | (518 + index as u32) << 6) << 11).to_le_bytes());
            restart[offset + 4..offset + 8].copy_from_slice(&0x067b0000u32.to_le_bytes());
            restart[offset + 12..offset + 16].copy_from_slice(&0x067b0000u32.to_le_bytes());
        }
        assert!(decode(&restart).is_empty());
        assert_eq!(hle.submit(preceding, 301_930_324, 16468, 32), 5);
        let history = hle.previous_command.unwrap();
        assert_eq!(history, [0x00, 0x08, 0x15, 0x04, 0x00, 0x00, 0x7b, 0x06]);
        hle.clear();
        assert_eq!(hle.take_ready(302_207_488).len(), 5);
        let mut idle = restart.clone();
        idle[..4].fill(0);
        assert_eq!(hle.submit(idle, 304_155_273, 0, 32), 0);
        assert_eq!(hle.previous_command, Some(history));
        for index in COMMAND_OFFSET + 7 * 40 + 8..COMMAND_OFFSET + 8 * 40 {
            let mut wrong = restart.clone();
            wrong[index] ^= 1;
            assert_eq!(decode_with_residue(&wrong, Some(&history)).len(), 4);
        }
        assert_eq!(hle.submit(restart.clone(), 304_325_606, 11750, 32), 5);
        assert_eq!(hle.take_ready(304_491_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(304_607_488).len(), 4);
        hle.reset();
        assert_eq!(hle.submit(restart, 310_000_000, 0, 32), 0);
    }
    #[test]
    fn profile033f_restart_requires_accepted_matching_history() {
        // Original326570078 control:518..522/033f, zero extensions.
        // Original328965634 control:523..527/033f, retained522/033f.
        let mut preceding = schedule();
        let mut restart = restart_schedule();
        for index in 0..5 {
            let offset = COMMAND_OFFSET + (7 - index) * 40;
            preceding[offset..offset + 4].copy_from_slice(
                &((0x21u32 | (518 + index as u32) << 6) << 11).to_le_bytes());
            preceding[offset + 4..offset + 8].copy_from_slice(&0x067f0000u32.to_le_bytes());
            restart[offset + 4..offset + 8].copy_from_slice(&0x067f0000u32.to_le_bytes());
            restart[offset + 12..offset + 16].copy_from_slice(&0x067f0000u32.to_le_bytes());
        }
        let previous = [0x00, 0x08, 0x15, 0x04, 0x00, 0x00, 0x7f, 0x06];
        assert!(decode(&restart).is_empty());
        let mut hle = RfHle::new(true);
        assert_eq!(hle.submit(preceding.clone(), 326_570_078, 16222, 32), 5);
        assert_eq!(hle.previous_command, Some(previous));
        hle.clear();
        let mut idle = restart.clone();
        idle[..4].fill(0);
        assert_eq!(hle.submit(idle, 328_800_000, 0, 32), 0);
        assert_eq!(hle.previous_command, Some(previous));
        assert_eq!(hle.submit(restart.clone(), 328_965_634, 11778, 32), 5);
        assert_eq!(hle.take_ready(329_131_007).len(), 5); // preceding accepted batch
        assert_eq!(hle.take_ready(329_131_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(329_247_488).len(), 4);
        assert!(!hle.busy());
        hle.reset();
        assert_eq!(hle.submit(restart.clone(), 330_000_000, 0, 32), 0);
        for index in 0..8 {
            let mut wrong = previous;
            wrong[index] ^= 1;
            assert!(decode_with_residue(&restart, Some(&wrong)).is_empty());
        }
        let offset = COMMAND_OFFSET + 7 * 40;
        for index in offset + 8..offset + 40 {
            let mut wrong = restart.clone();
            wrong[index] ^= 1;
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        for channel in [0u32, 521, 522, 528, 1023] {
            let mut wrong = restart.clone();
            wrong[offset..offset + 4].copy_from_slice(&((0x21 | channel << 6) << 11).to_le_bytes());
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        for profile in [0x333u32, 0x335, 0x33d] {
            let mut wrong = restart.clone();
            wrong[offset + 4..offset + 8].copy_from_slice(&((0x20 | profile << 6) << 11).to_le_bytes());
            assert_eq!(decode_with_residue(&wrong, Some(&previous)).len(), 4);
        }
        // A partially recognized prior generation cannot authorize residue.
        preceding[COMMAND_OFFSET + 3 * 40 + 8] = 1;
        let mut untrusted = RfHle::new(true);
        assert_eq!(untrusted.submit(preceding, 0, 0, 32), 4);
        assert_eq!(untrusted.previous_command, None);
        untrusted.clear();
        assert_eq!(untrusted.submit(restart, 1_000_000, 0, 32), 0);
    }
    #[test]
    fn observed_restart_scratch_residue_is_not_an_extra_measurement() {
        let bytes = restart_schedule();
        let previous = [0x00, 0x08, 0x15, 0x04, 0x00, 0x00, 0x67, 0x06];
        assert!(decode(&bytes).is_empty()); // no established earlier command
        let decoded = decode_with_residue(&bytes, Some(&previous));
        assert_eq!(decoded.len(), 5);
        assert_eq!(decoded.iter().map(|request| request.channel).collect::<Vec<_>>(),
            vec![527, 526, 525, 524, 523]);
        let mut hle = RfHle { enabled: true, previous_command: Some(previous), ..Default::default() };
        assert_eq!(hle.submit(bytes, 279_845_683, 11827, 32), 5);
        assert_eq!(hle.take_ready(280_011_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(280_127_488).len(), 4);
        assert!(!hle.busy());
        // Second observed restart repeats the same relationship: 707 ->708..712.
        hle.clear();
        let previous = ((0x21u32 | 707 << 6) << 11).to_le_bytes();
        let mut pair = [0; 8];
        pair[..4].copy_from_slice(&previous);
        pair[4..].copy_from_slice(&0x06670000u32.to_le_bytes());
        hle.previous_command = Some(pair);
        let mut bytes = restart_schedule();
        for index in 0..5 {
            let offset = COMMAND_OFFSET + (7 - index) * 40;
            let first = (0x21u32 | (708 + index as u32) << 6) << 11;
            bytes[offset..offset + 4].copy_from_slice(&first.to_le_bytes());
            bytes[offset + 8..offset + 16].copy_from_slice(&pair);
        }
        assert_eq!(hle.submit(bytes, 288_005_638, 11782, 32), 5);
        assert_eq!(hle.take_ready(288_171_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(288_287_488).len(), 4);
        hle.reset();
        assert!(hle.previous_command.is_none());
    }
    #[test]
    fn accepted_commands_supply_restart_history_without_replaying_idle_or_duplicates() {
        let mut hle = RfHle::new(true);
        let mut final_group = restart_schedule();
        for index in 0..5 {
            let offset = COMMAND_OFFSET + (7 - index) * 40;
            let first = (0x21u32 | (518 + index as u32) << 6) << 11;
            final_group[offset..offset + 4].copy_from_slice(&first.to_le_bytes());
            final_group[offset + 8..offset + 40].fill(0);
        }
        assert_eq!(hle.submit(final_group, 0, 0, 32), 5);
        let expected = [0x00, 0x08, 0x15, 0x04, 0x00, 0x00, 0x67, 0x06];
        assert_eq!(hle.previous_command, Some(expected));
        assert_eq!(hle.take_ready(293632).len(), 5);
        hle.clear();
        let mut empty = restart_schedule();
        empty[..4].fill(0);
        assert_eq!(hle.submit(empty, 320000, 0, 32), 0);
        assert_eq!(hle.previous_command, Some(expected));
        assert!(!hle.busy());
        assert_eq!(hle.submit(restart_schedule(), 480000, 0, 32), 5);
        assert_eq!(hle.submit(restart_schedule(), 480001, 0, 32), 0);
        assert_eq!(hle.take_ready(773632).len(), 5); // no extra residue measurement
        let accepted = hle.previous_command;
        hle.clear();
        let mut unsupported = schedule();
        let offset = COMMAND_OFFSET + 7 * 40 + 4;
        unsupported[offset..offset + 4].copy_from_slice(&0x06830000u32.to_le_bytes());
        assert_eq!(hle.submit(unsupported, 800000, 0, 32), 4);
        assert_eq!(hle.previous_command, accepted); // partial/unknown cannot replace history
    }
    #[test]
    fn restart_residue_exception_rejects_unobserved_extensions_and_profiles() {
        let offset = COMMAND_OFFSET + 7 * 40;
        let previous = [0x00, 0x08, 0x15, 0x04, 0x00, 0x00, 0x67, 0x06];
        for byte in offset + 8..offset + 40 {
            let mut bytes = restart_schedule();
            bytes[byte] ^= 1;
            assert_eq!(decode_with_residue(&bytes, Some(&previous)).len(), 4, "extension byte {byte}");
        }
        let mut bytes = restart_schedule();
        bytes[offset + 4..offset + 8].copy_from_slice(&0x066b0000u32.to_le_bytes());
        assert_eq!(decode_with_residue(&bytes, Some(&previous)).len(), 4); // profile0335
        for channel in [0u32, 522, 528, 1023] {
            let mut bytes = restart_schedule();
            let first = (0x21 | channel << 6) << 11;
            bytes[offset..offset + 4].copy_from_slice(&first.to_le_bytes());
            assert_eq!(decode_with_residue(&bytes, Some(&previous)).len(), 4);
        }
    }
    #[test]
    fn decodes_observed_mixed_gain_profiles() {
        let requests = decode(&profile0333_schedule());
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[0], Measurement { slot: 3, channel: 512,
            calibration: 0x333, finish: 4176 });
        assert_eq!(requests[4].channel, 1020);
        assert_eq!(requests[4].calibration, 0x335);
    }
    #[test]
    fn profile0333_still_rejects_unknown_shapes() {
        for (word, value) in [(0, 0x04010801u32), (0, 0x1c010800),
            (4, 0x06650000), (4, 0x06690000), (4, 0x06668000),
            (4, 0x06670001), (8, 1)] {
            let mut bytes = profile0333_schedule();
            let offset = COMMAND_OFFSET + 3 * 40 + word;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            let requests = decode(&bytes);
            assert_eq!(requests.len(), 4);
            assert!(requests.iter().all(|request| request.slot != 3));
        }
    }
    #[test]
    fn mixed_profiles_share_frame_timing_and_generation_lifetime() {
        let mut hle = RfHle::new(true);
        let now = 277_130_287;
        assert_eq!(hle.submit(profile0333_schedule(), now, 563 * 32, 32), 5);
        assert!(hle.take_ready(now).is_empty());
        assert_eq!(hle.take_ready(now + (5000 + 536 - 563) * 32), vec![(7, -400)]);
        assert_eq!(hle.take_ready(now + (5000 + 4176 - 563) * 32).len(), 4);
        assert_eq!(hle.submit(profile0333_schedule(), now, 0, 32), 0);
        hle.clear();
        assert_eq!(hle.submit(profile0333_schedule(), now, 0, 32), 5);
        hle.reset();
        assert!(hle.take_ready(u64::MAX).is_empty());
    }
    #[test]
    fn decodes_observed_five_slots() {
        let requests = decode(&schedule());
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[4], Measurement { slot: 7, channel: 0, calibration: 0x335, finish: 536 });
        assert_eq!(requests[0], Measurement { slot: 3, channel: 4, calibration: 0x335, finish: 4176 });
    }
    #[test]
    fn ignores_inactive_stale_records() {
        let mut bytes = schedule();
        put(&mut bytes, 0, 3);
        assert_eq!(decode(&bytes).len(), 1);
        put(&mut bytes, 0, 0);
        assert!(decode(&bytes).is_empty());
    }
    #[test]
    fn rejects_bad_length_and_mask() {
        assert!(decode(&[]).is_empty());
        let mut bytes = schedule();
        put(&mut bytes, 2, 1);
        assert!(decode(&bytes).is_empty());
    }
    #[test]
    fn unsupported_request_does_not_complete_its_slot() {
        for (offset, value) in [(4, 0xe501), (8, 1), (10, 0xe20b),
            (12, 37), (12, 5000), (COMMAND_OFFSET + 7 * 40, 0x801),
            (COMMAND_OFFSET + 7 * 40 + 8, 1)] {
            let mut bytes = schedule();
            put(&mut bytes, offset, value);
            let requests = decode(&bytes);
            assert_eq!(requests.len(), 4);
            assert!(requests.iter().all(|request| request.slot != 7));
        }
    }
    #[test]
    fn duplicate_records_and_unknown_band_are_rejected() {
        let mut bytes = schedule();
        put(&mut bytes, 0, 0x7ff);
        put(&mut bytes, 4 + 10 * 6, 0xe500);
        assert_eq!(decode(&bytes).len(), 4);
        bytes = schedule();
        bytes[COMMAND_OFFSET + 7 * 40 + 3] |= 8;
        assert_eq!(decode(&bytes).len(), 4);
        bytes = schedule();
        bytes[COMMAND_OFFSET + 7 * 40 + 5] ^= 2; // different calibration profile
        assert_eq!(decode(&bytes).len(), 4);
    }
    #[test]
    fn completion_is_delayed_and_once_per_generation() {
        let mut hle = RfHle { enabled: true, ..Default::default() };
        assert_eq!(hle.submit(schedule(), 100, 0, 32), 5);
        assert!(hle.take_ready(100 + (5000 + 536) * 32 - 1).is_empty());
        assert_eq!(hle.take_ready(100 + (5000 + 536) * 32), vec![(7, -400)]);
        assert_eq!(hle.submit(schedule(), 200, 0, 32), 0);
        assert_eq!(hle.take_ready(100 + (5000 + 4176) * 32).len(), 4);
        assert!(!hle.busy());
        assert!(hle.take_ready(u64::MAX).is_empty());
        assert_eq!(hle.submit(schedule(), 200, 0, 32), 0);
        let mut changed = schedule();
        changed[COMMAND_OFFSET + 7 * 40 + 1] ^= 2;
        assert_eq!(hle.submit(changed, 200, 0, 32), 0);
        hle.clear();
        assert_eq!(hle.submit(schedule(), 200, 0, 32), 5);
    }
    #[test]
    fn reset_cancels_work_and_default_is_off() {
        let mut hle = RfHle::default();
        assert_eq!(hle.submit(schedule(), 0, 0, 32), 0);
        hle.enabled = true;
        assert_eq!(hle.submit(schedule(), 0, 0, 32), 5);
        hle.reset();
        assert!(hle.take_ready(u64::MAX).is_empty());
    }

    #[test]
    fn observed_timer_phase_delivers_before_next_frame_collector() {
        let mut hle = RfHle::new(true);
        let now = 274_731_848;
        assert_eq!(hle.submit(schedule(), now, 2249 * 8, 32), 5);
        assert!(hle.take_ready(now - 1).is_empty());
        assert!(hle.take_ready(274_864_830).is_empty()); // producer frame
        hle.clear(); // ARM reuses the table; accepted work remains
        assert_eq!(hle.take_ready(274_891_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(275_007_488 - 1), vec![(4, -400), (5, -400), (6, -400)]);
        assert_eq!(hle.take_ready(275_007_488), vec![(3, -400)]);
        assert!(!hle.busy());
        assert!(hle.take_ready(275_024_830).is_empty());
    }

    #[test]
    fn phase_remainder_and_late_submission_keep_next_frame_deadlines() {
        let mut hle = RfHle::new(true);
        let now = 100_000;
        hle.submit(schedule(), now, 536 * 32 - 1, 32);
        assert!(hle.take_ready(now).is_empty());
        assert_eq!(hle.take_ready(now + 5000 * 32 + 1), vec![(7, -400)]);
        hle.reset();
        hle.submit(schedule(), now, 4999 * 32 + 31, 32);
        assert!(hle.take_ready(now + 536 * 32).is_empty());
        assert_eq!(hle.take_ready(now + 536 * 32 + 1), vec![(7, -400)]);
        assert_eq!(hle.take_ready(now + 4176 * 32 + 1).len(), 4);
        assert!(hle.take_ready(now + 5000 * 32).is_empty());
    }

    #[test]
    fn producer_clear_and_empty_cycle_drain_only_the_accepted_last_group() {
        let mut hle = RfHle::new(true);
        let now = 277_450_301;
        hle.submit(profile0333_schedule(), now, 16445, 32);
        assert!(hle.take_ready(277_584_626).is_empty());
        hle.clear(); // 277594124: producer clear before results are due
        let mut empty = profile0333_schedule();
        empty[..4].fill(0); // stale commands retained, but no new request
        assert_eq!(hle.submit(empty, 277_600_987, 0, 32), 0);
        assert!(hle.busy());
        assert_eq!(hle.take_ready(277_611_008), vec![(7, -400)]);
        assert_eq!(hle.take_ready(277_744_626).len(), 4);
        hle.clear();
        assert!(!hle.busy());
        assert!(hle.take_ready(u64::MAX).is_empty());
    }

    #[test]
    fn next_submission_does_not_replace_previous_in_flight_group() {
        let mut hle = RfHle::new(true);
        hle.submit(schedule(), 0, 0, 32);
        hle.clear();
        hle.submit(profile0333_schedule(), 160000, 0, 32);
        assert_eq!(hle.take_ready(177152), vec![(7, -400)]);
        assert_eq!(hle.take_ready(293632).len(), 4);
        assert!(hle.busy());
        assert_eq!(hle.take_ready(337152), vec![(7, -400)]);
        assert_eq!(hle.take_ready(453632).len(), 4);
        assert!(!hle.busy());
    }

    #[test]
    fn idle_control_does_not_latch_a_generation_or_block_scan_restart() {
        let mut hle = RfHle::new(true);
        let mut empty = schedule();
        empty[..4].fill(0);
        assert_eq!(hle.submit(empty.clone(), 0, 0, 32), 0);
        assert_eq!(hle.submit(empty, 160000, 0, 32), 0);
        assert!(hle.generation.is_none());
        assert!(!hle.busy());
        assert_eq!(hle.submit(profile0333_schedule(), 320000, 0, 32), 5);
        assert_eq!(hle.take_ready(497152), vec![(7, -400)]);
    }
}
