//! Rectangles and points: plain numbers, no meaning attached.
//!
//! The same [`Rect`] is used for panel pixels (what `Screen::present`
//! gets) and for the pixels of a video mode (what the renderer finds).
//! Each place that takes one says which.

/// A rectangle: the top-left corner and the size. A width or height of
/// 0 means "empty".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub const EMPTY: Rect = Rect::new(0, 0, 0, 0);

    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    pub const fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// The first column to the right of the rectangle.
    pub const fn right(&self) -> u16 {
        self.x + self.w
    }

    /// The first row below the rectangle.
    pub const fn bottom(&self) -> u16 {
        self.y + self.h
    }

    /// How many pixels it covers. u32: a 320x240 panel has 76,800.
    pub const fn area(&self) -> u32 {
        self.w as u32 * self.h as u32
    }

    pub const fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// The part both rectangles cover (empty if none).
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 <= x0 || y1 <= y0 {
            Rect::EMPTY
        } else {
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        }
    }

    /// The smallest rectangle that covers both. An empty one is ignored.
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Every number multiplied by `k` (a picture shown `k` times bigger),
    /// then moved by `(dx, dy)`.
    pub const fn scaled(&self, k: u16, dx: u16, dy: u16) -> Rect {
        Rect::new(self.x * k + dx, self.y * k + dy, self.w * k, self.h * k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_and_union() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Rect::new(5, 5, 5, 5));
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert!(a.intersect(&Rect::new(10, 0, 5, 5)).is_empty());
        assert_eq!(Rect::EMPTY.union(&b), b);
    }

    #[test]
    fn scale_to_panel() {
        // A 160x120 picture at 2x fills the 320x240 panel.
        assert_eq!(
            Rect::new(0, 0, 160, 120).scaled(2, 0, 0),
            Rect::new(0, 0, 320, 240)
        );
        assert_eq!(Rect::new(4, 8, 4, 1).scaled(2, 0, 0).area(), 4 * 2 * 2);
    }
}
