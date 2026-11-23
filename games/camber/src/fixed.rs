//! Whole-number maths: angles, sine and cosine, rotation matrices,
//! square roots, an arc tangent for the track bake, and a hash.
//!
//! The sine table, the matrices and the square root are the shared 3D
//! library's (`crates/game-3d`). CAMBER uses its smooth sine
//! (`sin_fine`): it goes in a straight line between the table's 1,024
//! steps a turn, so a slowly turning camera moves the picture smoothly
//! (KESTREL's camera never turns; here it turns all the time and the
//! steps showed as a jerk of about half a pixel). It is a `const fn`,
//! so the tracks are worked out while compiling with it and sit in the
//! game's constants, not in its state or on the stack.
//!
//! An **angle** is a `u16` (or, signed, an `i32` taken modulo 65,536):
//! 65,536 is one whole turn. Sine, cosine and every entry of a rotation
//! matrix are in 1/16384 ([`ONE14`]). The sine table is computed while
//! compiling with whole numbers only, so no floating point is used, not
//! even in the build.

pub use game_3d::{Mat3, ONE14, SHIFT14, cos_fine as cos, isqrt, sin_fine as sin};

/// An angle from whole degrees.
pub const fn deg(d: i32) -> i32 {
    d * 65_536 / 360
}

/// The direction of (x, z) as an angle: 0 along +z, a quarter turn
/// along +x. While compiling (the track bake); a binary search on the
/// sine table, good to about 1/8000 of a turn.
pub const fn atan2(x: i64, z: i64) -> u16 {
    if x == 0 && z == 0 {
        return 0;
    }
    let ax = if x < 0 { -x } else { x };
    let az = if z < 0 { -z } else { z };
    // The angle a in the first quarter with tan a = ax / az.
    let mut lo = 0i32;
    let mut hi = 16_384i32;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        let s = sin(mid as u16) as i64;
        let c = cos(mid as u16) as i64;
        if s * az <= c * ax {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let a = lo;
    let r = if x >= 0 && z >= 0 {
        a
    } else if x >= 0 {
        32_768 - a
    } else if z < 0 {
        32_768 + a
    } else {
        65_536 - a
    };
    (r & 0xFFFF) as u16
}

/// The difference `b - a` of two angles, as -32768..32767.
pub const fn turn(a: u16, b: u16) -> i32 {
    b.wrapping_sub(a) as i16 as i32
}

/// `v * k / 16384` for each part (k a sine, cosine or matrix entry).
pub fn scale(v: [i32; 3], k: i32) -> [i32; 3] {
    v.map(|x| ((i64::from(x) * i64::from(k)) >> SHIFT14) as i32)
}

/// A number from two whole numbers that looks random and is always the
/// same for the same input (scenery, the backdrop's hills).
pub const fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h
}
