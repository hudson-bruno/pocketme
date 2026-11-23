//! Finding what changed between two pictures.
//!
//! The rule is the one on the design page (its `present()`, "areas"
//! way), so the simulator in the browser and this code send the same
//! rectangles for the same frames. A test checks this against a file
//! made by running the design page's own code.
//!
//! 1. Each row is compared 4 pixels at a time (one 32-bit word). A
//!    changed row gives a span: from its first changed pixel to its last.
//! 2. Going down, a row joins the rectangle above it only if that is
//!    cheaper than opening a new one: the bigger rectangle may cost at
//!    most [`WINDOW_COST_PX`] pixels more than the two apart. A row with
//!    no change closes the rectangle.
//! 3. If more than [`MAX_AREAS`] are left, the two neighbours whose
//!    joining wastes the fewest pixels are joined, again and again.
//!
//! Older Rust code (experiments/display-dma/src/dirty.rs) did it a bit
//! differently; the crate README says where and why.

use board_api::Rect;

use crate::frame::{Frame, H, W};

/// Most changed rectangles sent in one frame. Every rectangle costs a
/// window command on the wire: about 50 us each, measured on the CYD at
/// both 40 and 80 MHz. 16 of them cost at most about 0.8 ms. The board
/// may still settle 8 by measurement. This is the one place to change.
pub const MAX_AREAS: usize = 16;

/// What opening one more rectangle is worth, in picture pixels. A row
/// joins the rectangle above it when the grown rectangle covers at most
/// this many pixels more than the rectangle and the row's span apart.
/// 24 is the design page's number. The measured cost of a window (about
/// 50 us) is worth about 31 picture pixels at 40 MHz and 62 at 80 MHz,
/// so 24 errs on the side of more, smaller rectangles.
pub const WINDOW_COST_PX: u32 = 24;

/// The changed rectangles of one frame, in picture pixels (160x120).
/// Fixed size: 8 bytes x [`MAX_AREAS`] (128 bytes), plus the count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Areas {
    rects: [Rect; MAX_AREAS],
    len: usize,
}

impl Areas {
    pub const fn new() -> Self {
        Areas {
            rects: [Rect::EMPTY; MAX_AREAS],
            len: 0,
        }
    }

    /// One rectangle: the whole picture.
    pub const fn full() -> Self {
        let mut a = Areas::new();
        a.rects[0] = Rect::new(0, 0, W as u16, H as u16);
        a.len = 1;
        a
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Adds a rectangle. Returns false (and adds nothing) when the list
    /// already holds [`MAX_AREAS`]; the caller joins rectangles first.
    pub fn push(&mut self, r: Rect) -> bool {
        if self.len == MAX_AREAS {
            return false;
        }
        self.rects[self.len] = r;
        self.len += 1;
        true
    }

    pub fn as_slice(&self) -> &[Rect] {
        &self.rects[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Picture pixels covered (the rectangles never overlap).
    pub fn pixels(&self) -> u32 {
        self.as_slice().iter().map(Rect::area).sum()
    }
}

impl Default for Areas {
    fn default() -> Self {
        Self::new()
    }
}

/// Words (of 4 pixels) in one row.
const WORDS: usize = W / 4;

/// Row `y` of the frame seen as 40 groups of 4 pixels, so two groups can
/// be compared in one go. The frame is aligned to 4 bytes and every row
/// starts at a multiple of 4, so on the chip each comparison is one
/// 32-bit load per side, not four byte loads (checked in the chip's
/// assembly; see the crate README). The fallback after `unwrap_or` is
/// never used: a row is always exactly 40 groups.
#[inline(always)]
fn words(f: &Frame, y: usize) -> &[[u8; 4]; WORDS] {
    let (w, _) = f.0[y * W..][..W].as_chunks::<4>();
    w.try_into().unwrap_or(&[[0; 4]; WORDS])
}

/// First and last changed pixel of row `y` (both included), or `None`.
/// Compares whole words, then looks inside only the first and the last
/// changed word.
#[inline(always)]
fn row_span(a: &Frame, b: &Frame, y: usize) -> Option<(usize, usize)> {
    let (wa, wb) = (words(a, y), words(b, y));
    let first = (0..WORDS).find(|&i| wa[i] != wb[i])?;
    // A difference exists, so the backward search stops at `first`.
    let last = (first..WORDS)
        .rev()
        .find(|&i| wa[i] != wb[i])
        .unwrap_or(first);
    let (p, q) = (wa[first], wb[first]);
    let lo = (0..4).find(|&i| p[i] != q[i]).unwrap_or(0);
    let (p, q) = (wa[last], wb[last]);
    let hi = (0..4).rev().find(|&i| p[i] != q[i]).unwrap_or(3);
    Some((first * 4 + lo, last * 4 + hi))
}

/// One rectangle while it is being built: x, y, right and bottom (both
/// "first one outside"). 4 bytes, because 160 and 120 fit in a byte.
#[derive(Clone, Copy)]
struct Band {
    x: u8,
    y: u8,
    r: u8,
    b: u8,
}

impl Band {
    fn w(self) -> u32 {
        u32::from(self.r - self.x)
    }
    fn h(self) -> u32 {
        u32::from(self.b - self.y)
    }
    fn area(self) -> u32 {
        self.w() * self.h()
    }
    /// Pixels sent for nothing if `self` and the next band `n` (below
    /// it) became one rectangle.
    fn waste(self, n: Band) -> u32 {
        let w = u32::from(self.r.max(n.r) - self.x.min(n.x));
        w * u32::from(n.b - self.y) - self.area() - n.area()
    }
    fn join(self, n: Band) -> Band {
        Band {
            x: self.x.min(n.x),
            y: self.y,
            r: self.r.max(n.r),
            b: n.b,
        }
    }
}

/// Compares the picture on the panel (`shown`) with the new one (`cur`)
/// and writes the changed rectangles into `out` (cleared first), top to
/// bottom, in picture pixels. Every changed pixel is inside one of
/// them; they never overlap; at most [`MAX_AREAS`].
///
/// Work memory, on the stack only during the call: one band per row at
/// most (120 x 4 bytes) and the waste between neighbours (120 x 2
/// bytes): 720 bytes.
pub fn diff(shown: &Frame, cur: &Frame, out: &mut Areas) {
    out.clear();
    let mut bands = [Band {
        x: 0,
        y: 0,
        r: 0,
        b: 0,
    }; H];
    let mut n = 0;
    let mut open = false;
    for y in 0..H {
        let Some((a, b)) = row_span(shown, cur, y) else {
            open = false;
            continue;
        };
        // Spans fit a byte: 0..=159 and 1..=160.
        let row = Band {
            x: a as u8,
            y: y as u8,
            r: b as u8 + 1,
            b: y as u8 + 1,
        };
        if open {
            let c = bands[n - 1];
            let x0 = c.x.min(row.x);
            let r = c.r.max(row.r);
            let grown = u32::from(r - x0) * (c.h() + 1);
            if grown <= c.area() + row.w() + WINDOW_COST_PX {
                bands[n - 1] = Band {
                    x: x0,
                    y: c.y,
                    r,
                    b: row.b,
                };
                continue;
            }
        }
        bands[n] = row;
        n += 1;
        open = true;
    }

    if n > MAX_AREAS {
        // The waste of joining each band with the next one, kept up to
        // date so each join costs one search, not a fresh count. At most
        // 19,200 pixels, so it fits a u16.
        let mut waste = [0u16; H];
        for i in 0..n - 1 {
            waste[i] = bands[i].waste(bands[i + 1]) as u16;
        }
        while n > MAX_AREAS {
            // The first smallest, as the design page picks it.
            let mut best = 0;
            for i in 1..n - 1 {
                if waste[i] < waste[best] {
                    best = i;
                }
            }
            bands[best] = bands[best].join(bands[best + 1]);
            bands.copy_within(best + 2..n, best + 1);
            if best + 2 < n - 1 {
                waste.copy_within(best + 2..n - 1, best + 1);
            }
            n -= 1;
            if best + 1 < n {
                waste[best] = bands[best].waste(bands[best + 1]) as u16;
            }
            if best > 0 {
                waste[best - 1] = bands[best - 1].waste(bands[best]) as u16;
            }
        }
    }

    for band in &bands[..n] {
        out.push(Rect::new(
            band.x.into(),
            band.y.into(),
            band.w() as u16,
            band.h() as u16,
        ));
    }
}
