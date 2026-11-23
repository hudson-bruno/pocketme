//! From the console's logical screen to bytes on the wire.
//!
//! The logical screen is 160x120 with one byte per pixel: a number into a
//! 256-colour palette. It is shown at 2x on the 320x240 panel: every
//! logical pixel becomes 2x2 panel pixels.
//!
//! The palette is kept ready to send:
//!
//! * 16-bit: each entry is a 32-bit word holding the colour twice, bytes
//!   already in wire order (the panel wants the high byte first; the
//!   ESP32 stores the low byte first). One logical pixel is then one load
//!   and one 32-bit store, and the doubling across is free.
//! * 12-bit: two equal panel pixels take 3 bytes (R G | B R | G B). Each
//!   entry holds those 3 bytes; four logical pixels make three 32-bit
//!   words, put together with shifts.
//!
//! The doubling down is a copy: each logical row is converted once into
//! the strip buffer and copied right after itself.

use crate::lcd::Depth;

pub const LW: usize = 160;
pub const LH: usize = 120;

/// A logical screen. Aligned to 4 so rows can be compared as words.
#[repr(C, align(4))]
pub struct Frame(pub [u8; LW * LH]);

impl Frame {
    pub const fn new() -> Self {
        Self([0; LW * LH])
    }

    pub fn row(&self, y: usize) -> &[u8] {
        &self.0[y * LW..][..LW]
    }

    /// The same pixels as 32-bit words (40 per row).
    pub fn words(&self) -> &[u32] {
        // SAFETY: the array is 4-aligned (repr align) and 19,200 bytes is
        // a whole number of words; every bit pattern is a valid u32.
        unsafe { core::slice::from_raw_parts(self.0.as_ptr().cast::<u32>(), LW * LH / 4) }
    }
}

/// The 256 colours in the forms the senders need.
pub struct Palette {
    /// RGB565, as games and screenshots see it.
    pub rgb565: [u16; 256],
    /// 16-bit wire form: the colour twice, bytes swapped.
    pub wire16: [u32; 256],
    /// 12-bit wire form: the 3 bytes of two equal pixels, in the low 24
    /// bits, first byte lowest.
    pub wire12: [u32; 256],
}

impl Palette {
    pub const fn new() -> Self {
        Self {
            rgb565: [0; 256],
            wire16: [0; 256],
            wire12: [0; 256],
        }
    }

    pub fn set_all(&mut self, colours: &[u16; 256]) {
        for (i, &c) in colours.iter().enumerate() {
            self.set(i, c);
        }
    }

    pub fn set(&mut self, i: usize, rgb565: u16) {
        self.rgb565[i] = rgb565;
        let swapped = u32::from(rgb565.swap_bytes());
        self.wire16[i] = swapped | (swapped << 16);
        let [r, g, b] = to_444(rgb565);
        let (r, g, b) = (u32::from(r), u32::from(g), u32::from(b));
        let b0 = (r << 4) | g;
        let b1 = (b << 4) | r;
        let b2 = (g << 4) | b;
        self.wire12[i] = b0 | (b1 << 8) | (b2 << 16);
    }
}

/// The 4-bit channels a RGB565 colour becomes in 12-bit mode: the top
/// four bits of each.
pub fn to_444(c: u16) -> [u8; 3] {
    [
        (c >> 12) as u8,
        ((c >> 7) & 0xF) as u8,
        ((c >> 1) & 0xF) as u8,
    ]
}

pub fn rgb565_to_444(c: u16) -> u16 {
    let [r, g, b] = to_444(c);
    (u16::from(r) << 8) | (u16::from(g) << 4) | u16::from(b)
}

/// Views a strip buffer as words. DMA buffers from esp-hal are 4-aligned.
fn words_mut(buf: &mut [u8]) -> &mut [u32] {
    // SAFETY: any bit pattern is a valid u32; `align_to_mut` only hands
    // out the aligned middle part.
    let (head, words, _) = unsafe { buf.align_to_mut::<u32>() };
    assert!(head.is_empty(), "strip buffer not 4-aligned");
    words
}

/// Bytes on the wire for one logical row segment of `lw` pixels, shown
/// doubled across (one panel row).
pub fn row_bytes(depth: Depth, lw: usize) -> usize {
    match depth {
        Depth::Rgb565 => lw * 4,
        Depth::Rgb444 => lw * 3,
    }
}

/// Converts logical rows `y0..y0 + rows`, columns `x0..x0 + lw`, into
/// `dst`, each row twice (once per panel row). Returns the bytes
/// written. In 12-bit mode `lw` must be a multiple of 4.
#[inline(never)]
pub fn fill(
    depth: Depth,
    fb: &Frame,
    pal: &Palette,
    y0: usize,
    rows: usize,
    x0: usize,
    lw: usize,
    dst: &mut [u8],
) -> usize {
    match depth {
        Depth::Rgb565 => fill16(fb, &pal.wire16, y0, rows, x0, lw, dst),
        Depth::Rgb444 => fill12(fb, &pal.wire12, y0, rows, x0, lw, dst),
    }
}

#[esp_hal::ram] // in RAM: no flash cache misses; ram also means not inlined
fn fill16(
    fb: &Frame,
    pal: &[u32; 256],
    y0: usize,
    rows: usize,
    x0: usize,
    lw: usize,
    dst: &mut [u8],
) -> usize {
    let words = words_mut(dst);
    let mut at = 0;
    for y in y0..y0 + rows {
        let src = &fb.0[y * LW + x0..][..lw];
        let (first, rest) = words[at..at + 2 * lw].split_at_mut(lw);
        for (out, &index) in first.iter_mut().zip(src) {
            // u8 into a 256-entry table: no bounds check is generated.
            *out = pal[usize::from(index)];
        }
        rest.copy_from_slice(first);
        at += 2 * lw;
    }
    at * 4
}

#[esp_hal::ram] // in RAM: no flash cache misses; ram also means not inlined
fn fill12(
    fb: &Frame,
    pal: &[u32; 256],
    y0: usize,
    rows: usize,
    x0: usize,
    lw: usize,
    dst: &mut [u8],
) -> usize {
    debug_assert!(lw % 4 == 0);
    let words = words_mut(dst);
    let per_row = lw * 3 / 4;
    let mut at = 0;
    for y in y0..y0 + rows {
        let src = &fb.0[y * LW + x0..][..lw];
        let (first, rest) = words[at..at + 2 * per_row].split_at_mut(per_row);
        for (out, four) in first.chunks_exact_mut(3).zip(src.chunks_exact(4)) {
            let p0 = pal[usize::from(four[0])];
            let p1 = pal[usize::from(four[1])];
            let p2 = pal[usize::from(four[2])];
            let p3 = pal[usize::from(four[3])];
            // Memory order is wire order: the low byte of each word goes
            // first. p0's three bytes, then p1's, and so on.
            out[0] = p0 | (p1 << 24);
            out[1] = (p1 >> 8) | (p2 << 16);
            out[2] = (p2 >> 16) | (p3 << 8);
        }
        rest.copy_from_slice(first);
        at += 2 * per_row;
    }
    at * 4
}

/// Bytes on the wire for one logical column segment of `h` pixels, sent
/// transposed (one panel gate line, see [`fill_cols`]).
pub fn col_bytes(depth: Depth, h: usize) -> usize {
    row_bytes(depth, h)
}

/// The transposed form of [`fill`], for the columns way (`cols.rs`):
/// logical columns `x0..x0 + cols`, rows `y0..y0 + h`. Each logical
/// column becomes two equal panel gate lines; one gate line is the
/// column's `h` pixels top to bottom, each doubled along the line. With
/// `rev_x` the columns come right to left, with `rev_y` each column
/// bottom to top. Returns the bytes written. In 12-bit mode `h` must be
/// a multiple of 4.
///
/// The same work per pixel as the row fill (one byte load, one palette
/// load, one store, then a copy of the whole line), but the bytes are
/// read down a column, 160 apart. Internal RAM has no cache, so the
/// stride costs nothing more than an add.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn fill_cols(
    depth: Depth,
    fb: &Frame,
    pal: &Palette,
    x0: usize,
    cols: usize,
    y0: usize,
    h: usize,
    rev_x: bool,
    rev_y: bool,
    dst: &mut [u8],
) -> usize {
    match depth {
        Depth::Rgb565 => fill16_cols(fb, &pal.wire16, x0, cols, y0, h, rev_x, rev_y, dst),
        Depth::Rgb444 => fill12_cols(fb, &pal.wire12, x0, cols, y0, h, rev_x, rev_y, dst),
    }
}

/// Index of the first pixel of logical column number `k` of a block,
/// and the step from one pixel to the next along the gate line (`LW` or
/// minus `LW`, as a wrapping add).
#[inline(always)]
fn col_start(
    x0: usize,
    cols: usize,
    y0: usize,
    h: usize,
    k: usize,
    rev_x: bool,
    rev_y: bool,
) -> (usize, usize) {
    let x = if rev_x { x0 + cols - 1 - k } else { x0 + k };
    if rev_y {
        ((y0 + h - 1) * LW + x, LW.wrapping_neg())
    } else {
        (y0 * LW + x, LW)
    }
}

#[esp_hal::ram] // in RAM: no flash cache misses; ram also means not inlined
#[allow(clippy::too_many_arguments)]
fn fill16_cols(
    fb: &Frame,
    pal: &[u32; 256],
    x0: usize,
    cols: usize,
    y0: usize,
    h: usize,
    rev_x: bool,
    rev_y: bool,
    dst: &mut [u8],
) -> usize {
    let words = words_mut(dst);
    let mut at = 0;
    for k in 0..cols {
        let (mut i, step) = col_start(x0, cols, y0, h, k, rev_x, rev_y);
        let (first, rest) = words[at..at + 2 * h].split_at_mut(h);
        for out in first.iter_mut() {
            // The colour twice in one word: the doubling along the line.
            *out = pal[usize::from(fb.0[i])];
            i = i.wrapping_add(step);
        }
        // The second gate line of this logical column.
        rest.copy_from_slice(first);
        at += 2 * h;
    }
    at * 4
}

#[esp_hal::ram] // in RAM: no flash cache misses; ram also means not inlined
#[allow(clippy::too_many_arguments)]
fn fill12_cols(
    fb: &Frame,
    pal: &[u32; 256],
    x0: usize,
    cols: usize,
    y0: usize,
    h: usize,
    rev_x: bool,
    rev_y: bool,
    dst: &mut [u8],
) -> usize {
    debug_assert!(h % 4 == 0);
    let words = words_mut(dst);
    // One logical pixel doubled along the line is exactly one 3-byte
    // pair, so a gate line is 3h bytes, whole words when h is a
    // multiple of 4.
    let per_line = h * 3 / 4;
    let mut at = 0;
    for k in 0..cols {
        let (mut i, step) = col_start(x0, cols, y0, h, k, rev_x, rev_y);
        let (first, rest) = words[at..at + 2 * per_line].split_at_mut(per_line);
        for out in first.chunks_exact_mut(3) {
            let p0 = pal[usize::from(fb.0[i])];
            i = i.wrapping_add(step);
            let p1 = pal[usize::from(fb.0[i])];
            i = i.wrapping_add(step);
            let p2 = pal[usize::from(fb.0[i])];
            i = i.wrapping_add(step);
            let p3 = pal[usize::from(fb.0[i])];
            i = i.wrapping_add(step);
            // As in fill12: memory order is wire order.
            out[0] = p0 | (p1 << 24);
            out[1] = (p1 >> 8) | (p2 << 16);
            out[2] = (p2 >> 16) | (p3 << 8);
        }
        rest.copy_from_slice(first);
        at += 2 * per_line;
    }
    at * 4
}

/// How many logical rows of `lw` pixels fit in one strip of `strip_rows`
/// (the strip rows are counted for full-width rows), at least 1.
pub fn rows_per_strip(depth: Depth, lw: usize, strip_rows: usize) -> usize {
    let budget = strip_rows * 2 * row_bytes(depth, LW);
    (budget / (2 * row_bytes(depth, lw))).max(1)
}

/// A rectangle in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub const FULL: Rect = Rect {
        x: 0,
        y: 0,
        w: LW as u16,
        h: LH as u16,
    };

    /// The panel window (inclusive corners) this rectangle covers at 2x.
    pub fn panel_window(&self) -> (u16, u16, u16, u16) {
        (
            self.x * 2,
            self.y * 2,
            (self.x + self.w) * 2 - 1,
            (self.y + self.h) * 2 - 1,
        )
    }

    /// Widened to whole groups of 4 logical pixels across, as 12-bit
    /// mode needs (4 pixels = 3 words).
    pub fn to_quads(self) -> Rect {
        let x0 = self.x & !3;
        let x1 = (self.x + self.w).div_ceil(4) * 4;
        Rect {
            x: x0,
            y: self.y,
            w: x1.min(LW as u16) - x0,
            h: self.h,
        }
    }

    /// Widened to whole groups of 4 logical pixels down, as 12-bit mode
    /// needs for the columns way (a column of 4 pixels = 3 words).
    pub fn to_quads_y(self) -> Rect {
        let y0 = self.y & !3;
        let y1 = (self.y + self.h).div_ceil(4) * 4;
        Rect {
            x: self.x,
            y: y0,
            w: self.w,
            h: y1.min(LH as u16) - y0,
        }
    }
}

/// Sends a rectangle of the logical screen. `strip_rows` sets the strip
/// size (in full-width logical rows).
pub fn present_rect(
    lcd: &mut crate::lcd::Lcd,
    fb: &Frame,
    pal: &Palette,
    rect: Rect,
    strip_rows: usize,
    how: crate::lcd::Send,
) -> Result<crate::lcd::Timing, crate::lcd::LcdError> {
    let depth = lcd.depth();
    let rect = if depth == Depth::Rgb444 {
        rect.to_quads()
    } else {
        rect
    };
    let (x0, lw) = (usize::from(rect.x), usize::from(rect.w));
    let per = rows_per_strip(depth, lw, strip_rows);
    let h = usize::from(rect.h);
    let strips = h.div_ceil(per);
    let y0 = usize::from(rect.y);
    lcd.draw(rect.panel_window(), strips, how, |k, buf| {
        let first = y0 + k * per;
        let rows = per.min(y0 + h - first);
        fill(depth, fb, pal, first, rows, x0, lw, buf)
    })
}

/// Sends the whole logical screen.
pub fn present_full(
    lcd: &mut crate::lcd::Lcd,
    fb: &Frame,
    pal: &Palette,
    strip_rows: usize,
    how: crate::lcd::Send,
) -> Result<crate::lcd::Timing, crate::lcd::LcdError> {
    present_rect(lcd, fb, pal, Rect::FULL, strip_rows, how)
}
