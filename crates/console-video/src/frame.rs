//! The picture the console draws into.

use board_api::Rect;

/// Width of the picture in pixels.
pub const W: usize = 160;
/// Height of the picture in pixels.
pub const H: usize = 120;

/// The picture: 160 x 120 bytes (19,200), row after row. Each byte is a
/// palette number. Aligned to 4 bytes, so rows can be compared 4 pixels
/// at a time.
#[repr(C, align(4))]
#[derive(Clone, PartialEq, Eq)]
pub struct Frame(pub [u8; W * H]);

impl Frame {
    /// All pixels palette number 0.
    pub const fn new() -> Self {
        Frame([0; W * H])
    }

    /// Every pixel the same palette number.
    pub fn clear(&mut self, c: u8) {
        self.0.fill(c);
    }

    /// The palette number at `(x, y)`, or 0 outside the picture.
    pub fn get(&self, x: u16, y: u16) -> u8 {
        if usize::from(x) < W && usize::from(y) < H {
            self.0[usize::from(y) * W + usize::from(x)]
        } else {
            0
        }
    }

    /// Sets one pixel. Outside the picture: nothing happens.
    pub fn set(&mut self, x: u16, y: u16, c: u8) {
        if usize::from(x) < W && usize::from(y) < H {
            self.0[usize::from(y) * W + usize::from(x)] = c;
        }
    }

    /// Fills a rectangle. The part outside the picture is cut off.
    pub fn fill_rect(&mut self, r: Rect, c: u8) {
        let r = r.intersect(&Rect::new(0, 0, W as u16, H as u16));
        for y in r.y..r.bottom() {
            let start = usize::from(y) * W + usize::from(r.x);
            self.0[start..start + usize::from(r.w)].fill(c);
        }
    }

    /// Row `y` (160 bytes).
    pub fn row(&self, y: usize) -> &[u8] {
        &self.0[y * W..][..W]
    }

    /// Row `y` (160 bytes), to change.
    pub fn row_mut(&mut self, y: usize) -> &mut [u8] {
        &mut self.0[y * W..][..W]
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}
