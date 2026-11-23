//! Small fixed-size containers, so the crate needs no heap.

use core::fmt;

/// The largest HCI packet the radio driver (esp-radio) takes from the host,
/// with the one-byte packet type in front. Every command and every data
/// (ACL) fragment we send fits in this.
pub const MAX_PACKET: usize = 259;

/// One HCI packet on its way to the controller, packet type byte included
/// (0x01 command, 0x02 ACL data).
#[derive(Clone, Copy)]
pub struct Packet {
    len: u16,
    data: [u8; MAX_PACKET],
}

impl Default for Packet {
    fn default() -> Self {
        Self::new()
    }
}

impl Packet {
    pub const fn new() -> Self {
        Self {
            len: 0,
            data: [0; MAX_PACKET],
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data[..usize::from(self.len)]
    }

    pub fn len(&self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends bytes. Returns false (and appends nothing) if they do not fit.
    pub fn put(&mut self, bytes: &[u8]) -> bool {
        let at = usize::from(self.len);
        if at + bytes.len() > MAX_PACKET {
            return false;
        }
        self.data[at..at + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len() as u16;
        true
    }

    pub fn put_u8(&mut self, b: u8) -> bool {
        self.put(&[b])
    }

    pub fn put_u16(&mut self, v: u16) -> bool {
        self.put(&v.to_le_bytes())
    }

    /// Writes over two bytes already in the packet (for a length field
    /// that is only known at the end).
    pub fn set_u16(&mut self, at: usize, v: u16) {
        self.data[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    pub fn set_u8(&mut self, at: usize, v: u8) {
        self.data[at] = v;
    }
}

/// A first-in first-out queue of `N` copyable things.
pub struct Ring<T: Copy, const N: usize> {
    items: [T; N],
    head: usize,
    len: usize,
}

impl<T: Copy, const N: usize> Ring<T, N> {
    /// `fill` only fills the empty slots; it is never handed out.
    pub const fn new(fill: T) -> Self {
        Self {
            items: [fill; N],
            head: 0,
            len: 0,
        }
    }

    /// Adds at the back. Gives the item back if the queue is full.
    pub fn push(&mut self, item: T) -> Result<(), T> {
        if self.len == N {
            return Err(item);
        }
        self.items[(self.head + self.len) % N] = item;
        self.len += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let item = self.items[self.head];
        self.head = (self.head + 1) % N;
        self.len -= 1;
        Some(item)
    }

    pub fn front(&self) -> Option<&T> {
        (self.len > 0).then(|| &self.items[self.head])
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

/// A short text built without a heap (for status lines). What does not
/// fit is cut off.
#[derive(Clone, Copy)]
pub struct Text<const N: usize> {
    len: usize,
    data: [u8; N],
}

impl<const N: usize> Default for Text<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Text<N> {
    pub const fn new() -> Self {
        Self {
            len: 0,
            data: [0; N],
        }
    }

    /// Takes bytes from a device (a name), keeping printable ASCII only and
    /// putting '?' for anything else, so it is safe in a log line.
    pub fn from_device_bytes(raw: &[u8]) -> Self {
        let mut t = Self::new();
        for &b in raw {
            if b == 0 {
                break;
            }
            let c = if (0x20..0x7F).contains(&b) && b != b'"' && b != b'\\' {
                b
            } else {
                b'?'
            };
            if t.len == N {
                break;
            }
            t.data[t.len] = c;
            t.len += 1;
        }
        t
    }

    pub fn as_str(&self) -> &str {
        // Only whole ASCII or whole UTF-8 pieces are ever written.
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl<const N: usize> fmt::Write for Text<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            let mut b = [0; 4];
            let piece = c.encode_utf8(&mut b).as_bytes();
            if self.len + piece.len() > N {
                return Err(fmt::Error);
            }
            self.data[self.len..self.len + piece.len()].copy_from_slice(piece);
            self.len += piece.len();
        }
        Ok(())
    }
}

/// Prints bytes as hex, two digits each, no spaces ("a1003f").
pub struct Hex<'a>(pub &'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// Prints a device's bytes (a name) as text, printable ASCII only.
pub struct Printable<'a>(pub &'a [u8]);

impl fmt::Display for Printable<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in self.0 {
            if b == 0 {
                break;
            }
            let c = if (0x20..0x7F).contains(&b) && b != b'"' && b != b'\\' {
                b as char
            } else {
                '?'
            };
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

/// Reads little-endian numbers from a byte slice, with bounds checks.
pub fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

pub fn u24_at(b: &[u8], at: usize) -> Option<u32> {
    Some(
        u32::from(*b.get(at)?) | u32::from(*b.get(at + 1)?) << 8 | u32::from(*b.get(at + 2)?) << 16,
    )
}

pub fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
        *b.get(at + 2)?,
        *b.get(at + 3)?,
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;

    #[test]
    fn ring_is_fifo_and_bounded() {
        let mut r: Ring<u8, 3> = Ring::new(0);
        assert!(r.push(1).is_ok());
        assert!(r.push(2).is_ok());
        assert!(r.push(3).is_ok());
        assert_eq!(r.push(4), Err(4));
        assert_eq!(r.pop(), Some(1));
        assert!(r.push(5).is_ok());
        assert_eq!(r.pop(), Some(2));
        assert_eq!(r.pop(), Some(3));
        assert_eq!(r.pop(), Some(5));
        assert_eq!(r.pop(), None);
    }

    #[test]
    fn packet_refuses_overflow() {
        let mut p = Packet::new();
        assert!(p.put(&[0; MAX_PACKET]));
        assert!(!p.put_u8(1));
        assert_eq!(p.len(), MAX_PACKET);
    }

    #[test]
    fn text_cuts_and_cleans() {
        let t: Text<5> = Text::from_device_bytes(b"Ab\"\x01cdef");
        assert_eq!(t.as_str(), "Ab??c");
        let mut t: Text<4> = Text::new();
        let _ = write!(t, "{}", 123456);
        assert_eq!(t.as_str(), "1234");
        let mut t: Text<8> = Text::new();
        let _ = write!(t, "x{}", 12);
        assert_eq!(t.as_str(), "x12");
    }
}
