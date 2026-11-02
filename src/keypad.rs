//! E830 keyboard controller at 0x50308000, as used by ISR 0x208e51ac.
//! Coordinates come from the original firmware's initialized key table.
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PhoneKey {
    One, Two, Three, Four, Five, Six, Seven, Eight, Nine, Zero, Star, Hash,
    SoftLeft, SoftRight, Call, End, Up, Down, Left, Right, Ok,
}

impl PhoneKey {
    pub const ALL: [Self; 21] = [Self::One, Self::Two, Self::Three, Self::Four,
        Self::Five, Self::Six, Self::Seven, Self::Eight, Self::Nine, Self::Zero,
        Self::Star, Self::Hash, Self::SoftLeft, Self::SoftRight, Self::Call,
        Self::End, Self::Up, Self::Down, Self::Left, Self::Right, Self::Ok];

    pub const fn bit(self) -> u32 { 1 << self as u8 }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "1" => Self::One, "2" => Self::Two, "3" => Self::Three,
            "4" => Self::Four, "5" => Self::Five, "6" => Self::Six,
            "7" => Self::Seven, "8" => Self::Eight, "9" => Self::Nine,
            "0" => Self::Zero, "star" | "*" => Self::Star, "hash" | "#" => Self::Hash,
            "soft-left" => Self::SoftLeft, "soft-right" => Self::SoftRight,
            "call" => Self::Call, "end" => Self::End, "up" => Self::Up,
            "down" => Self::Down, "left" => Self::Left, "right" => Self::Right,
            "ok" => Self::Ok, _ => return None,
        })
    }

    fn coordinates(self) -> u16 {
        let (row, column) = match self {
            Self::One => (1, 8), Self::Two => (2, 16), Self::Three => (2, 8),
            Self::Four => (1, 4), Self::Five => (1, 16), Self::Six => (2, 4),
            Self::Seven => (1, 2), Self::Eight => (4, 2), Self::Nine => (2, 2),
            Self::Zero => (2, 1), Self::Star => (1, 1), Self::Hash => (4, 1),
            Self::SoftLeft => (8, 4), Self::SoftRight => (8, 8),
            Self::Call => (8, 2), Self::End => (16, 2), Self::Up => (8, 1),
            Self::Down => (16, 24), Self::Left => (20, 17),
            Self::Right => (20, 18), Self::Ok => (20, 20),
        };
        row | column << 5
    }
}

#[derive(Default)]
pub(crate) struct Keypad {
    control: u8,
    held: u32,
    events: VecDeque<(u8, u16)>,
    overflow: bool,
}

impl Keypad {
    pub fn set_keys(&mut self, keys: u32) {
        let keys = keys & ((1 << PhoneKey::ALL.len()) - 1);
        for key in PhoneKey::ALL {
            if (self.held ^ keys) & key.bit() != 0 {
                if self.events.len() < 64 {
                    self.events.push_back((if keys & key.bit() != 0 { 1 } else { 2 }, key.coordinates()));
                } else { self.overflow = true; }
            }
        }
        self.held = keys;
    }

    pub fn irq_pending(&self) -> bool {
        self.events.front().is_some_and(|(event, _)| self.control & event != 0)
            || self.overflow && self.control & 4 != 0
    }

    pub fn read(&self, offset: u32, size: usize) -> u64 {
        let raw = match offset & !3 {
            0 => u32::from(self.control),
            4 => self.events.front().map_or(0, |(_, data)| u32::from(*data)),
            // ISR checks 0=asserted and writes ones to acknowledge.
            8 => 7 & !self.events.front().map_or(0, |(event, _)| u32::from(*event))
                & if self.overflow { !4 } else { u32::MAX },
            _ => 0,
        };
        (u64::from(raw) >> ((offset & 3) * 8)) & match size { 1 => 0xff, 2 => 0xffff, _ => 0xffff_ffff }
    }

    pub fn write(&mut self, offset: u32, value: u64) {
        match offset {
            0 => {
                self.control = value as u8;
                if self.control == 0x18 { self.events.clear(); self.overflow = false; }
            }
            8 => {
                if self.events.front().is_some_and(|(event, _)| value as u8 & event != 0) {
                    self.events.pop_front();
                }
                if value & 4 != 0 { self.overflow = false; }
            }
            _ => {} // key data is external input, not writable RAM
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_release_are_latched_masked_and_acknowledged_once() {
        let mut keypad = Keypad::default();
        assert_eq!(keypad.read(8, 1), 7);
        keypad.set_keys(PhoneKey::One.bit());
        assert!(!keypad.irq_pending());
        keypad.write(0, 3);
        assert!(keypad.irq_pending());
        assert_eq!(keypad.read(4, 2), 1 | 8 << 5);
        assert_eq!(keypad.read(8, 1), 6);
        keypad.set_keys(PhoneKey::One.bit());
        keypad.set_keys(0);
        assert_eq!(keypad.read(8, 1), 6);
        keypad.write(8, 2);
        assert_eq!(keypad.read(8, 1), 6);
        keypad.write(8, 7);
        assert_eq!(keypad.read(8, 1), 5);
        keypad.write(8, 7);
        assert!(!keypad.irq_pending());
        assert_eq!(keypad.read(8, 1), 7);
    }

    #[test]
    fn multiple_keys_release_and_read_only_coordinates() {
        let mut keypad = Keypad::default();
        keypad.write(0, 3);
        keypad.set_keys(PhoneKey::One.bit() | PhoneKey::Ok.bit());
        keypad.write(4, 0xffff);
        assert_eq!(keypad.read(4, 1), (1 | 8 << 5) & 255);
        assert_eq!(keypad.read(5, 1), 1);
        keypad.write(8, 7);
        assert_eq!(keypad.read(4, 2), 20 | 20 << 5);
        keypad.set_keys(0);
        for _ in 0..3 { assert!(keypad.irq_pending()); keypad.write(8, 7); }
        assert!(!keypad.irq_pending());
    }

    #[test]
    fn reset_and_bounded_overflow() {
        let mut keypad = Keypad::default();
        for _ in 0..100 { keypad.set_keys(PhoneKey::One.bit()); keypad.set_keys(0); }
        assert_eq!(keypad.events.len(), 64);
        keypad.write(0, 4);
        assert!(keypad.irq_pending());
        keypad.write(8, 4);
        assert!(!keypad.irq_pending());
        keypad.write(0, 0x18);
        assert_eq!(keypad.read(8, 1), 7);
    }
}
