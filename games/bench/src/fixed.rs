//! Whole-number maths: angles, sine and cosine, rotation matrices. Taken
//! from MERCATOR (`fixed.rs`) and cut down to what the bench needs. One
//! change: every sum here is 32 bits. The bench keeps all its vectors
//! below 2^15 and every matrix entry is at most 16384 (plus rounding),
//! so a row times a vector stays below 3 * 2^29 and fits an `i32`. On
//! the chip a 32-bit product is one instruction and needs no call.
//!
//! * An **angle** is a `u16`: 65,536 is one whole turn.
//! * Sine, cosine and matrix entries are in 1/16384 ([`ONE14`]).
//!
//! The sine table is made while compiling, with whole numbers only.

/// 1.0 for sine, cosine and matrices.
pub const ONE14: i32 = 16384;
/// Bits of [`ONE14`].
pub const SHIFT14: u32 = 14;
/// The largest vector part [`Mat3::apply`] takes.
pub const VEC_MAX: i32 = 1 << 15;
/// Entries for a quarter turn (the table has one more, for 90 degrees).
const QUARTER: usize = 256;

/// sin(k * 90 degrees / 256) in 1/16384, for k = 0..=256.
const SIN_C: [i16; QUARTER + 1] = sin_table();
static SIN_Q: [i16; QUARTER + 1] = SIN_C;

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

/// Sine while compiling (for model tables), in 1/16384.
pub const fn csin(a: u16) -> i32 {
    let i = (a >> 6) as usize;
    let q = i & (QUARTER - 1);
    match i / QUARTER {
        0 => SIN_C[q] as i32,
        1 => SIN_C[QUARTER - q] as i32,
        2 => -(SIN_C[q] as i32),
        _ => -(SIN_C[QUARTER - q] as i32),
    }
}

/// Cosine while compiling, in 1/16384.
pub const fn ccos(a: u16) -> i32 {
    csin(a.wrapping_add(16_384))
}

/// Sine of an angle, in 1/16384 (1024 steps a turn).
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

/// A vector of three whole numbers.
pub type V3 = [i32; 3];

/// `a . b` in 32 bits. Callers keep one side at most 16,400 and the
/// other below [`VEC_MAX`] (2^15), so each product is below 2^29.1 and
/// the sum below 2^31: written without overflow checks (they cost chip
/// code in the inner loops), checked against a 64-bit sum in debug
/// builds (the PC tests).
#[inline(always)]
pub fn dot(a: V3, b: V3) -> i32 {
    let s = a[0]
        .wrapping_mul(b[0])
        .wrapping_add(a[1].wrapping_mul(b[1]))
        .wrapping_add(a[2].wrapping_mul(b[2]));
    debug_assert_eq!(
        i64::from(s),
        (0..3).fold(0i64, |t, k| t + i64::from(a[k]) * i64::from(b[k])),
        "dot out of range: {a:?} . {b:?}"
    );
    s
}

/// A 3 x 3 rotation matrix in 1/16384, rows first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mat3(pub [[i32; 3]; 3]);

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3([[ONE14, 0, 0], [0, ONE14, 0], [0, 0, ONE14]]);

    /// Turning by `yaw` (round y; +z towards +x), then `pitch` (round x;
    /// +z towards +y), then `roll` (round z): `M = Yaw * Pitch * Roll`.
    #[inline(never)]
    pub fn rotation(yaw: u16, pitch: u16, roll: u16) -> Mat3 {
        let (sy, cy) = (sin(yaw), cos(yaw));
        let (sp, cp) = (sin(pitch), cos(pitch));
        let (sr, cr) = (sin(roll), cos(roll));
        let yaw_m = Mat3([[cy, 0, sy], [0, ONE14, 0], [-sy, 0, cy]]);
        let pitch_m = Mat3([[ONE14, 0, 0], [0, cp, sp], [0, -sp, cp]]);
        let roll_m = Mat3([[cr, sr, 0], [-sr, cr, 0], [0, 0, ONE14]]);
        yaw_m.mul(&pitch_m).mul(&roll_m)
    }

    /// The product `self * o` (both rotations: entries at most 16384,
    /// so each sum is below 3 * 2^28).
    #[inline(never)]
    pub fn mul(&self, o: &Mat3) -> Mat3 {
        let mut m = [[0i32; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = dot(self.0[r], o.col(c)) >> SHIFT14;
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

    /// The matrix times a vector (parts below [`VEC_MAX`]), the result in
    /// the vector's units.
    #[inline(never)]
    pub fn apply(&self, v: V3) -> V3 {
        let m = &self.0;
        [
            dot(m[0], v) >> SHIFT14,
            dot(m[1], v) >> SHIFT14,
            dot(m[2], v) >> SHIFT14,
        ]
    }

    /// Column `c`.
    pub fn col(&self, c: usize) -> V3 {
        [self.0[0][c], self.0[1][c], self.0[2][c]]
    }
}

/// The whole-number square root (rounded down). Works while compiling.
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

/// A number from two whole numbers that looks random and is always the
/// same for the same input (where each object of a scene flies).
pub const fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}
