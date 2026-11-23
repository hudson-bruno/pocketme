//! Whole-number maths: angles, sine and cosine, rotation matrices,
//! vectors, square roots and a small dice. Taken from KESTREL
//! (`games/kestrel/src/fixed.rs`) and grown for a ship that turns freely
//! (a matrix kept upright by [`Mat3::fix`], vectors in 64-bit sums).
//!
//! * An **angle** is a `u16`: 65,536 is one whole turn, so turning wraps
//!   round by itself (`wrapping_add`).
//! * Sine, cosine and every entry of a rotation matrix are in 1/16384
//!   ([`ONE14`]).
//!
//! The sine table is computed while compiling, with whole numbers only
//! (a Taylor series in 128-bit numbers), so no floating point is used,
//! not even in the build.

/// 1.0 for sine, cosine and matrices.
pub const ONE14: i32 = 16384;
/// Bits of [`ONE14`].
pub const SHIFT14: u32 = 14;
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

/// `a * b` in 64 bits. Exact for every pair of `i32` (the product is at
/// most 2^62 in size), so it is written without an overflow check: on
/// the chip a checked 64-bit product costs about 60 bytes of code each
/// time, and there are many.
#[inline(always)]
pub fn wide(a: i32, b: i32) -> i64 {
    i64::from(a).wrapping_mul(i64::from(b))
}

/// `a . b` in a 64-bit sum. Callers keep parts below 2^30 (positions
/// inside `world::LIMIT` and their differences), so each product is
/// below 2^60 and the sum cannot run over: written without the 64-bit
/// overflow checks (chip code), checked in debug builds.
#[inline(never)]
pub fn dot(a: V3, b: V3) -> i64 {
    debug_assert!(
        a.iter()
            .chain(b.iter())
            .all(|v| v.unsigned_abs() <= 1 << 30)
    );
    wide(a[0], b[0])
        .wrapping_add(wide(a[1], b[1]))
        .wrapping_add(wide(a[2], b[2]))
}

/// `a - b`. The caller keeps both inside the world (`world::LIMIT`,
/// 2^29), so the difference fits; checked in debug builds only.
#[inline(never)]
pub fn sub(a: V3, b: V3) -> V3 {
    debug_assert!((0..3).all(|k| a[k].checked_sub(b[k]).is_some()));
    [
        a[0].wrapping_sub(b[0]),
        a[1].wrapping_sub(b[1]),
        a[2].wrapping_sub(b[2]),
    ]
}

/// The length of `v` (rounded down).
#[inline(never)]
pub fn length(v: V3) -> i32 {
    isqrt(dot(v, v) as u64) as i32
}

/// `v` made `to` long (nothing when `v` is zero). `to` at most 2^20.
#[inline(never)]
pub fn norm(v: V3, to: i32) -> V3 {
    let l = isqrt(dot(v, v) as u64) as i64;
    if l == 0 {
        return [0, 0, 0];
    }
    v.map(|c| (wide(c, to) / l) as i32)
}

/// `v` times `k / 16384`.
#[inline(never)]
pub fn scale14(v: V3, k: i32) -> V3 {
    v.map(|c| (wide(c, k) >> SHIFT14) as i32)
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

    /// The product `self * o`.
    #[inline(never)]
    pub fn mul(&self, o: &Mat3) -> Mat3 {
        let mut m = [[0i32; 3]; 3];
        for (r, row) in m.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (dot(self.0[r], o.col(c)) >> SHIFT14) as i32;
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

    /// Every entry times `s / 256` (a size: 256 is 1.0). `s` from 0 to
    /// 2^18 - 1 (the 64-bit product cannot run over; the entry stays
    /// below 2^24).
    pub fn scaled(&self, s: i32) -> Mat3 {
        let mut m = self.0;
        for row in m.iter_mut() {
            for v in row.iter_mut() {
                *v = (wide(*v, s) / 256) as i32;
            }
        }
        Mat3(m)
    }

    /// The matrix times a vector, the result in the vector's units.
    #[inline(never)]
    pub fn apply(&self, v: V3) -> V3 {
        let m = &self.0;
        [
            (dot(m[0], v) >> SHIFT14) as i32,
            (dot(m[1], v) >> SHIFT14) as i32,
            (dot(m[2], v) >> SHIFT14) as i32,
        ]
    }

    /// The transposed matrix times a vector: world to the object's own
    /// axes for a rotation.
    #[inline(never)]
    pub fn apply_t(&self, v: V3) -> V3 {
        [
            (dot(self.col(0), v) >> SHIFT14) as i32,
            (dot(self.col(1), v) >> SHIFT14) as i32,
            (dot(self.col(2), v) >> SHIFT14) as i32,
        ]
    }

    /// Column `c`: where the object's own axis `c` (0 right, 1 up, 2
    /// nose) points in the world, 16384 long.
    pub fn col(&self, c: usize) -> V3 {
        [self.0[0][c], self.0[1][c], self.0[2][c]]
    }

    /// A matrix from its three columns.
    pub fn from_cols(r: V3, u: V3, f: V3) -> Mat3 {
        Mat3([[r[0], u[0], f[0]], [r[1], u[1], f[1]], [r[2], u[2], f[2]]])
    }

    /// The object turned by small angles round its own axes: `pitch`
    /// (nose up), `yaw` (nose right), `roll` (bank right). The result is
    /// made a clean rotation again ([`Mat3::fix`]), so rounding never
    /// piles up.
    #[inline(never)]
    pub fn turned(&self, pitch: i32, yaw: i32, roll: i32) -> Mat3 {
        let r = Mat3::rotation(yaw as u16, pitch as u16, roll as u16);
        self.mul(&r).fix()
    }

    /// The nearest clean rotation: the nose kept, the up axis made square
    /// to it, the right axis from both; each 16384 long.
    #[inline(never)]
    pub fn fix(&self) -> Mat3 {
        let f = norm(self.col(2), ONE14);
        let u = self.col(1);
        let k = (dot(u, f) >> SHIFT14) as i32;
        let u = norm(sub(u, scale14(f, k)), ONE14);
        let r = cross14(u, f);
        if f == [0, 0, 0] || u == [0, 0, 0] {
            return Mat3::IDENTITY;
        }
        Mat3::from_cols(r, u, f)
    }

    /// A rotation whose nose points along `fwd` and whose up axis is as
    /// near `up` as it can be.
    #[inline(never)]
    pub fn looking(fwd: V3, up: V3) -> Mat3 {
        Mat3::from_cols([0, 0, 0], up, fwd).fix()
    }
}

/// `a x b` for two vectors 16384 long, the result in 1/16384.
#[inline(never)]
pub fn cross14(a: V3, b: V3) -> V3 {
    [
        // Parts at most 2^14 long: products below 2^29.
        ((wide(a[1], b[2]).wrapping_sub(wide(a[2], b[1]))) >> SHIFT14) as i32,
        ((wide(a[2], b[0]).wrapping_sub(wide(a[0], b[2]))) >> SHIFT14) as i32,
        ((wide(a[0], b[1]).wrapping_sub(wide(a[1], b[0]))) >> SHIFT14) as i32,
    ]
}

/// The whole-number square root (rounded down). Works while compiling.
#[inline(never)]
pub const fn isqrt(n: u64) -> u64 {
    // Digit by digit in base 4: no division (a 64-bit division is a long
    // call on the chip). `x + bit` stays below 2^64 and `n - (x + bit)` is
    // taken only when it is not negative.
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

/// A number from a few whole numbers that looks random and is always
/// the same for the same input (the galaxy is made from these).
pub const fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}

/// A small dice: the same numbers every run for the same start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dice(pub u32);

impl Dice {
    /// The next number, 0..=65535.
    #[inline(never)]
    pub fn roll(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 16
    }

    /// A number below `n` (n at least 1).
    #[inline(never)]
    pub fn below(&mut self, n: u32) -> u32 {
        self.roll() % n.max(1)
    }

    /// A number from `-n` to `n`.
    #[inline(never)]
    pub fn spread(&mut self, n: i32) -> i32 {
        // n is a few thousand at most.
        (self.below(n.wrapping_mul(2).wrapping_add(1).max(1) as u32) as i32).wrapping_sub(n)
    }
}
