//! Whole-number maths: a hash and a small dice, here; angles, sine and
//! cosine, rotation matrices and square roots come from the shared 3D
//! library (`game-3d`) and are passed on as they are.
//!
//! * An **angle** is a `u16`: 65,536 is one whole turn, so turning wraps
//!   round by itself (`wrapping_add`).
//! * Sine, cosine and every entry of a rotation matrix are in 1/16384
//!   ([`ONE14`]).

pub use game_3d::{Mat3, ONE14, SHIFT14, cos, deg, isqrt, sin};

/// A number from a few whole numbers that looks random and is always
/// the same for the same input (for building the course).
pub const fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}

/// A small dice: the same numbers every run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dice(pub u32);

impl Dice {
    /// The next number, 0..=65535.
    pub fn roll(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 16
    }

    /// A number below `n` (n at least 1).
    pub fn below(&mut self, n: u32) -> u32 {
        self.roll() % n.max(1)
    }

    /// A number from `lo` to `hi`, both included.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi - lo + 1).max(1) as u32) as i32
    }
}
