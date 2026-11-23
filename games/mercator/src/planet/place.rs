//! Step G1: where the site's things go (SPEC 3.4, "Placement"): the
//! seams, the ledge, the wreck, the boulders and the landing start,
//! from the seed with a bounded number of tries each. A spot is only
//! taken when a walk from the pad reaches it ([`walk_to`]), so every
//! site can be played (checked over 7,000 sites by `tests/planet.rs`).
//! G1 places them and keeps them; G2 and later draw and use them.

use super::site::{self, Site};
use super::tune::{
    APART, BOULDER_FAR, BOULDER_NEAR, BOULDERS, DRONES_BASE, DRONES_MAX, HOP, LEDGE_NEAR,
    PLACE_SALT, PLINTH_BASE, SEAM_FAR, SEAM_NEAR, SEAMS, START_OUT, START_UP, TAN8, TAN10, TAN15,
    TRIES, WALK_DIAG, WALK_GRID, WALK_STEP, WRECK_FAR, WRECK_NEAR,
};
use crate::fixed::{cos, hash, sin, wide};
use crate::world::M;

/// The site's things (kept in `Below`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Places {
    /// Seams 1 to 3, then seam 4 on the ledge: (x, z).
    pub seams: [[i32; 2]; SEAMS],
    pub wreck: [i32; 2],
    /// A boulder over each guarded seam, when one fits.
    pub boulders: [Option<[i32; 2]>; BOULDERS],
    pub drones: u8,
    /// Where the landing starts: (x, y, z).
    pub start: [i32; 3],
    /// The tries all placements took (the tests check the bound).
    pub tries: u16,
}

impl Places {
    pub const NONE: Places = Places {
        seams: [[0; 2]; SEAMS],
        wreck: [0; 2],
        boulders: [None; BOULDERS],
        drones: 0,
        start: [0; 3],
        tries: 0,
    };
}

/// The most tries all placements may take: each seam, the ledge and
/// the wreck two rounds of [`TRIES`] (the second round lets the slope
/// rule go), each boulder one round.
pub const MAX_TRIES: u16 = (TRIES * 2 * 5 + TRIES * BOULDERS as u32) as u16;

/// A spot `near..far` from (cx, cz) in direction and distance from the
/// hash of `key`.
fn spot(seed: u32, key: u32, c: [i32; 2], near: i32, far: i32) -> [i32; 2] {
    let h = hash(seed ^ PLACE_SALT, key);
    let a = h as u16;
    let d = near + ((h >> 16) % (far - near).max(1) as u32) as i32;
    // d below 2^16, sin and cos at most 2^14: below 2^30.
    [c[0] + ((sin(a) * d) >> 14), c[1] + ((cos(a) * d) >> 14)]
}

fn apart(p: [i32; 2], others: &[[i32; 2]]) -> bool {
    others.iter().all(|o| {
        let (dx, dz) = (p[0] - o[0], p[1] - o[1]);
        wide(dx, dx) + wide(dz, dz) > wide(APART, APART)
    })
}

/// Whether a walk on the 2 m grid, in a straight line of steps from
/// the pad to the grid point nearest `to`, reaches it: each step at most
/// 35 degrees, and at most one jet hop of up to 5 m. The proof's walk
/// (`reach`) finds at least this path, so a spot this accepts passes it.
#[inline(never)]
pub fn walk_to(s: &Site, to: [i32; 2]) -> bool {
    let g = |v: i32| (v + v.signum() * (WALK_GRID / 2)) / WALK_GRID;
    let (tx, tz) = (g(to[0]), g(to[1]));
    let (ax, az) = (tx.abs(), tz.abs());
    let (sx, sz) = (tx.signum(), tz.signum());
    let (mut x, mut z) = (0i32, 0i32);
    let mut h = site::surface(s, 0, 0);
    let mut hop = true;
    // Bresenham's walk: the long axis steps every time, the short one
    // when its error runs over (a diagonal step).
    let n = ax.max(az);
    let mut err = 0;
    for _ in 0..n {
        let (mut dx, mut dz) = (0, 0);
        if ax >= az {
            dx = sx;
            err += az;
            if 2 * err >= ax {
                err -= ax;
                dz = sz;
            }
        } else {
            dz = sz;
            err += ax;
            if 2 * err >= az {
                err -= az;
                dx = sx;
            }
        }
        x += dx;
        z += dz;
        let nh = site::surface(s, x * WALK_GRID, z * WALK_GRID);
        let step = (nh - h).abs();
        let max = if dx != 0 && dz != 0 {
            WALK_DIAG
        } else {
            WALK_STEP
        };
        if step > max {
            if hop && step <= HOP {
                hop = false;
            } else {
                return false;
            }
        }
        h = nh;
    }
    true
}

/// Places everything for `s` (the ledge's plinth is set into `s`).
#[inline(never)]
pub fn place(s: &mut Site, danger: u8) -> Places {
    let mut p = Places::NONE;
    let seed = s.seed;
    let mut key = 0u32;
    let mut tries = 0u16;
    let dry = |s: &Site, q: [i32; 2]| site::height(s, q[0], q[1]) > s.wet;
    // Seams 1 to 3: a rock face (8 degrees or more), apart, dry,
    // reachable; in the second round any slope.
    for i in 0..SEAMS - 1 {
        let mut got = None;
        let mut last = [SEAM_NEAR, 0];
        'rounds: for round in 0..2 {
            for _ in 0..TRIES {
                key += 1;
                tries += 1;
                let q = spot(seed, key, [0, 0], SEAM_NEAR, SEAM_FAR);
                last = q;
                if (round == 1 || site::steeper(s, q[0], q[1], TAN8))
                    && apart(q, &p.seams[..i])
                    && dry(s, q)
                    && walk_to(s, q)
                {
                    got = Some(q);
                    break 'rounds;
                }
            }
        }
        p.seams[i] = got.unwrap_or(last);
    }
    // Seam 4 on its plinth, 3 to 5 m high (3 m in the second round):
    // the hop onto it is the walk's one hop.
    let mut got = None;
    let mut last = ([LEDGE_NEAR, 0], PLINTH_BASE);
    'ledge: for round in 0..2 {
        for _ in 0..TRIES {
            key += 1;
            tries += 1;
            let q = spot(seed, key, [0, 0], LEDGE_NEAR, SEAM_FAR);
            let ph = if round == 0 {
                PLINTH_BASE + (hash(seed ^ PLACE_SALT, key + 1000) % 3) as i32 * M
            } else {
                PLINTH_BASE
            };
            last = (q, ph);
            if !apart(q, &p.seams[..SEAMS - 1]) || !dry(s, q) {
                continue;
            }
            s.plinth = [q[0], q[1], site::height(s, q[0], q[1]) + ph];
            s.plinth_on = true;
            let ok = walk_to(s, q);
            s.plinth_on = false;
            if ok {
                got = Some(q);
                break 'ledge;
            }
        }
    }
    let (q, ph) = got.map_or(last, |q| (q, s.plinth[2]));
    if got.is_none() {
        s.plinth = [q[0], q[1], site::height(s, q[0], q[1]) + ph];
    }
    s.plinth_on = true;
    p.seams[SEAMS - 1] = q;
    // The wreck: under 10 degrees (any slope in the second round),
    // apart from the seams, dry, reachable.
    let mut got = None;
    let mut last = [WRECK_NEAR, 0];
    'wreck: for round in 0..2 {
        for _ in 0..TRIES {
            key += 1;
            tries += 1;
            let q = spot(seed, key, [0, 0], WRECK_NEAR, WRECK_FAR);
            last = q;
            if (round == 1 || !site::steeper(s, q[0], q[1], TAN10))
                && apart(q, &p.seams)
                && dry(s, q)
                && walk_to(s, q)
            {
                got = Some(q);
                break 'wreck;
            }
        }
    }
    p.wreck = got.unwrap_or(last);
    // Drones guard the first seams; a boulder waits uphill of each
    // guarded seam on a slope of 15 degrees or more.
    p.drones = DRONES_BASE.saturating_add(danger).min(DRONES_MAX);
    for i in 0..usize::from(p.drones).min(BOULDERS) {
        let c = p.seams[i];
        let (gx, gz) = site::slope(s, c[0], c[1]);
        for _ in 0..TRIES {
            key += 1;
            tries += 1;
            let q = spot(seed, key, c, BOULDER_NEAR, BOULDER_FAR);
            let up = wide(q[0] - c[0], gx) + wide(q[1] - c[1], gz) > 0;
            if up && site::steeper(s, q[0], q[1], TAN15) {
                p.boulders[i] = Some(q);
                break;
            }
        }
    }
    // The landing start.
    let q = spot(seed, 0xFFFF, [0, 0], START_OUT, START_OUT + 1);
    p.start = [q[0], site::surface(s, q[0], q[1]) + START_UP, q[1]];
    p.tries = tries;
    p
}

/// For the tests only (the chip's linker drops it): whether every
/// placed thing can be reached from the pad, by the proof's rules (SPEC
/// 3.4): walks on the 2 m grid, steps of at most 35 degrees (8
/// neighbours), and at most one jet hop of up to 5 m, over the square
/// `±half` grid steps round the pad. `h`, `seen` and `queue` are the
/// caller's (`(2 half + 1)^2` long; `queue` twice that). Returns a bit
/// per thing not reached: seams 0..4, then the wreck (bit 4).
#[doc(hidden)]
pub fn reach(
    s: &Site,
    p: &Places,
    half: i32,
    h: &mut [i32],
    seen: &mut [u8],
    queue: &mut [u32],
) -> u8 {
    let n = 2 * half + 1;
    let idx = |x: i32, z: i32| ((z + half) * n + (x + half)) as usize;
    for z in -half..=half {
        for x in -half..=half {
            h[idx(x, z)] = site::surface(s, x * WALK_GRID, z * WALK_GRID);
        }
    }
    // seen bit 1: reached without the hop, bit 2: with it used. Each
    // (place, bit) enters the queue once: a breadth-first walk.
    for v in seen.iter_mut() {
        *v = 0;
    }
    let (mut head, mut tail) = (0usize, 0usize);
    seen[idx(0, 0)] = 1;
    queue[tail] = (idx(0, 0) as u32) << 2 | 1;
    tail += 1;
    while head < tail {
        let e = queue[head];
        head += 1;
        let (i, bit) = ((e >> 2) as i32, (e & 3) as u8);
        let (x, z) = (i % n - half, i / n - half);
        for (dx, dz) in [
            (1, 0),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            let (nx, nz) = (x + dx, z + dz);
            if nx.abs() > half || nz.abs() > half {
                continue;
            }
            let step = (h[idx(nx, nz)] - h[idx(x, z)]).abs();
            let max = if dx != 0 && dz != 0 {
                WALK_DIAG
            } else {
                WALK_STEP
            };
            let add = if step <= max {
                bit
            } else if step <= HOP && bit == 1 {
                2
            } else {
                continue;
            };
            let j = idx(nx, nz);
            if seen[j] & add == 0 {
                seen[j] |= add;
                queue[tail] = (j as u32) << 2 | u32::from(add);
                tail += 1;
            }
        }
    }
    let g = |v: i32| (v + v.signum() * (WALK_GRID / 2)) / WALK_GRID;
    let mut missed = 0u8;
    for (k, q) in p.seams.iter().chain(core::iter::once(&p.wreck)).enumerate() {
        let (x, z) = (g(q[0]), g(q[1]));
        if x.abs() > half || z.abs() > half || seen[idx(x, z)] == 0 {
            missed |= 1 << k;
        }
    }
    missed
}
