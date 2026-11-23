//! Step G4: the props (SPEC 3.10, "Props"): small plants and rocks
//! strewn over the site, one try per 8 m cell, kept or not by the
//! cell's hash and the look's density. Nothing is stored for them: a
//! cell's prop (if any), its place and its kind come from the site's
//! seed whenever they are needed. A rock prop is solid (the pilot walks
//! round it); a plant lies flat for 3 s when jets (the ship's or the
//! pilot's) blow on it (`Below::flats`, at most 16 at once, the oldest
//! reused).
//!
//! Drawn up to 24, 48 or 72 m by detail level; first to go when the
//! picture is short of time (SPEC 3.16, cut 1).

use super::Below;
use super::site::{self, Site};
use super::tune::{
    FLAT_R, FLAT_TICKS, PROP_CELL, PROP_CLEAR, PROP_DENSITY, PROP_NEAR_THING, ROCK_EVERY, SEAMS,
};
use super::walker::apart;
use crate::fixed::{V3, hash};

/// A prop: where (world units, on the ground), its turn, and its kind
/// (0: rock, 1: the look's plant).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prop {
    pub at: V3,
    pub yaw: u16,
    pub rock: bool,
}

/// A plant laid flat: its cell, ticks left (0: a free slot).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flat {
    pub cx: i8,
    pub cz: i8,
    pub t: u16,
}

impl Flat {
    pub const NONE: Flat = Flat { cx: 0, cz: 0, t: 0 };
}

/// The cell holding (x, z) (world units).
pub fn cell_of(x: i32, z: i32) -> (i32, i32) {
    (x.div_euclid(PROP_CELL), z.div_euclid(PROP_CELL))
}

/// Cell (cx, cz)'s try: its place (x, z, world units) and its hash
/// when the look's density keeps it (nothing else checked yet).
pub fn spot(s: &Site, cx: i32, cz: i32) -> Option<(i32, i32, u32)> {
    let density = PROP_DENSITY[usize::from(s.look % 8)];
    let h = hash(
        s.seed ^ (cx as u32).wrapping_mul(0x9E37_79B9),
        cz as u32 ^ 0x960F,
    );
    if h & 0xFF >= density {
        return None;
    }
    let x = cx * PROP_CELL + ((h >> 8) % PROP_CELL as u32) as i32;
    let z = cz * PROP_CELL + ((h >> 19) % PROP_CELL as u32) as i32;
    Some((x, z, h))
}

/// Within `r` across (squares compared: no root).
fn within(a: [i32; 2], b: [i32; 2], r: i32) -> bool {
    let (dx, dz) = (i64::from(a[0] - b[0]), i64::from(a[1] - b[1]));
    dx * dx + dz * dz < i64::from(r) * i64::from(r)
}

/// The prop at a kept spot, unless it falls on the pad, by a seam, the
/// wreck or a boulder's first place, or in water.
pub fn keep(b: &Below, x: i32, z: i32, h: u32) -> Option<Prop> {
    if within([x, z], [0, 0], PROP_CLEAR) {
        return None;
    }
    let p = &b.places;
    let near = |q: [i32; 2]| within([x, z], q, PROP_NEAR_THING);
    if p.seams[..SEAMS].iter().any(|&q| near(q)) || near(p.wreck) {
        return None;
    }
    if p.boulders.iter().flatten().any(|&q| near(q)) {
        return None;
    }
    let s = &b.site;
    let y = site::height(s, x, z);
    if y < s.wet {
        return None;
    }
    Some(Prop {
        at: [x, y, z],
        yaw: (h >> 4) as u16,
        rock: (h >> 16).is_multiple_of(ROCK_EVERY),
    })
}

/// The prop of cell (cx, cz), if it has one.
pub fn at_cell(b: &Below, cx: i32, cz: i32) -> Option<Prop> {
    let (x, z, h) = spot(&b.site, cx, cz)?;
    keep(b, x, z, h)
}

/// How far the plant of cell (cx, cz) is laid down (0 upright, 256
/// flat).
pub fn laid(b: &Below, cx: i32, cz: i32) -> i32 {
    for f in &b.flats {
        if f.t > 0 && i32::from(f.cx) == cx && i32::from(f.cz) == cz {
            // Down at once, up again over its last half second.
            return (i32::from(f.t) * 256 / 30).min(256);
        }
    }
    0
}

/// Jets at `at` (world units) blowing over `r`: the plants there lie
/// down.
pub fn blow(b: &mut Below, at: V3, r: i32) {
    let reach = r + FLAT_R;
    let (c0, c1) = (
        cell_of(at[0] - reach, at[2] - reach),
        cell_of(at[0] + reach, at[2] + reach),
    );
    for cz in c0.1..=c1.1 {
        for cx in c0.0..=c1.0 {
            let Some(p) = at_cell(b, cx, cz) else {
                continue;
            };
            if p.rock || apart([p.at[0], p.at[2]], [at[0], at[2]]) > reach {
                continue;
            }
            flatten(b, cx, cz);
        }
    }
}

/// Lays the plant of cell (cx, cz) down for `FLAT_TICKS`.
fn flatten(b: &mut Below, cx: i32, cz: i32) {
    let (x, z) = (cx.clamp(-127, 127) as i8, cz.clamp(-127, 127) as i8);
    let mut slot = 0;
    for (k, f) in b.flats.iter().enumerate() {
        if f.t > 0 && f.cx == x && f.cz == z {
            slot = k;
            break;
        }
        if f.t < b.flats[slot].t {
            slot = k;
        }
    }
    b.flats[slot] = Flat {
        cx: x,
        cz: z,
        t: FLAT_TICKS,
    };
}

/// One tick of the laid plants.
pub fn tick(b: &mut Below) {
    for f in b.flats.iter_mut() {
        f.t = f.t.saturating_sub(1);
    }
}

/// The rock props in the 3 x 3 cells round (x, z) (world units), as
/// solids (x, z, radius) for the walk; returns how many were put.
pub fn rocks_near(b: &Below, x: i32, z: i32, out: &mut [[i32; 3]], r: i32) -> usize {
    let (cx, cz) = cell_of(x, z);
    let mut n = 0;
    for dz in -1..=1 {
        for dx in -1..=1 {
            if n >= out.len() {
                return n;
            }
            if let Some(p) = at_cell(b, cx + dx, cz + dz)
                && p.rock
            {
                out[n] = [p.at[0], p.at[2], r];
                n += 1;
            }
        }
    }
    n
}

/// Flats in use (the tests' count).
pub fn flats_on(b: &Below) -> usize {
    b.flats.iter().filter(|f| f.t > 0).count()
}
