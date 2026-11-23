//! Small drawing helpers shared by the screens, and the console's pixel
//! art (written as text, turned into pixels while compiling).

use console_video::{CLEAR, Frame, Palette, W, text_width};

use crate::theme::{self, GOLD, INK, LINE, MUTE, OK, PANEL, PAPER, WARN};

/// Puts the console's 8 colours into palette numbers 240 to 247.
pub fn set_console_colours(p: &mut Palette) {
    for (i, &hex) in theme::UI_COLOURS.iter().enumerate() {
        p.set(INK + i as u8, theme::rgb(hex));
    }
}

/// Width of `text` drawn `scale` times bigger.
pub fn width(text: &str, scale: u8) -> i16 {
    // Saturating, and never past i16::MAX: a long text must not wrap to
    // a negative width.
    let w = text_width(text).saturating_mul(u16::from(scale.max(1)));
    w.min(i16::MAX as u16) as i16
}

/// Writes `text` centred on column `cx`, `scale` times bigger.
pub fn text_centred(f: &mut Frame, cx: i16, y: i16, text: &str, c: u8, scale: u8) {
    f.text_scaled(cx - width(text, scale) / 2, y, text, c, scale);
}

/// Writes `text` so that it ends at column `right` (exclusive).
pub fn text_right(f: &mut Frame, right: i16, y: i16, text: &str, c: u8) {
    f.text(right - width(text, 1), y, text, c);
}

/// A frame `thick` pixels wide around the rectangle `(x, y, w, h)`
/// (drawn outside it).
pub fn ring(f: &mut Frame, x: i16, y: i16, w: u16, h: u16, thick: u16, c: u8) {
    for i in 1..=thick {
        let d = i as i16;
        f.outline(x - d, y - d, w + 2 * i, h + 2 * i, c);
    }
}

/// A small fixed-size text (no heap): at most `N` bytes, cut on a
/// character boundary, and only printable ASCII (anything else becomes
/// `?`, which is also what the font would draw).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text<const N: usize> {
    bytes: [u8; N],
    len: u8,
}

impl<const N: usize> Text<N> {
    pub const fn new() -> Self {
        Text {
            bytes: [0; N],
            len: 0,
        }
    }

    /// Takes the first `N` characters of `s`.
    pub fn of(s: &str) -> Self {
        let mut t = Self::new();
        for c in s.chars().take(N) {
            t.bytes[usize::from(t.len)] = if c.is_ascii_graphic() || c == ' ' {
                c as u8
            } else {
                b'?'
            };
            t.len += 1;
        }
        t
    }

    /// Takes the first `N` bytes of `b`; a byte that is not a printable
    /// letter becomes `?` (for bytes read back from lent RAM, which may
    /// be rubbish).
    pub fn of_ascii(b: &[u8]) -> Self {
        let mut t = Self::new();
        for &c in b.iter().take(N) {
            t.bytes[usize::from(t.len)] = if c.is_ascii_graphic() || c == b' ' {
                c
            } else {
                b'?'
            };
            t.len += 1;
        }
        t
    }

    /// `a` then `b`, cut to `N` characters in all.
    pub fn joined(a: &str, b: &str) -> Self {
        let mut t = Self::of(a);
        let more = Text::<N>::of(b);
        let room = N - usize::from(t.len);
        let n = usize::from(more.len).min(room);
        let at = usize::from(t.len);
        t.bytes[at..at + n].copy_from_slice(&more.bytes[..n]);
        t.len += n as u8;
        t
    }

    pub fn as_str(&self) -> &str {
        // Only ASCII is ever stored, so this cannot fail.
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const N: usize> Default for Text<N> {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Pixel art --------------------------------------------------------------

/// A palette number that a sprite drawn "tinted" replaces with one
/// colour (for one-colour icons).
pub const TINT: u8 = 254;

/// Turns rows of letters into palette numbers, while compiling:
/// `.` see-through, `#` tint, and one letter per console colour
/// (k ink, p panel, w paper, g gold, m mute, o ok, r warn, l line).
const fn art<const N: usize>(rows: &[&str]) -> [u8; N] {
    let mut out = [CLEAR; N];
    let mut n = 0;
    let mut r = 0;
    while r < rows.len() {
        let b = rows[r].as_bytes();
        let mut i = 0;
        while i < b.len() {
            out[n] = match b[i] {
                b'.' => CLEAR,
                b'#' => TINT,
                b'k' => INK,
                b'p' => PANEL,
                b'w' => PAPER,
                b'g' => GOLD,
                b'm' => MUTE,
                b'o' => OK,
                b'r' => WARN,
                b'l' => LINE,
                _ => panic!("art: unknown letter"),
            };
            n += 1;
            i += 1;
        }
        r += 1;
    }
    assert!(n == N, "art: wrong number of pixels");
    out
}

/// A picture made with [`art`]: `w` wide, `pixels.len() / w` tall.
pub struct Art {
    pub w: u16,
    pub h: u16,
    pub pixels: &'static [u8],
}

impl Art {
    /// Draws the art with each pixel as a `k x k` square. Only the first
    /// `rows` rows (the boot screen draws the icon row by row). `TINT`
    /// pixels take colour `tint`.
    pub fn draw(&self, f: &mut Frame, x: i16, y: i16, k: u16, rows: u16, tint: u8) {
        let rows = rows.min(self.h);
        for j in 0..rows {
            for i in 0..self.w {
                let c = self.pixels[usize::from(j * self.w + i)];
                if c == CLEAR {
                    continue;
                }
                let c = if c == TINT { tint } else { c };
                let (px, py) = (x + (i * k) as i16, y + (j * k) as i16);
                if k == 1 {
                    f.pixel(px, py, c);
                } else {
                    f.rect(px, py, k, k, c);
                }
            }
        }
    }
}

/// The little handheld on the boot screen: 16 x 16, drawn at 2x.
/// Its screen (`l`) lights up at the end of the boot animation; see
/// [`HANDHELD_SCREEN`].
pub const HANDHELD: Art = Art {
    w: 16,
    h: 16,
    pixels: &art::<256>(&[
        "..wwwwwwwwwwww..",
        ".wmmmmmmmmmmmmw.",
        ".wmkkkkkkkkkkmw.",
        ".wmkllllllllkmw.",
        ".wmkllllllllkmw.",
        ".wmkllllllllkmw.",
        ".wmkllllllllkmw.",
        ".wmkkkkkkkkkkmw.",
        ".wmmmmmmmmmmmmw.",
        ".wmmkmmmmmmmmmw.",
        ".wmkkkmmmmmrmmw.",
        ".wmmkmmmmmgmmmw.",
        ".wmmmmmmmmmmmmw.",
        ".wmmmmmmmlmlmmw.",
        "..wwwwwwwwwwww..",
        "................",
    ]),
};

/// Where the handheld's screen is, in art pixels: x, y, w, h.
pub const HANDHELD_SCREEN: (u16, u16, u16, u16) = (4, 3, 8, 4);

/// A gamepad, 11 x 7, one colour (the bar's "pad connected" mark).
pub const PAD_ICON: Art = Art {
    w: 11,
    h: 7,
    pixels: &art::<77>(&[
        ".#########.",
        "###########",
        "##.#####.##",
        "#...###.#.#",
        "##.#####.##",
        "###.....###",
        ".#.......#.",
    ]),
};

/// An SD card, 7 x 8, one colour (the "no card" screen, drawn at 3x).
pub const CARD_ICON: Art = Art {
    w: 7,
    h: 8,
    pixels: &art::<56>(&[
        "#####..", "######.", "#######", "#######", "#.....#", "#######", "#######", "#######",
    ]),
};

/// A gear, 9 x 9, one colour: the home screen's menu button, at the
/// right end of the bar.
pub const GEAR_ICON: Art = Art {
    w: 9,
    h: 9,
    pixels: &art::<81>(&[
        "...###...",
        ".#.###.#.",
        "..#####..",
        "###...###",
        "###...###",
        "###...###",
        "..#####..",
        ".#.###.#.",
        "...###...",
    ]),
};

/// A small sun, 7 x 7, one colour: the BRIGHTNESS item of the menu.
pub const SUN_ICON: Art = Art {
    w: 7,
    h: 7,
    pixels: &art::<49>(&[
        "...#...", ".#...#.", "..###..", "#.###.#", "..###..", ".#...#.", "...#...",
    ]),
};

/// Places of the few still stars behind the home screen (never moving,
/// so they cost nothing to send after the first frame).
pub const STARS: [(i16, i16); 10] = [
    (12, 16),
    (40, 13),
    (150, 30),
    (122, 15),
    (6, 70),
    (153, 76),
    (24, 98),
    (138, 96),
    (70, 12),
    (100, 100),
];

/// Draws the stars in a quiet colour.
pub fn stars(f: &mut Frame) {
    for &(x, y) in &STARS {
        f.pixel(x, y, LINE);
    }
}

/// The centre column of the picture.
pub const CX: i16 = (W / 2) as i16;
