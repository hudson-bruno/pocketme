//! Whole-number maths: angles, sine, a small arctangent, random numbers.
//!
//! No floating point anywhere in the game. The formats:
//!
//! * Positions and speeds: **Q16**, 1/65536 of a pixel (`i32`). Speeds
//!   are per physics tick (1/60 s), accelerations per tick per tick.
//! * Angles: 256 steps for a full turn, kept with 8 more bits of
//!   fraction (**Q8**, so 65536 for a full turn) in an `i32`. 0 is
//!   upright, positive leans right (clockwise on the screen).
//! * Sine and cosine: **Q14** (16384 is 1.0), from a table.

/// One pixel in position units (Q16).
pub const PX: i32 = 1 << 16;

/// One angle step (1/256 turn) in angle units (Q8).
pub const STEP: i32 = 1 << 8;

/// A quarter turn in angle units: the craft leans at most this far.
pub const QUARTER: i32 = 64 * STEP;

/// 1.0 in the sine table.
pub const ONE: i32 = 1 << 14;

/// sin(i / 256 of a turn) for i = 0..=64, in Q14. Worked out once, off
/// line (round(sin(i * pi / 128) * 16384)).
#[rustfmt::skip]
const SIN: [i16; 65] = [
    0, 402, 804, 1205, 1606, 2006, 2404, 2801, 3196, 3590, 3981, 4370, 4756,
    5139, 5520, 5897, 6270, 6639, 7005, 7366, 7723, 8076, 8423, 8765, 9102,
    9434, 9760, 10080, 10394, 10702, 11003, 11297, 11585, 11866, 12140, 12406,
    12665, 12916, 13160, 13395, 13623, 13842, 14053, 14256, 14449, 14635,
    14811, 14978, 15137, 15286, 15426, 15557, 15679, 15791, 15893, 15986,
    16069, 16143, 16207, 16261, 16305, 16340, 16364, 16379, 16384,
];

/// atan(i / 32) in 1/256 turns, for i = 0..=32 (0 to 45 degrees).
#[rustfmt::skip]
const ATAN: [u8; 33] = [
    0, 1, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 15, 16, 17, 18, 19, 20, 21, 22,
    23, 24, 25, 25, 26, 27, 28, 29, 29, 30, 31, 31, 32,
];

/// The whole step of an angle, rounded to the nearest.
pub fn step_of(angle: i32) -> i32 {
    angle.saturating_add(STEP / 2) >> 8
}

/// sin of a whole step (any value; the table is used four ways).
pub fn sin_step(step: i32) -> i32 {
    let s = step.rem_euclid(256);
    let (q, i) = (s / 64, (s % 64) as usize);
    match q {
        0 => i32::from(SIN[i]),
        1 => i32::from(SIN[64 - i]),
        2 => -i32::from(SIN[i]),
        _ => -i32::from(SIN[64 - i]),
    }
}

/// cos of a whole step.
pub fn cos_step(step: i32) -> i32 {
    sin_step(step.saturating_add(64))
}

/// The angle (whole steps, -64..=64) that points from the craft towards
/// `(dx, dy)` (screen directions, y down), as far as the craft can lean.
/// Straight up is 0; anything below the craft leans fully to its side.
pub fn lean_towards(dx: i32, dy: i32) -> i32 {
    let ax = u64::from(dx.unsigned_abs());
    let up = u64::from(dy.saturating_neg().max(0).unsigned_abs());
    if ax == 0 && up == 0 {
        return 0;
    }
    // Angle from straight up, 0..=64 steps.
    let a = if up >= ax {
        i32::from(ATAN[(ax * 32 / up) as usize])
    } else {
        64 - i32::from(ATAN[(up * 32 / ax) as usize])
    };
    if dx < 0 { -a } else { a }
}

/// Turns the offset `(x, y)` by a whole-step angle. The result is in
/// Q14 (16384 times bigger).
pub fn turn(x: i32, y: i32, step: i32) -> (i32, i32) {
    let (s, c) = (sin_step(step), cos_step(step));
    // Screen y grows down, so a clockwise turn is this matrix.
    (
        x.saturating_mul(c).saturating_sub(y.saturating_mul(s)),
        x.saturating_mul(s).saturating_add(y.saturating_mul(c)),
    )
}

/// A small random number maker (xorshift32): the same numbers for the
/// same seed, on the PC and on the chip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(u32);

impl Rng {
    /// A seed of 0 would give only zeros; it is changed to another.
    pub const fn new(seed: u32) -> Rng {
        Rng(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    pub fn roll(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// A number from 0 to `n - 1` (0 when `n` is 0).
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.roll() % n }
    }

    /// A number from `-r` to `r`.
    pub fn spread(&mut self, r: i32) -> i32 {
        let r = r.max(0);
        self.below(r.unsigned_abs() * 2 + 1) as i32 - r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_and_cosine_agree_all_round() {
        for s in -300..300 {
            let (a, b) = (sin_step(s), cos_step(s));
            let len = a * a + b * b;
            let one = ONE * ONE;
            assert!((len - one).abs() < one / 500, "{s}: {a} {b}");
        }
        assert_eq!(sin_step(64), ONE);
        assert_eq!(cos_step(0), ONE);
        assert_eq!(sin_step(-64), -ONE);
    }

    #[test]
    fn leaning_towards_a_point() {
        assert_eq!(lean_towards(0, -10), 0);
        assert_eq!(lean_towards(10, -10), 32);
        assert_eq!(lean_towards(-10, -10), -32);
        assert_eq!(lean_towards(10, 0), 64);
        assert_eq!(lean_towards(-10, 30), -64);
        assert_eq!(lean_towards(0, 0), 0);
        assert_eq!(lean_towards(i32::MIN, 0), -64);
        assert_eq!(lean_towards(i32::MIN, i32::MIN), -33);
        assert_eq!(lean_towards(i32::MAX, i32::MIN), 32);
    }

    #[test]
    fn spread_stays_inside() {
        let mut r = Rng::new(1);
        for _ in 0..1000 {
            let v = r.spread(3);
            assert!((-3..=3).contains(&v));
        }
        assert_eq!(Rng::new(0).roll(), Rng::new(0x9E37_79B9).roll());
    }
}
