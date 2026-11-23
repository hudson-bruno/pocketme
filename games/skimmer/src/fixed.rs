//! Whole-number maths: square roots, lengths, directions, chance.
//!
//! No floating point anywhere in the game. Positions and speeds are in
//! **1/256 of a pixel** (`i32`): [`PX`] is one pixel. Speeds are per
//! physics tick (1/60 s). Directions are vectors whose length is
//! [`UNIT`] (4096), so a direction times a length, divided by 4096, is a
//! step of that length.

/// One pixel in position units.
pub const PX: i32 = 256;
/// The length of a direction vector.
pub const UNIT: i32 = 4096;

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

/// The squared length of `(dx, dy)`, without overflow.
pub fn len2(dx: i32, dy: i32) -> i64 {
    let (x, y) = (i64::from(dx), i64::from(dy));
    x * x + y * y
}

/// The length of `(dx, dy)` (rounded down).
pub fn len(dx: i32, dy: i32) -> i32 {
    isqrt(len2(dx, dy) as u64).min(i32::MAX as u64) as i32
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
    if len2(dx, dy) > len2(max, 0) {
        norm(dx, dy, max)
    } else {
        (dx, dy)
    }
}

/// `v * num / den`, in 64 bits, rounded towards zero.
pub fn mul_div(v: i32, num: i32, den: i32) -> i32 {
    (i64::from(v) * i64::from(num) / i64::from(den.max(1))) as i32
}

/// A direction (`UNIT` long) for `a` of 64 steps round the circle,
/// from 0 (along +x) the way from +x to +y. No floating point: an
/// eighth of the circle from a table, mirrored.
pub fn dir64(a: i32) -> (i32, i32) {
    // Cosine and sine of 0 to 8 steps of 45/8 degrees, times 4096.
    const C: [(i32, i32); 9] = [
        (4096, 0),
        (4076, 401),
        (4017, 799),
        (3920, 1189),
        (3784, 1567),
        (3612, 1931),
        (3406, 2276),
        (3166, 2598),
        (2896, 2896),
    ];
    let a = a.rem_euclid(64);
    let (o, r) = (a / 8, (a % 8) as usize);
    let (c, s) = if o % 2 == 0 {
        C[r]
    } else {
        let v = C[8 - r];
        (v.1, v.0)
    };
    match o / 2 {
        0 => (c, s),
        1 => (-s, c),
        2 => (-c, -s),
        _ => (s, -c),
    }
}

/// A small random number maker (xorshift, 32 bits), seeded from the
/// console's call count when a match starts: the same input always
/// gives the same match.
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
            assert!((UNIT - 3..=UNIT).contains(&l), "{x},{y}: {l}");
        }
        assert_eq!(norm(0, 0, UNIT), (0, 0));
        for a in 0..64 {
            let (x, y) = dir64(a);
            let l = len(x, y);
            assert!((UNIT - 3..=UNIT + 3).contains(&l), "{a}: {l}");
            // A quarter turn on.
            assert_eq!(dir64(a + 16), (-y, x));
        }
        assert_eq!(clamp_len(3, 4, 10), (3, 4));
        assert_eq!(clamp_len(30, 40, 10), (6, 8));
    }
}
