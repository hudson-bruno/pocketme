//! Whole-number maths: angles, sine and cosine, rotation matrices and
//! square roots.
//!
//! * An **angle** is a `u16`: 65,536 is one whole turn, so turning wraps
//!   round by itself (`wrapping_add`).
//! * Sine, cosine and every entry of a rotation matrix are in 1/16384
//!   ([`ONE14`]).
//!
//! The sine table is computed while compiling, with whole numbers only
//! (a Taylor series in 128-bit numbers), so no floating point is used,
//! not even in the build.

use crate::sums::{Checked, Sums};

/// 1.0 for sine, cosine and matrices.
pub const ONE14: i32 = 16384;
/// Bits of [`ONE14`].
pub const SHIFT14: u32 = 14;
/// Entries for a quarter turn (the table has one more, for 90 degrees).
const QUARTER: usize = 256;

/// sin(k * 90 degrees / 256) in 1/16384, for k = 0..=256.
static SIN_Q: [i16; QUARTER + 1] = sin_table();

/// pi in 1/2^40, as a whole number.
const PI_Q40: i128 = 3_454_217_652_358;

const fn sin_table() -> [i16; QUARTER + 1] {
    let mut t = [0i16; QUARTER + 1];
    let mut k = 0;
    while k <= QUARTER {
        let x = PI_Q40 * k as i128 / (2 * QUARTER as i128);
        let x2 = (x * x) >> 40;
        let mut term = x;
        let mut sum = x;
        let mut n = 1i128;
        while n < 10 {
            term = -((term * x2) >> 40) / ((2 * n) * (2 * n + 1));
            sum += term;
            n += 1;
        }
        t[k] = ((sum + (1 << 25)) >> 26) as i16;
        k += 1;
    }
    t
}

/// Sine of an angle, in 1/16384 (1024 steps a turn: the angle is
/// rounded down to the table's step). See [`sin_fine`] for a sine that
/// goes smoothly between the steps.
pub fn sin(a: u16) -> i32 {
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

/// Sine of an angle, in 1/16384, going in a straight line between the
/// table's 1,024 steps a turn (CAMBER's: its camera turns all the time,
/// and with [`sin`]'s steps a slow turn shows as a jerk of about half a
/// pixel). A `const fn`, so tables can be worked out with it while
/// compiling; while the game runs it reads the one sine table.
///
/// Its sums are plain Rust sums (a `const fn` cannot take the setup's
/// [`crate::Sums`]); they cannot run over.
#[inline(never)]
pub const fn sin_fine(a: u16) -> i32 {
    let t = &SIN_Q;
    let i = (a >> 6) as usize;
    let f = (a & 63) as i32;
    let q = i & (QUARTER - 1);
    // Within a quarter, from entry q towards q + 1 (or back down).
    let (s0, s1) = match i / QUARTER {
        0 => (t[q] as i32, t[q + 1] as i32),
        1 => (t[QUARTER - q] as i32, t[QUARTER - q - 1] as i32),
        2 => (-(t[q] as i32), -(t[q + 1] as i32)),
        _ => (-(t[QUARTER - q] as i32), -(t[QUARTER - q - 1] as i32)),
    };
    s0 + (s1 - s0) * f / 64
}

/// Cosine of an angle, in 1/16384, smooth like [`sin_fine`].
#[inline(never)]
pub const fn cos_fine(a: u16) -> i32 {
    sin_fine(a.wrapping_add(16_384))
}

/// An angle from degrees (whole degrees, for tables).
pub const fn deg(d: i32) -> u16 {
    (d * 65_536 / 360) as u16
}

/// A 3 x 3 rotation (and scale) matrix in 1/16384, rows first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mat3(pub [[i32; 3]; 3]);

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3([[ONE14, 0, 0], [0, ONE14, 0], [0, 0, ONE14]]);

    /// Turning by `yaw` (round the up axis y; positive turns the nose
    /// from +z towards +x), then `pitch` (round x; positive lifts the
    /// nose towards +y), then `roll` (round z; positive lifts the left
    /// side: a bank to the right). Applied to a model as roll first,
    /// then pitch, then yaw: `M = Yaw * Pitch * Roll`.
    pub fn rotation(yaw: u16, pitch: u16, roll: u16) -> Mat3 {
        Mat3::from_sines(
            (sin(yaw), cos(yaw)),
            (sin(pitch), cos(pitch)),
            (sin(roll), cos(roll)),
        )
    }

    /// [`Mat3::rotation`] with the smooth sine ([`sin_fine`],
    /// [`cos_fine`]; CAMBER's).
    #[inline(never)]
    pub fn rotation_fine(yaw: u16, pitch: u16, roll: u16) -> Mat3 {
        Mat3::from_sines(
            (sin_fine(yaw), cos_fine(yaw)),
            (sin_fine(pitch), cos_fine(pitch)),
            (sin_fine(roll), cos_fine(roll)),
        )
    }

    /// The turning from the sines and cosines of yaw, pitch and roll.
    #[inline(always)]
    fn from_sines((sy, cy): (i32, i32), (sp, cp): (i32, i32), (sr, cr): (i32, i32)) -> Mat3 {
        let yaw_m = Mat3([[cy, 0, sy], [0, ONE14, 0], [-sy, 0, cy]]);
        let pitch_m = Mat3([[ONE14, 0, 0], [0, cp, sp], [0, -sp, cp]]);
        let roll_m = Mat3([[cr, sr, 0], [-sr, cr, 0], [0, 0, ONE14]]);
        yaw_m.mul(&pitch_m).mul(&roll_m)
    }

    /// The product `self * o` (checked sums).
    #[inline(always)]
    pub fn mul(&self, o: &Mat3) -> Mat3 {
        self.mul_with::<Checked>(o)
    }

    /// The product `self * o`, with the sums `S`.
    pub fn mul_with<S: Sums>(&self, o: &Mat3) -> Mat3 {
        let mut m = [[0i32; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                let s = S::add64(
                    S::add64(
                        S::wide(self.0[r][0], o.0[0][c]),
                        S::wide(self.0[r][1], o.0[1][c]),
                    ),
                    S::wide(self.0[r][2], o.0[2][c]),
                );
                *cell = (s >> SHIFT14) as i32;
            }
        }
        Mat3(m)
    }

    /// Rows and columns swapped. For a rotation this is the turn back.
    pub fn transposed(&self) -> Mat3 {
        let m = &self.0;
        Mat3([
            [m[0][0], m[1][0], m[2][0]],
            [m[0][1], m[1][1], m[2][1]],
            [m[0][2], m[1][2], m[2][2]],
        ])
    }

    /// Column `c`: for a model's turning, its right (0), up (1) and
    /// forward (2) in the world.
    pub fn col(&self, c: usize) -> [i32; 3] {
        [self.0[0][c], self.0[1][c], self.0[2][c]]
    }

    /// Every entry times `s / 256` (a size: 256 is 1.0; checked sums).
    #[inline(always)]
    pub fn scaled(&self, s: i32) -> Mat3 {
        self.scaled_with::<Checked>(s)
    }

    /// Every entry times `s / 256`, with the sums `S`.
    pub fn scaled_with<S: Sums>(&self, s: i32) -> Mat3 {
        let mut m = self.0;
        for row in m.iter_mut() {
            for v in row.iter_mut() {
                *v = S::mul(*v, s) / 256;
            }
        }
        Mat3(m)
    }

    /// The matrix times a vector, the result in the vector's units
    /// (checked sums).
    #[inline(always)]
    pub fn apply(&self, v: [i32; 3]) -> [i32; 3] {
        self.apply_with::<Checked>(v)
    }

    /// The matrix times a vector, with the sums `S`.
    pub fn apply_with<S: Sums>(&self, v: [i32; 3]) -> [i32; 3] {
        let m = &self.0;
        let row = |r: usize| -> i32 {
            (S::add64(
                S::add64(S::wide(m[r][0], v[0]), S::wide(m[r][1], v[1])),
                S::wide(m[r][2], v[2]),
            ) >> SHIFT14) as i32
        };
        [row(0), row(1), row(2)]
    }
}

/// The whole-number square root (rounded down), for every `u64`. Works
/// while compiling. Digit by digit, with no division (MERCATOR's way:
/// on the chip a 64-bit division is a slow library call).
pub const fn isqrt(n: u64) -> u64 {
    let mut n = n;
    let mut x = 0u64;
    let mut bit = 1u64 << 62;
    while bit > n {
        bit >>= 2;
    }
    while bit != 0 {
        let t = x.wrapping_add(bit);
        if n >= t {
            n = n.wrapping_sub(t);
            x = (x >> 1).wrapping_add(bit);
        } else {
            x >>= 1;
        }
        bit >>= 2;
    }
    x
}
