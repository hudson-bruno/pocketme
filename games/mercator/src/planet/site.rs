//! Step G1: the land of a landing site, from the system's seed (SPEC
//! 3.4). One function, [`height`], gives the ground's height anywhere;
//! the land walk asks it once a corner a picture, so it is the planet
//! part's busiest code (in the flash part by design, MODULES.md 4).
//!
//! Units: x east, z north, y up, in world units (1/256 m), with the pad
//! at (0, 0). Heights stay within about 40 m of 0 and places within
//! 2^20 of the pad, so the sums below are far inside `i32`.

use super::tune::{
    GIANT, LOW, MESA_EASE, MESA_FROM, MESA_LIFT, OCT_SALT, ORIGIN_TRIES, PAD_BLEND, PAD_DRY, PAD_R,
    PLINTH_R, RIDGE_FROM, RISER, SHAPE, SITE_SALT, TERRACE,
};
use crate::fixed::{ONE14, hash, wide};
use crate::world::M;

/// What the height of a site depends on (kept in `Below`, 36 B).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    pub seed: u32,
    pub look: u8,
    /// Where the pad's (0, 0) lies in the noise.
    pub ox: i32,
    pub oz: i32,
    /// The pad's height.
    pub pad: i32,
    /// Water (or glowing lava) fills the ground up to here; `-LOW`: none.
    pub wet: i32,
    /// The ledge's plinth: its middle, its top's height, and 0 for none.
    pub plinth: [i32; 3],
    pub plinth_on: bool,
}

impl Site {
    pub const NONE: Site = Site {
        seed: 0,
        look: 0,
        ox: 0,
        oz: 0,
        pad: 0,
        wet: -LOW,
        plinth: [0; 3],
        plinth_on: false,
    };
}

/// The smooth curve t^2 (3 - 2t), t and the result in 1/16384.
fn smooth(t: i32) -> i32 {
    // t in 0..=16384: t*t below 2^29, the next product below 2^31.
    let t2 = t.wrapping_mul(t) >> 14;
    t2.wrapping_mul(3 * ONE14 - 2 * t) >> 14
}

/// One octave of value noise at (x, z): cells `1 << shift` units wide,
/// the result -16384..=16384.
fn octave(salt: u32, x: i32, z: i32, shift: u32) -> i32 {
    let (ix, iz) = (x >> shift, z >> shift);
    let mask = (1i32 << shift) - 1;
    let sx = smooth((((x & mask) as i64 * i64::from(ONE14)) >> shift) as i32);
    let sz = smooth((((z & mask) as i64 * i64::from(ONE14)) >> shift) as i32);
    let v = |a: i32, b: i32| {
        let k = (a as u32).wrapping_mul(0x9E37) ^ (b as u32).wrapping_mul(0x85EB);
        (hash(salt, k) % 32_769) as i32 - ONE14
    };
    let (v00, v10) = (v(ix, iz), v(ix + 1, iz));
    let (v01, v11) = (v(ix, iz + 1), v(ix + 1, iz + 1));
    // Differences below 2^15 times weights at most 2^14: below 2^29.
    let a = v00 + (((v10 - v00) * sx) >> 14);
    let b = v01 + (((v11 - v01) * sx) >> 14);
    a + (((b - a) * sz) >> 14)
}

/// The noise's height at (x, z) of the noise's own plane, shaped by the
/// look: no pad, no plinth.
pub fn raw(seed: u32, look: u8, x: i32, z: i32) -> i32 {
    let sh = &SHAPE[usize::from(look % 8)];
    let n0 = octave(seed ^ OCT_SALT[0], x, z, 15);
    let n1 = octave(seed ^ OCT_SALT[1], x, z, 13);
    let n2 = octave(seed ^ OCT_SALT[2], x, z, 11);
    // Values at most 2^14 times heights below 2^13: below 2^27.
    let c0 = (n0 * sh.amp[0]) >> 14;
    let mut c1 = (n1 * sh.amp[1]) >> 14;
    let mut c2 = (n2 * sh.amp[2]) >> 14;
    match look {
        // DESERT: the 32 m octave folded, so the dunes' ridges are sharp.
        1 => c1 = sh.amp[1] - 2 * c1.abs(),
        // ICE: the 8 m octave only on the ridges.
        2 => {
            let k = ((n1 - RIDGE_FROM) * 4).clamp(0, ONE14);
            c2 = (c2 * k) >> 14;
        }
        _ => {}
    }
    let mut h = c0 + c1 + c2;
    match look {
        // LAVA: 3 m terraces, each rising over its top 0.6 m.
        3 => {
            let q = h.div_euclid(TERRACE);
            let r = h.rem_euclid(TERRACE);
            let rise = (r - (TERRACE - RISER)).max(0) * (TERRACE / RISER.max(1));
            h = q * TERRACE + rise.min(TERRACE);
        }
        // RUST: mesas, where the 128 m octave is high.
        7 => {
            let t = ((c0 - MESA_FROM) * (ONE14 / MESA_EASE.max(1))).clamp(0, ONE14);
            h += (smooth(t) * MESA_LIFT) >> 14;
        }
        _ => {}
    }
    h
}

/// The ground's height at (x, z): the noise, the pad and the plinth.
/// Water is not in it: see [`surface`].
#[inline(never)]
pub fn height(s: &Site, x: i32, z: i32) -> i32 {
    let mut h = raw(s.seed, s.look, x.wrapping_add(s.ox), z.wrapping_add(s.oz));
    // The pad: flat within PAD_R, eased in by the square of the distance
    // up to PAD_BLEND.
    let r2 = wide(x, x) + wide(z, z);
    let (a, b) = (wide(PAD_R, PAD_R), wide(PAD_BLEND, PAD_BLEND));
    if r2 <= a {
        h = s.pad;
    } else if r2 < b {
        let t = ((r2 - a) * i64::from(ONE14) / (b - a)) as i32;
        h = s.pad + (((h - s.pad) * t) >> 14);
    }
    if s.plinth_on {
        let (dx, dz) = (x.wrapping_sub(s.plinth[0]), z.wrapping_sub(s.plinth[1]));
        if dx.abs() <= PLINTH_R
            && dz.abs() <= PLINTH_R
            && wide(dx, dx) + wide(dz, dz) <= wide(PLINTH_R, PLINTH_R)
        {
            h = s.plinth[2];
        }
    }
    h
}

/// Where a foot or a ship would stand: the ground, or the water over it.
pub fn surface(s: &Site, x: i32, z: i32) -> i32 {
    height(s, x, z).max(s.wet)
}

/// The slope at (x, z) as (east, north) rise per run, in 1/16384
/// (taken over 2 m).
pub fn slope(s: &Site, x: i32, z: i32) -> (i32, i32) {
    let gx = surface(s, x + M, z) - surface(s, x - M, z);
    let gz = surface(s, x, z + M) - surface(s, x, z - M);
    // Rises below 2^14 (64 m) times 2^14 over 2^9.
    (gx * (ONE14 / (2 * M)), gz * (ONE14 / (2 * M)))
}

/// Whether the slope at (x, z) is at least `tan` (in 1/16384).
pub fn steeper(s: &Site, x: i32, z: i32, tan: i32) -> bool {
    let (gx, gz) = slope(s, x, z);
    wide(gx, gx) + wide(gz, gz) >= wide(tan, tan)
}

/// A system's site (None for the giant): the noise's origin is picked
/// from [`ORIGIN_TRIES`] tries as the one with the quietest land round
/// the pad and dry ground under it.
#[inline(never)]
pub fn make(h: u32, look: u8) -> Option<Site> {
    if look == GIANT {
        return None;
    }
    let seed = hash(h, SITE_SALT);
    let wet = SHAPE[usize::from(look % 8)].wet;
    let mut best = (i64::MAX, 0, 0, 0);
    for t in 0..ORIGIN_TRIES {
        let ox = (hash(seed, 2 * t) & 0x00FF_FFFF) as i32 - 0x0080_0000;
        let oz = (hash(seed, 2 * t + 1) & 0x00FF_FFFF) as i32 - 0x0080_0000;
        let h0 = raw(seed, look, ox, oz);
        let mut score = 0i64;
        for k in 0..8u16 {
            let a = k.wrapping_mul(8192);
            let (px, pz) = (
                (crate::fixed::sin(a) * (PAD_BLEND / 256)) >> 6,
                (crate::fixed::cos(a) * (PAD_BLEND / 256)) >> 6,
            );
            score += i64::from((raw(seed, look, ox + px, oz + pz) - h0).abs());
        }
        // Water under the pad costs 32 times its depth.
        score += i64::from((wet + PAD_DRY - h0).max(0)) * 32;
        if score < best.0 {
            best = (score, ox, oz, h0);
        }
    }
    let (_, ox, oz, h0) = best;
    Some(Site {
        seed,
        look,
        ox,
        oz,
        pad: h0.max(wet + PAD_DRY),
        wet,
        plinth: [0; 3],
        plinth_on: false,
    })
}
