//! Small whole-number helpers: angles, lengths, rounding and the
//! fingerprint. Sine, cosine and square roots come from game-3d.

use game_3d::{ONE14, cos_fine, isqrt, sin_fine};

/// The way an angle points, as (x, z) in 1/16384: game-3d's yaw, 0
/// along +z, a quarter turn along +x.
pub fn dir(a: u16) -> (i32, i32) {
    (sin_fine(a), cos_fine(a))
}

/// The right-hand side of an angle, as (x, z) in 1/16384 (game-3d's
/// `Mat3::rotation(yaw, ..)` column 0: (cos, -sin)).
pub fn right(a: u16) -> (i32, i32) {
    (cos_fine(a), -sin_fine(a))
}

/// `a * b / 16384`, rounded to the nearest (halves away from zero), in
/// 64 bits.
pub fn mul14(a: i32, b: i32) -> i32 {
    let p = i64::from(a) * i64::from(b);
    let h = i64::from(ONE14 / 2);
    (if p >= 0 {
        (p + h) / 16_384
    } else {
        (p - h) / 16_384
    }) as i32
}

/// The length of a horizontal vector.
pub fn len2(x: i32, z: i32) -> i32 {
    let (x, z) = (i64::from(x), i64::from(z));
    isqrt((x * x + z * z) as u64) as i32
}

/// The squared horizontal distance in 64 bits (subs squared pass
/// `i32` beyond about 11 m).
pub fn dist2(a: (i32, i32), b: (i32, i32)) -> i64 {
    let dx = i64::from(a.0) - i64::from(b.0);
    let dz = i64::from(a.1) - i64::from(b.1);
    dx * dx + dz * dz
}

/// The angle of the horizontal direction (x, z) in game-3d's yaw, or
/// `None` for no direction. atan is approximated by
/// `t * 8192 + 2847 * t * (1 - t)` (t = small / big, a fraction; angles
/// in 1/65536 turn); within about 0.25 degrees.
pub fn yaw_of(x: i32, z: i32) -> Option<u16> {
    if x == 0 && z == 0 {
        return None;
    }
    let (ax, az) = (u64::from(x.unsigned_abs()), u64::from(z.unsigned_abs()));
    let (small, big, x_big) = if ax <= az {
        (ax, az, false)
    } else {
        (az, ax, true)
    };
    let t = (small << 16) / big; // 0..=65536
    let a = ((8192 * t * 65_536 + 2847 * t * (65_536 - t)) >> 32) as u32;
    let base = if x_big { 16_384 - a } else { a };
    let ang = match (x >= 0, z >= 0) {
        (true, true) => base,
        (true, false) => 32_768 - base,
        (false, false) => 32_768 + base,
        (false, true) => 65_536 - base,
    };
    Some((ang & 0xFFFF) as u16)
}

/// The signed difference `to - from` the short way round, as `i16`
/// (so a turn through angle 0 does not jump).
pub fn angle_diff(from: u16, to: u16) -> i32 {
    i32::from(to.wrapping_sub(from) as i16)
}

/// `a` turned by the signed amount `d`.
pub fn turn(a: u16, d: i32) -> u16 {
    a.wrapping_add(d as i16 as u16)
}

/// A short fingerprint (FNV-1a, 64 bits) of whole numbers.
pub fn fnv(h: u64, v: i64) -> u64 {
    let mut h = h;
    for b in v.to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The fingerprint's start value.
pub const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;
