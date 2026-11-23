//! Handing finished pixels to the board.
//!
//! The board asks for a short run of panel pixels: a piece of one panel
//! row or one panel column, in either direction. [`Source::run`] makes
//! them straight from the small picture: it reads each picture pixel
//! once, looks up its colour once, and writes it `scale` times. No full
//! panel picture (150 KB) ever exists.

use board_api::{ByteOrder, Dir, Indexed, PixelSource, Rect};

use crate::frame::{Frame, H, W};
use crate::palette::Palette;

/// Where the picture sits on a panel: shown `scale` times bigger, with
/// its top-left corner at panel pixel `(left, top)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub scale: u8,
    pub left: u16,
    pub top: u16,
}

impl View {
    /// The biggest whole-number scale that fits the panel, centred.
    /// 320x240 gives scale 2 at (0, 0).
    pub const fn fit(panel_w: u16, panel_h: u16) -> View {
        let sx = panel_w / W as u16;
        let sy = panel_h / H as u16;
        let s = if sx < sy { sx } else { sy };
        let s = if s == 0 { 1 } else { s };
        View {
            scale: s as u8,
            left: panel_w.saturating_sub(W as u16 * s) / 2,
            top: panel_h.saturating_sub(H as u16 * s) / 2,
        }
    }

    /// A rectangle of the picture, in panel pixels.
    pub const fn to_panel(&self, r: Rect) -> Rect {
        r.scaled(self.scale as u16, self.left, self.top)
    }

    /// The picture pixel under panel pixel `(x, y)`, or `None` in the
    /// border. Used to turn a touch into picture pixels.
    pub const fn to_picture(&self, x: u16, y: u16) -> Option<(u16, u16)> {
        let k = self.scale as u16;
        if x < self.left || y < self.top {
            return None;
        }
        let (px, py) = ((x - self.left) / k, (y - self.top) / k);
        if px as usize >= W || py as usize >= H {
            return None;
        }
        Some((px, py))
    }
}

/// The [`PixelSource`] the renderer hands to `Screen::present`: a
/// picture, its palette, where it sits, and the border colour. It holds
/// only references and a few numbers (about 20 bytes).
pub struct Source<'a> {
    pub frame: &'a Frame,
    pub palette: &'a Palette,
    pub view: View,
    /// RGB565 colour of panel pixels outside the picture.
    pub border: u16,
}

/// One run, turned into "forward" form: it covers panel positions
/// `start .. start + n` along its line, whichever way the board walks.
struct Line {
    /// First panel position along the line (the smallest).
    start: usize,
    n: usize,
    /// Where the picture starts along the line, in panel pixels, and
    /// how many picture pixels it has along it.
    lo: usize,
    len: usize,
    /// The picture row (for a row run) or column (for a column run) the
    /// line crosses, or `None` when it misses the picture (border only).
    fixed: Option<usize>,
    horizontal: bool,
}

/// How a run's inside part splits: `head` copies of a first picture
/// pixel cut by the run's start, `whole` pixels written `k` times each,
/// then `tail` copies of a last pixel cut by the run's end.
#[derive(Clone, Copy)]
struct Split {
    head: usize,
    whole: usize,
    tail: usize,
    k: usize,
}

impl<'a> Source<'a> {
    pub fn new(frame: &'a Frame, palette: &'a Palette, view: View, border: u16) -> Self {
        Source {
            frame,
            palette,
            view,
            border,
        }
    }

    /// The finished colour of one panel pixel (the slow, simple way;
    /// tests compare [`PixelSource::run`] against it).
    pub fn panel_pixel(&self, x: u16, y: u16) -> u16 {
        match self.view.to_picture(x, y) {
            Some((px, py)) => self.palette.get(self.frame.get(px, py)),
            None => self.border,
        }
    }

    /// Fills `out`, which holds the run's pixels in forward order
    /// (`REV` = false) or backward order (`REV` = true).
    fn fill<const SWAP: bool, const REV: bool>(&self, l: &Line, out: &mut [u16]) {
        let border = if SWAP {
            self.border.swap_bytes()
        } else {
            self.border
        };
        let Some(f) = l.fixed else {
            out.fill(border);
            return;
        };
        let k = usize::from(self.view.scale.max(1));
        // Forward positions: [start, pic0) border, [pic0, pic1) picture,
        // [pic1, end) border.
        let end = l.start + l.n;
        let pic0 = l.lo.clamp(l.start, end);
        let pic1 = (l.lo + l.len * k).clamp(pic0, end);
        let (before, inside) = (pic0 - l.start, pic1 - pic0);
        // In backward order the border "before" sits at the end of `out`.
        let (b_lo, ins, b_hi) = if REV {
            let (hi, rest) = out.split_at_mut(end - pic1);
            let (ins, lo) = rest.split_at_mut(inside);
            (lo, ins, hi)
        } else {
            let (lo, rest) = out.split_at_mut(before);
            let (ins, hi) = rest.split_at_mut(inside);
            (lo, ins, hi)
        };
        b_lo.fill(border);
        b_hi.fill(border);
        if inside == 0 {
            return;
        }
        // The first picture pixel touched, and how many of its copies
        // fall before the run's start (`phase`).
        let off = pic0 - l.lo;
        let (p, phase) = (off / k, off % k);
        let head = ((k - phase) % k).min(inside);
        let whole = (inside - head) / k;
        let s = Split {
            head,
            whole,
            tail: inside - head - whole * k,
            k,
        };
        // The picture pixels along the line, from pixel `p`. Each way is
        // a plain slice walk, so the loop has no bounds checks.
        if l.horizontal {
            let row = &self.frame.0[f * W..][..W];
            self.put::<SWAP, REV, _>(ins, s, row[p..].iter().copied(), border);
        } else {
            let col = self.frame.0[p * W..]
                .as_chunks::<W>()
                .0
                .iter()
                .map(|r| r[f]);
            self.put::<SWAP, REV, _>(ins, s, col, border);
        }
    }

    /// Writes the inside part of a run from a stream of picture pixels.
    #[inline(always)]
    fn put<const SWAP: bool, const REV: bool, I: Iterator<Item = u8>>(
        &self,
        ins: &mut [u16],
        s: Split,
        pixels: I,
        border: u16,
    ) {
        let pal = self.palette.colours();
        let mut px = pixels.map(|i| {
            let c = pal[usize::from(i)];
            if SWAP { c.swap_bytes() } else { c }
        });
        let n = ins.len();
        let (h, rest) = if REV {
            let (r, h) = ins.split_at_mut(n - s.head);
            (h, r)
        } else {
            ins.split_at_mut(s.head)
        };
        if s.head > 0 {
            h.fill(px.next().unwrap_or(border));
        }
        let (mid, t) = if REV {
            let (t, m) = rest.split_at_mut(s.tail);
            (m, t)
        } else {
            rest.split_at_mut(s.whole * s.k)
        };
        if s.k == 2 {
            // The common case, 160x120 on 320x240: one load, one lookup,
            // two stores per picture pixel.
            let (pairs, _) = mid.as_chunks_mut::<2>();
            if REV {
                for (o, c) in pairs.iter_mut().rev().zip(px.by_ref()) {
                    *o = [c, c];
                }
            } else {
                for (o, c) in pairs.iter_mut().zip(px.by_ref()) {
                    *o = [c, c];
                }
            }
        } else if REV {
            for (o, c) in mid.rchunks_exact_mut(s.k).zip(px.by_ref()) {
                o.fill(c);
            }
        } else {
            for (o, c) in mid.chunks_exact_mut(s.k).zip(px.by_ref()) {
                o.fill(c);
            }
        }
        if s.tail > 0 {
            t.fill(px.next().unwrap_or(border));
        }
    }
}

impl PixelSource for Source<'_> {
    fn run(&self, x: u16, y: u16, dir: Dir, order: ByteOrder, out: &mut [u16]) {
        let n = out.len();
        if n == 0 {
            return;
        }
        let v = self.view;
        let k = usize::from(v.scale.max(1));
        let (x, y) = (usize::from(x), usize::from(y));
        let horizontal = matches!(dir, Dir::Right | Dir::Left);
        // The fixed coordinate (the row of a row run, the column of a
        // column run) decides whether the line meets the picture at all.
        let (fixed, f_lo, f_len, along, a_lo, a_len) = if horizontal {
            (y, v.top, H, x, v.left, W)
        } else {
            (x, v.left, W, y, v.top, H)
        };
        let f_lo = usize::from(f_lo);
        let fixed = (fixed >= f_lo && fixed < f_lo + f_len * k).then(|| (fixed - f_lo) / k);
        let backward = matches!(dir, Dir::Left | Dir::Up);
        // A backward run that would pass position 0 is outside the panel
        // (the board never asks for one); give border rather than fail.
        let Some(start) = (if backward {
            (along + 1).checked_sub(n)
        } else {
            Some(along)
        }) else {
            out.fill(order.apply(self.border));
            return;
        };
        let line = Line {
            start,
            n,
            lo: usize::from(a_lo),
            len: a_len,
            fixed,
            horizontal,
        };
        let swap = order.apply(0x00FF) != 0x00FF;
        match (swap, backward) {
            (false, false) => self.fill::<false, false>(&line, out),
            (false, true) => self.fill::<false, true>(&line, out),
            (true, false) => self.fill::<true, false>(&line, out),
            (true, true) => self.fill::<true, true>(&line, out),
        }
    }

    fn indexed(&self) -> Option<Indexed<'_>> {
        Some(Indexed {
            pixels: &self.frame.0,
            width: W as u16,
            height: H as u16,
            scale: self.view.scale,
            left: self.view.left,
            top: self.view.top,
            palette: self.palette.colours(),
            palette_version: self.palette.version(),
            border: self.border,
        })
    }
}
