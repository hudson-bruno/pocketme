//! Short texts built from letters and numbers without `core::fmt` (it
//! costs chip code), drawn letter by letter (so no UTF-8 check is
//! linked either). From MERCATOR's `hud.rs`.

use game_kit::Frame;

/// Letters per text (the longest log line, a ramp result with every
/// number at its largest, is about 190; the console's log takes 255).
pub const CAP: usize = 200;

/// A text of at most [`CAP`] letters (plain ASCII).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Txt {
    b: [u8; CAP],
    n: u8,
}

impl Default for Txt {
    fn default() -> Self {
        Txt::new()
    }
}

impl Txt {
    pub const fn new() -> Txt {
        Txt { b: [0; CAP], n: 0 }
    }

    pub fn clear(&mut self) {
        self.n = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    #[inline(never)]
    pub fn s(&mut self, s: &str) -> &mut Txt {
        for &c in s.as_bytes() {
            if usize::from(self.n) < CAP {
                self.b[usize::from(self.n)] = c;
                self.n += 1;
            }
        }
        self
    }

    #[inline(never)]
    pub fn num(&mut self, mut v: u32) -> &mut Txt {
        let mut d = [0u8; 10];
        let mut k = 0;
        loop {
            d[k] = b'0' + (v % 10) as u8;
            k += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            if usize::from(self.n) < CAP {
                self.b[usize::from(self.n)] = d[k];
                self.n += 1;
            }
        }
        self
    }

    /// A number that may be below zero.
    pub fn int(&mut self, v: i32) -> &mut Txt {
        if v < 0 {
            self.s("-");
        }
        self.num(v.unsigned_abs())
    }

    /// `v` hundredths as "12.34".
    pub fn hundredths(&mut self, v: u32) -> &mut Txt {
        self.num(v / 100).s(".");
        if v % 100 < 10 {
            self.s("0");
        }
        self.num(v % 100)
    }

    /// For tests and tools.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(self.bytes()).unwrap_or("?")
    }

    /// The text's letters.
    pub fn bytes(&self) -> &[u8] {
        self.b.get(..usize::from(self.n)).unwrap_or(&[])
    }

    /// Draws the text at (x, y), `k` times as large.
    #[inline(never)]
    pub fn at(&self, f: &mut Frame, x: i16, y: i16, c: u8, k: u8) -> i16 {
        let mut x = x;
        for &ch in self.bytes() {
            x = f.text_scaled(x, y, letter(ch), c, k);
        }
        x
    }
}

/// The printable ASCII letters, space to tilde: a letter kept as a byte
/// is drawn or logged as its one-letter slice of this.
const ASCII: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

/// The one-letter text for byte `b` ("?" for anything not printable).
#[inline(never)]
pub fn letter(b: u8) -> &'static str {
    let i = usize::from(b.wrapping_sub(32));
    ASCII.get(i..i.wrapping_add(1)).unwrap_or("?")
}
