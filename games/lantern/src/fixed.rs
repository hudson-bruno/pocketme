//! Whole-number maths: positions, angles, sine and cosine.
//!
//! * A **position** is in 1/4096 of a map cell ([`ONE`] = one cell), so a
//!   32-cell map is 131,072 at most: far from the `i32` limit.
//! * An **angle** is a `u16`: 65,536 is one whole turn, so turning wraps
//!   round by itself (`wrapping_add`). Angle 0 looks along +x (east);
//!   a bigger angle turns right (towards +y, which is south: rows grow
//!   downwards on the map).
//! * A **direction** (sine, cosine) is in 1/16384 ([`TRIG_ONE`]).
//!
//! The sine table is computed while compiling, with whole numbers only
//! (a Taylor series in 128-bit numbers), so no floating point is ever
//! used, not even in the build.

/// One map cell in position units.
pub const ONE: i32 = 4096;
/// Bits of [`ONE`].
pub const SHIFT: u32 = 12;
/// 1.0 for sine and cosine.
pub const TRIG_ONE: i32 = 16384;
/// Bits of [`TRIG_ONE`].
pub const TRIG_SHIFT: u32 = 14;
/// Entries for a quarter turn (the table has one more, for 90 degrees).
pub const QUARTER: usize = 256;
/// One turn in angle units.
pub const TURN: u32 = 65_536;

/// sin(k * 90 degrees / 256) in 1/16384, for k = 0..=256.
pub static SIN_Q: [i16; QUARTER + 1] = sin_table();

/// pi in 1/2^40, as a whole number (pi * 2^40 = 3,454,217,652,358.4).
const PI_Q40: i128 = 3_454_217_652_358;

const fn sin_table() -> [i16; QUARTER + 1] {
    let mut t = [0i16; QUARTER + 1];
    let mut k = 0;
    while k <= QUARTER {
        // x = (pi / 2) * k / 256 in 1/2^40.
        let x = PI_Q40 * k as i128 / (2 * QUARTER as i128);
        // sin x = x - x^3/3! + x^5/5! - ..., term by term.
        let x2 = (x * x) >> 40;
        let mut term = x;
        let mut sum = x;
        let mut n = 1i128;
        while n < 10 {
            term = -((term * x2) >> 40) / ((2 * n) * (2 * n + 1));
            sum += term;
            n += 1;
        }
        // To 1/16384, rounded.
        let v = (sum + (1 << 25)) >> 26;
        t[k] = v as i16;
        k += 1;
    }
    t
}

/// Sine of an angle, in 1/16384.
pub fn sin(a: u16) -> i32 {
    // 1024 steps a turn: the top 10 bits of the angle.
    let i = usize::from(a >> 6);
    let q = i & (QUARTER - 1);
    match i / QUARTER {
        0 => i32::from(SIN_Q[q]),
        1 => i32::from(SIN_Q[QUARTER - q]),
        2 => -i32::from(SIN_Q[q]),
        _ => -i32::from(SIN_Q[QUARTER - q]),
    }
}

/// Cosine of an angle, in 1/16384.
pub fn cos(a: u16) -> i32 {
    sin(a.wrapping_add(16_384))
}

/// The cell a position is in.
pub fn cell(p: i32) -> i32 {
    p >> SHIFT
}

/// The middle of a cell, as a position.
pub const fn centre(c: i32) -> i32 {
    c * ONE + ONE / 2
}

/// Square of the distance between two points, in cells squared times
/// 4096 (so it stays in range: positions are below 32 cells).
pub fn dist2(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    let dx = (ax - bx) >> 4;
    let dy = (ay - by) >> 4;
    // dx, dy below 32 * 256 = 8192: the sum is below 2^27.
    dx * dx + dy * dy
}

/// `dist2` for a distance of `cells_q12` (in position units).
pub const fn dist2_of(d: i32) -> i32 {
    (d >> 4) * (d >> 4)
}

/// The angle that looks from (0, 0) towards (dx, dy): a whole-number
/// arctangent good to about half a degree (the table, searched).
pub fn angle_to(dx: i32, dy: i32) -> u16 {
    if dx == 0 && dy == 0 {
        return 0;
    }
    let (ax, ay) = (dx.unsigned_abs(), dy.unsigned_abs());
    // The angle inside the first eighth: tan = small / big.
    let (small, big) = if ay <= ax { (ay, ax) } else { (ax, ay) };
    // Find k with sin/cos(k) closest to small/big, k in 0..=128
    // (0 to 45 degrees). small * cos(k) <= big * sin(k) at the answer.
    let (mut lo, mut hi) = (0usize, QUARTER / 2);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let s = u64::from(SIN_Q[mid] as u16);
        let c = u64::from(SIN_Q[QUARTER - mid] as u16);
        if u64::from(small) * c > u64::from(big) * s {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let mut k = lo as u32; // 0..=128 steps of 1/1024 turn
    if ay > ax {
        k = 256 - k;
    }
    // Into the right quarter: x and y signs.
    let k = match (dx >= 0, dy >= 0) {
        (true, true) => k,
        (false, true) => 512 - k,
        (false, false) => 512 + k,
        (true, false) => (1024 - k) & 1023,
    };
    (k << 6) as u16
}

/// A small random number maker (the same numbers every run).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(pub u32);

impl Rng {
    /// The next number, 0..=65535.
    pub fn roll(&mut self) -> u32 {
        // Numerical Recipes' constants; wraps on purpose.
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 16
    }

    /// A number below `n` (n at least 1).
    pub fn below(&mut self, n: u32) -> u32 {
        self.roll() % n.max(1)
    }
}
