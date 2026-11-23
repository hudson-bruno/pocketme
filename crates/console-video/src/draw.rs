//! Drawing helpers on the picture: lines, filled rectangles, sprites and
//! text. They are what the console (and later the games) draw with.
//!
//! Positions are signed (`i16`), so a sprite or a word can stand partly
//! off the picture: the part outside is simply not drawn. Colours are
//! palette numbers. Nothing here allocates or uses floating point.

use crate::font::{FONT_H, glyph_ink, glyph_of, glyph_w};
use crate::frame::{Frame, H, W};

/// The palette number the design page uses as "see-through" in sprites.
/// Only a habit: [`Frame::sprite`] takes the see-through number you give.
pub const CLEAR: u8 = 255;

/// A small picture to copy into the frame: `w x h` palette numbers, row
/// after row. It only borrows its pixels, so sprites can live in flash
/// (`static` or `const` arrays) and cost no RAM.
#[derive(Clone, Copy, Debug)]
pub struct Sprite<'a> {
    w: u16,
    h: u16,
    pixels: &'a [u8],
}

impl<'a> Sprite<'a> {
    /// A sprite of `w x h` pixels. Stops with an error if `pixels` does
    /// not hold exactly `w * h` of them (while compiling, for a `const`).
    pub const fn new(w: u16, h: u16, pixels: &'a [u8]) -> Self {
        assert!(pixels.len() == w as usize * h as usize, "sprite size");
        Sprite { w, h, pixels }
    }

    pub const fn w(&self) -> u16 {
        self.w
    }

    pub const fn h(&self) -> u16 {
        self.h
    }

    pub const fn pixels(&self) -> &'a [u8] {
        self.pixels
    }
}

/// The part of `[start, start + len)` inside `[0, max)`, or `None`.
#[inline(always)]
fn clip(start: i16, len: u16, max: usize) -> Option<(usize, usize)> {
    let a = i32::from(start).max(0);
    let b = (i32::from(start) + i32::from(len)).min(max as i32);
    (a < b).then_some((a as usize, b as usize))
}

impl Frame {
    /// Sets one pixel. Outside the picture: nothing happens.
    #[inline]
    pub fn pixel(&mut self, x: i16, y: i16, c: u8) {
        if (0..W as i16).contains(&x) && (0..H as i16).contains(&y) {
            self.0[y as usize * W + x as usize] = c;
        }
    }

    /// A line `w` pixels long going right from `(x, y)`.
    pub fn hline(&mut self, x: i16, y: i16, w: u16, c: u8) {
        self.rect(x, y, w, 1, c);
    }

    /// A line `h` pixels long going down from `(x, y)`.
    pub fn vline(&mut self, x: i16, y: i16, h: u16, c: u8) {
        self.rect(x, y, 1, h, c);
    }

    /// A filled rectangle with its top-left corner at `(x, y)`.
    pub fn rect(&mut self, x: i16, y: i16, w: u16, h: u16, c: u8) {
        let (Some((x0, x1)), Some((y0, y1))) = (clip(x, w, W), clip(y, h, H)) else {
            return;
        };
        for row in self.0[y0 * W..y1 * W].as_chunks_mut::<W>().0 {
            row[x0..x1].fill(c);
        }
    }

    /// The outline of a rectangle, one pixel thick.
    pub fn outline(&mut self, x: i16, y: i16, w: u16, h: u16, c: u8) {
        if w == 0 || h == 0 {
            return;
        }
        self.hline(x, y, w, c);
        self.hline(x, y.saturating_add((h - 1) as i16), w, c);
        self.vline(x, y, h, c);
        self.vline(x.saturating_add((w - 1) as i16), y, h, c);
    }

    /// Copies a sprite with its top-left corner at `(x, y)`. Pixels equal
    /// to `see_through` are skipped, so the picture behind shows.
    pub fn sprite(&mut self, x: i16, y: i16, s: &Sprite<'_>, see_through: u8) {
        self.copy_sprite(x, y, s, Some(see_through));
    }

    /// Copies a sprite with every pixel drawn (no see-through colour).
    pub fn blit(&mut self, x: i16, y: i16, s: &Sprite<'_>) {
        self.copy_sprite(x, y, s, None);
    }

    fn copy_sprite(&mut self, x: i16, y: i16, s: &Sprite<'_>, key: Option<u8>) {
        let (Some((x0, x1)), Some((y0, y1))) = (clip(x, s.w, W), clip(y, s.h, H)) else {
            return;
        };
        let sw = usize::from(s.w);
        // Where the visible part starts inside the sprite.
        let sx = (x0 as i32 - i32::from(x)) as usize;
        let sy = (y0 as i32 - i32::from(y)) as usize;
        let n = x1 - x0;
        let src_rows = s.pixels[sy * sw..].chunks_exact(sw);
        let dst_rows = self.0[y0 * W..y1 * W].as_chunks_mut::<W>().0.iter_mut();
        for (src, dst) in src_rows.zip(dst_rows) {
            let (src, dst) = (&src[sx..sx + n], &mut dst[x0..x1]);
            match key {
                None => dst.copy_from_slice(src),
                Some(k) => {
                    for (d, &p) in dst.iter_mut().zip(src) {
                        if p != k {
                            *d = p;
                        }
                    }
                }
            }
        }
    }

    /// Writes `text` in the built-in 5-pixel font with its top-left
    /// corner at `(x, y)`, in colour `c`. Returns the x where a next
    /// letter would start. Small letters come out as capitals.
    pub fn text(&mut self, x: i16, y: i16, text: &str, c: u8) -> i16 {
        self.text_scaled(x, y, text, c, 1)
    }

    /// The same, with every font pixel drawn as a `scale x scale` square
    /// (the design page uses 2 for titles). Scale 0 counts as 1.
    pub fn text_scaled(&mut self, mut x: i16, y: i16, text: &str, c: u8, scale: u8) -> i16 {
        let k = u16::from(scale.max(1));
        for ch in text.chars() {
            let g = glyph_of(ch);
            let gw = glyph_w(g);
            // Letters wholly off the picture are skipped, but still
            // counted, so the returned x is the same either way.
            let x_end = i32::from(x) + i32::from(gw * k);
            if x_end > 0 && i32::from(x) < W as i32 {
                for j in 0..FONT_H {
                    for i in 0..gw {
                        if glyph_ink(g, i, j) {
                            let px = x.saturating_add((i * k) as i16);
                            let py = y.saturating_add((j * k) as i16);
                            if k == 1 {
                                self.pixel(px, py, c);
                            } else {
                                self.rect(px, py, k, k, c);
                            }
                        }
                    }
                }
            }
            x = x.saturating_add(((gw + 1) * k) as i16);
        }
        x
    }
}
