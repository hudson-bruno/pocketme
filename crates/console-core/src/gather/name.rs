//! The name as small tables, all built while compiling from the one
//! constant [`crate::theme::NAME`] (the name home writes) and
//! console-video's font: its ink pixels at font size ("cells"), left to
//! right, and when each cell is written. In flash on the chip
//! (POCKETME: 85 cells, 5 x 8 + 85 x 2 + 85 x 2 bytes).

use console_video::Glyph;

use super::{LETTER_GAP, LETTER_SPREAD};
use crate::theme::NAME;

/// Height of every letter (console-video's font).
pub const FONT_H: usize = console_video::FONT_H as usize;

/// The font's letter for byte `c`, as `Frame::text` draws it.
const fn glyph_of(c: u8) -> Glyph {
    console_video::glyph_for(c)
}

/// Width of a letter, without the empty column after it.
const fn glyph_w(g: Glyph) -> usize {
    console_video::glyph_w(g) as usize
}

/// Is pixel `(i, j)` of the letter ink?
const fn glyph_ink(g: Glyph, i: usize, j: usize) -> bool {
    console_video::glyph_ink(g, i as u16, j as u16)
}

/// How many ink pixels a letter has.
const fn ink_count(g: Glyph) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < glyph_w(g) {
        let mut j = 0;
        while j < FONT_H {
            if glyph_ink(g, i, j) {
                n += 1;
            }
            j += 1;
        }
        i += 1;
    }
    n
}

const fn name_bytes() -> &'static [u8] {
    let b = NAME.as_bytes();
    let mut i = 0;
    while i < b.len() {
        assert!(b[i].is_ascii(), "NAME: ASCII letters only");
        i += 1;
    }
    b
}

/// Width of the name at font size (as console-video's `text_width`).
pub const NAME_W: usize = {
    let b = name_bytes();
    let mut w = 0;
    let mut i = 0;
    while i < b.len() {
        w += glyph_w(glyph_of(b[i])) + 1;
        i += 1;
    }
    assert!(
        w >= 2 && w - 1 <= 64,
        "NAME: 1 to 64 pixels wide at font size"
    );
    w - 1
};

/// The name at font size: row `j`, bit `i` is column `i`.
pub static ROWS: [u64; FONT_H] = {
    let b = name_bytes();
    let mut rows = [0u64; FONT_H];
    let (mut x, mut i) = (0, 0);
    while i < b.len() {
        let g = glyph_of(b[i]);
        let mut cx = 0;
        while cx < glyph_w(g) {
            let mut cy = 0;
            while cy < FONT_H {
                if glyph_ink(g, cx, cy) {
                    rows[cy] |= 1 << (x + cx);
                }
                cy += 1;
            }
            cx += 1;
        }
        x += glyph_w(g) + 1;
        i += 1;
    }
    rows
};

/// Number of ink pixels of the name at font size.
pub const CELLS: usize = {
    let b = name_bytes();
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        n += ink_count(glyph_of(b[i]));
        i += 1;
    }
    assert!(n > 0, "NAME: needs at least one ink pixel");
    n
};

/// Number of letters with ink (the ones written one after the other).
pub const LETTERS: usize = {
    let b = name_bytes();
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        if ink_count(glyph_of(b[i])) > 0 {
            n += 1;
        }
        i += 1;
    }
    n
};

/// Each cell's place at font size (x, y), column by column from the
/// left, top to bottom in a column.
pub static CELL_XY: [[u8; 2]; CELLS] = {
    let b = name_bytes();
    let mut t = [[0u8; 2]; CELLS];
    let (mut x, mut i, mut n) = (0, 0, 0);
    while i < b.len() {
        let g = glyph_of(b[i]);
        let mut cx = 0;
        while cx < glyph_w(g) {
            let mut cy = 0;
            while cy < FONT_H {
                if glyph_ink(g, cx, cy) {
                    t[n] = [(x + cx) as u8, cy as u8];
                    n += 1;
                }
                cy += 1;
            }
            cx += 1;
        }
        x += glyph_w(g) + 1;
        i += 1;
    }
    t
};

/// When each cell's points leave the figure, in ms after the stream
/// starts: letter `k` starts at `k * LETTER_GAP`, and its own cells
/// follow one another over `LETTER_SPREAD`, left to right.
pub static CELL_T: [u16; CELLS] = {
    let b = name_bytes();
    let mut t = [0u16; CELLS];
    let (mut i, mut n, mut k) = (0, 0, 0);
    while i < b.len() {
        let g = glyph_of(b[i]);
        let ink = ink_count(g);
        if ink > 0 {
            let mut j = 0;
            while j < ink {
                t[n] = (k * LETTER_GAP as usize + j * LETTER_SPREAD as usize / ink) as u16;
                n += 1;
                j += 1;
            }
            k += 1;
        }
        i += 1;
    }
    t
};

/// Is the name's pixel (i, j) at font size ink? Off the name is not.
#[inline(always)]
pub fn ink(i: i32, j: i32) -> bool {
    (0..NAME_W as i32).contains(&i)
        && (0..FONT_H as i32).contains(&j)
        && ROWS[j as usize] >> i & 1 != 0
}
