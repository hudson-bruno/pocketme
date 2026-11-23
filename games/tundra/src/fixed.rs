//! Whole-number maths: square roots, lengths, directions, random numbers.
//!
//! No floating point anywhere in the game. Positions and speeds are in
//! **1/256 of a pixel** (`i32`): [`PX`] is one pixel. Speeds are per
//! physics tick (1/60 s). Directions are vectors whose length is
//! [`UNIT`] (256), so a direction times a speed, divided by 256, is a
//! velocity.

/// One pixel in position units.
pub const PX: i32 = 256;
/// The length of a direction vector.
pub const UNIT: i32 = 256;

/// The whole square root of `v` (rounded down).
pub fn isqrt(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    // Newton's method from a guess above the root; it only goes down.
    let mut x = 1u64 << (64 - v.leading_zeros()).div_ceil(2);
    loop {
        let y = (x + v / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// The length of `(dx, dy)`.
pub fn len(dx: i32, dy: i32) -> i32 {
    let (x, y) = (i64::from(dx), i64::from(dy));
    isqrt((x * x + y * y) as u64).min(i32::MAX as u64) as i32
}

/// `(dx, dy)` made `to` long, the same way round. `(0, 0)` stays.
pub fn norm(dx: i32, dy: i32, to: i32) -> (i32, i32) {
    let l = len(dx, dy);
    if l == 0 {
        return (0, 0);
    }
    let s = |v: i32| (i64::from(v) * i64::from(to) / i64::from(l)) as i32;
    (s(dx), s(dy))
}

/// `(dx, dy)` cut to at most `max` long.
pub fn clamp_len(dx: i32, dy: i32, max: i32) -> (i32, i32) {
    if len(dx, dy) > max {
        norm(dx, dy, max)
    } else {
        (dx, dy)
    }
}

/// `v` times `num / 256`, rounded towards zero.
pub fn scale(v: i32, num: i32) -> i32 {
    (i64::from(v) * i64::from(num) / 256) as i32
}

/// The eight ways a skater can face on screen, from a direction:
/// 0 right, then clockwise (2 down, 4 left, 6 up).
pub fn octant(dx: i32, dy: i32) -> usize {
    let (ax, ay) = (dx.abs(), dy.abs());
    // tan(22.5 degrees) is about 106 / 256.
    let horizontal = ay * 256 <= ax * 106;
    let vertical = ax * 256 <= ay * 106;
    match (horizontal, vertical, dx >= 0, dy >= 0) {
        (true, _, true, _) => 0,
        (true, _, false, _) => 4,
        (_, true, _, true) => 2,
        (_, true, _, false) => 6,
        (_, _, true, true) => 1,
        (_, _, false, true) => 3,
        (_, _, false, false) => 5,
        (_, _, true, false) => 7,
    }
}

/// Unit steps for the eight ways of [`octant`] (for drawing).
pub const OCTANT_STEP: [(i16, i16); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// A small random number maker (xorshift, 32 bits). Seeded from the
/// frame count and the input, so a run is the same every time it gets
/// the same input: there is no hidden source of chance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(pub u32);

impl Rng {
    pub const fn new(seed: u32) -> Rng {
        Rng(if seed == 0 { 0x2545_F491 } else { seed })
    }

    pub fn step(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// 0 to `n - 1` (`n` above 0).
    pub fn below(&mut self, n: u32) -> u32 {
        self.step() % n.max(1)
    }

    /// -`n` to `n`.
    pub fn spread(&mut self, n: i32) -> i32 {
        self.below(2 * n.max(0) as u32 + 1) as i32 - n
    }

    /// True once in `n` (on average).
    pub fn one_in(&mut self, n: u32) -> bool {
        self.below(n) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_roots_are_whole_and_rounded_down() {
        for v in 0..20_000u64 {
            let r = isqrt(v);
            assert!(r * r <= v && (r + 1) * (r + 1) > v, "{v}");
        }
        let big = (u32::MAX as u64) * 3;
        let r = isqrt(big);
        assert!(r * r <= big && (r + 1) * (r + 1) > big);
    }

    #[test]
    fn directions_have_the_unit_length() {
        for (x, y) in [(3, 4), (-100, 1), (0, -9), (40_000, 40_000)] {
            let (a, b) = norm(x, y, UNIT);
            let l = len(a, b);
            assert!((UNIT - 2..=UNIT).contains(&l), "{x},{y}: {l}");
        }
        assert_eq!(norm(0, 0, UNIT), (0, 0));
    }

    #[test]
    fn octants_go_round_clockwise() {
        assert_eq!(octant(10, 0), 0);
        assert_eq!(octant(10, 10), 1);
        assert_eq!(octant(0, 10), 2);
        assert_eq!(octant(-10, 10), 3);
        assert_eq!(octant(-10, 0), 4);
        assert_eq!(octant(-10, -10), 5);
        assert_eq!(octant(0, -10), 6);
        assert_eq!(octant(10, -10), 7);
    }
}
