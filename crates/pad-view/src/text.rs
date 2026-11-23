//! Small text helpers that need no heap: a fixed line buffer, hex dumps,
//! names of HID usages and of Bluetooth device classes.

use core::fmt::{self, Write};

/// One line of text, at most `N` bytes. Writing past the end is cut off
/// quietly (never an error, never a panic).
#[derive(Clone)]
pub struct Line<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> Default for Line<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Line<N> {
    pub const fn new() -> Self {
        Line {
            buf: [0; N],
            len: 0,
        }
    }
    pub fn as_str(&self) -> &str {
        // Only whole ASCII or UTF-8 pieces are written (see write_str).
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    /// Replace the text.
    pub fn set(&mut self, s: &str) {
        self.clear();
        let _ = self.write_str(s);
    }
    /// A number that changes when the text changes.
    pub fn hash(&self) -> u32 {
        fnv(self.as_str().as_bytes())
    }
}

impl<const N: usize> Write for Line<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for ch in s.chars() {
            // The mono fonts only have ASCII; show anything else as '?'.
            let c = if ch.is_ascii() && !ch.is_ascii_control() {
                ch as u8
            } else {
                b'?'
            };
            if self.len >= N {
                break;
            }
            self.buf[self.len] = c;
            self.len += 1;
        }
        Ok(())
    }
}

/// FNV-1a: a quick hash, used to notice that something changed.
pub fn fnv(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Write bytes as hex pairs separated by spaces, as many as `max` bytes.
pub fn hex<W: Write>(w: &mut W, bytes: &[u8], max: usize) {
    for (i, b) in bytes.iter().take(max).enumerate() {
        if i > 0 {
            let _ = w.write_char(' ');
        }
        let _ = write!(w, "{:02X}", b);
    }
}

/// Write a value from -1.0 to 1.0 as "+0.53" without float formatting
/// (which would add several kilobytes to the firmware).
pub fn signed_2dp<W: Write>(w: &mut W, v: f32) {
    let mut hundredths = (v * 100.0) as i32;
    if v * 100.0 - hundredths as f32 >= 0.5 {
        hundredths += 1;
    } else if hundredths as f32 - v * 100.0 >= 0.5 {
        hundredths -= 1;
    }
    let sign = if hundredths < 0 { '-' } else { '+' };
    let a = hundredths.unsigned_abs();
    let _ = write!(w, "{}{}.{:02}", sign, a / 100, a % 100);
}

/// Write a number with a sign and `decimals` digits after the point
/// (0 to 3), rounded, for example "+1.00" or "-12.5", without float
/// formatting (see [`signed_2dp`]).
pub fn signed_fixed<W: Write>(w: &mut W, v: f32, decimals: u32) {
    let decimals = decimals.min(3);
    let scale = 10i64.pow(decimals);
    let x = v * scale as f32;
    // `as` stops at the ends of i64 and turns NaN into 0.
    let r = if x >= 0.0 {
        (x + 0.5) as i64
    } else {
        (x - 0.5) as i64
    };
    let sign = if r < 0 { '-' } else { '+' };
    let a = r.unsigned_abs();
    let s = scale as u64;
    let _ = write!(w, "{}{}", sign, a / s);
    if decimals > 0 {
        let _ = write!(w, ".{:0width$}", a % s, width = decimals as usize);
    }
}

/// A short name for a usage, at most 6 characters.
pub fn usage_name<W: Write>(w: &mut W, page: u16, usage: u16) {
    let s = match (page, usage) {
        (0x01, 0x30) => "X",
        (0x01, 0x31) => "Y",
        (0x01, 0x32) => "Z",
        (0x01, 0x33) => "Rx",
        (0x01, 0x34) => "Ry",
        (0x01, 0x35) => "Rz",
        (0x01, 0x36) => "Slider",
        (0x01, 0x37) => "Dial",
        (0x01, 0x38) => "Wheel",
        (0x01, 0x39) => "Hat",
        (0x01, 0x85) => "SysMnu",
        (0x01, 0x90) => "DUp",
        (0x01, 0x91) => "DDown",
        (0x01, 0x92) => "DRight",
        (0x01, 0x93) => "DLeft",
        (0x02, 0xC4) => "Gas",
        (0x02, 0xC5) => "Brake",
        (0x06, 0x20) => "Batt",
        (0x0C, 0x40) => "Menu",
        (0x0C, 0xB2) => "Record",
        (0x0C, 0x223) => "ACHome",
        (0x0C, 0x224) => "ACBack",
        (0x09, n) => {
            let _ = write!(w, "B{}", n);
            return;
        }
        (p, n) if p >= 0xFF00 => {
            let _ = write!(w, "V{:02X}", n & 0xFF);
            return;
        }
        (p, n) => {
            let _ = write!(w, "{:X}:{:X}", p, n);
            return;
        }
    };
    let _ = w.write_str(s);
}

/// The class of device in words, for example "Peripheral: gamepad".
///
/// The 24-bit class has a major class in bits 8-12 and a minor class in
/// bits 2-7 (Bluetooth Assigned Numbers, "Class of Device").
pub fn class_words<W: Write>(w: &mut W, cod: u32) {
    let major = (cod >> 8) & 0x1F;
    let minor = (cod >> 2) & 0x3F;
    let m = match major {
        0 => "Misc",
        1 => "Computer",
        2 => "Phone",
        3 => "Network",
        4 => "Audio/video",
        5 => "Peripheral",
        6 => "Imaging",
        7 => "Wearable",
        8 => "Toy",
        9 => "Health",
        0x1F => "Uncategorised",
        _ => "Reserved",
    };
    let _ = w.write_str(m);
    if major == 5 {
        // Peripheral: bits 2-5 say gamepad, joystick...; bits 6-7 keyboard
        // and pointing device.
        let kind = match minor & 0x0F {
            0 => "",
            1 => "joystick",
            2 => "gamepad",
            3 => "remote",
            4 => "sensor",
            5 => "digitizer",
            6 => "card reader",
            _ => "other",
        };
        let kb = minor & 0x10 != 0;
        let ptr = minor & 0x20 != 0;
        let _ = w.write_str(": ");
        if kb {
            let _ = w.write_str("keyboard ");
        }
        if ptr {
            let _ = w.write_str("pointer ");
        }
        let _ = w.write_str(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_cuts_off() {
        let mut l: Line<4> = Line::new();
        let _ = write!(l, "abcdef");
        assert_eq!(l.as_str(), "abcd");
        l.set("é1");
        assert_eq!(l.as_str(), "?1");
    }

    #[test]
    fn two_decimals() {
        let mut l: Line<16> = Line::new();
        for (v, s) in [
            (0.534, "+0.53"),
            (-0.536, "-0.54"),
            (1.0, "+1.00"),
            (-1.0, "-1.00"),
            (0.0, "+0.00"),
        ] {
            l.clear();
            signed_2dp(&mut l, v);
            assert_eq!(l.as_str(), s);
        }
    }

    #[test]
    fn fixed_decimals() {
        let mut l: Line<16> = Line::new();
        for (v, d, s) in [
            (1.0, 2, "+1.00"),
            (-0.005, 2, "-0.01"),
            (12.34, 1, "+12.3"),
            (-2293.7, 0, "-2294"),
            (0.0, 1, "+0.0"),
        ] {
            l.clear();
            signed_fixed(&mut l, v, d);
            assert_eq!(l.as_str(), s);
        }
    }

    #[test]
    fn gamepad_class() {
        let mut l: Line<40> = Line::new();
        class_words(&mut l, 0x000508);
        assert_eq!(l.as_str(), "Peripheral: gamepad");
        l.clear();
        class_words(&mut l, 0x002508);
        assert_eq!(l.as_str(), "Peripheral: gamepad");
    }
}
