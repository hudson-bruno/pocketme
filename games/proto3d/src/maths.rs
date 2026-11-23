//! Small whole-number helpers the rules share: units, directions and
//! angles. Sine, cosine and square roots come from game-3d.

use game_3d::{ONE14, cos_fine, isqrt, sin_fine};

use crate::tune::SUB_PER_UNIT;

/// Centimetres (the records' unit) to subs (1/4096 m), rounded down.
pub const fn cm(c: i16) -> i32 {
    (c as i32 * 1024).div_euclid(25)
}

/// Subs to game units (1/256 m), as drawing does (`sub >> 4`).
pub const fn unit(s: i32) -> i32 {
    s >> 4
}

/// Game units to subs.
pub const fn sub(u: i32) -> i32 {
    u * SUB_PER_UNIT
}

/// The length of a horizontal vector.
pub fn len2(x: i32, z: i32) -> i32 {
    let (x, z) = (i64::from(x), i64::from(z));
    isqrt((x * x + z * z) as u64) as i32
}

/// The length of a vector.
pub fn len3(v: [i32; 3]) -> i64 {
    let s: i64 = v.iter().map(|&c| i64::from(c) * i64::from(c)).sum();
    isqrt(s as u64) as i64
}

/// The angle (1/65536 turn, game-3d's yaw: 0 looks along +z, a quarter
/// turn along +x) of the horizontal direction (x, z), or `None` for no
/// direction. Within about 40 (0.22 degrees) of the true angle
/// (atan(t) close to t * pi/4 + 0.273 t (1 - t), whole numbers only).
pub fn yaw_of(x: i32, z: i32) -> Option<u16> {
    if x == 0 && z == 0 {
        return None;
    }
    let (ax, az) = (u64::from(x.unsigned_abs()), u64::from(z.unsigned_abs()));
    let (small, big, swap) = if ax <= az {
        (ax, az, false)
    } else {
        (az, ax, true)
    };
    // t in 1/65536, 0..=65536.
    let t = (small << 16) / big;
    // atan(t) in 1/65536 of a turn: 8192 t + 2847 t (1 - t), t a fraction.
    let a = ((8192 * t * 65_536 + 2847 * t * (65_536 - t)) >> 32) as u32;
    let base = if swap { 16_384 - a } else { a };
    let ang = match (x >= 0, z >= 0) {
        (true, true) => base,
        (true, false) => 32_768 - base,
        (false, false) => 32_768 + base,
        (false, true) => 65_536 - base,
    };
    Some((ang & 0xFFFF) as u16)
}

/// The signed difference `to - from`, the short way round (-32768 to
/// 32767).
pub fn angle_diff(from: u16, to: u16) -> i32 {
    i32::from(to.wrapping_sub(from) as i16)
}

/// `v` kept within `lo..=hi`, as `i32::clamp` but without its check that
/// `lo <= hi` (`hi` wins then): that check's panic message pulled about
/// 600 bytes of number formatting into the chip build (repair round,
/// MEASURED). Every caller here has `lo <= hi`.
pub fn clamp(v: i32, lo: i32, hi: i32) -> i32 {
    v.max(lo).min(hi)
}

/// `from` turned towards `to` by at most `max`.
pub fn turn_towards(from: u16, to: u16, max: u16) -> u16 {
    let d = clamp(angle_diff(from, to), -i32::from(max), i32::from(max));
    from.wrapping_add(d as u16)
}

/// The world direction (x, z), in 1/16384, of a direction given in the
/// camera's terms (`right`, `fwd`, in 1/16384) for a camera turned by
/// `yaw`: game-3d's `Mat3::rotation(yaw, ..)` has right (cos, 0, -sin)
/// and forward (sin, 0, cos).
pub fn turn_by_yaw(right: i32, fwd: i32, yaw: u16) -> [i32; 2] {
    let (s, c) = (sin_fine(yaw), cos_fine(yaw));
    [
        (right * c + fwd * s) / ONE14,
        (-right * s + fwd * c) / ONE14,
    ]
}

/// Moves the horizontal vector `v` towards `target` by at most `step`
/// (in length).
pub fn approach(v: &mut [i32; 2], target: [i32; 2], step: i32) {
    let d = [target[0] - v[0], target[1] - v[1]];
    let l = len2(d[0], d[1]);
    if l <= step {
        *v = target;
    } else {
        v[0] += d[0] * step / l;
        v[1] += d[1] * step / l;
    }
}

/// Shortens the horizontal vector `v` by `step` (to nothing at most).
pub fn shorten(v: &mut [i32; 2], step: i32) {
    let l = len2(v[0], v[1]);
    if l <= step {
        *v = [0, 0];
    } else {
        v[0] = v[0] * (l - step) / l;
        v[1] = v[1] * (l - step) / l;
    }
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
