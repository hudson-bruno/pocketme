//! Step G3: the loose bodies (SPEC 3.6, "Chunks" and "Boulders"): the
//! chunks a seam or the wreck bursts into, which roll downhill and are
//! picked up into the ship's hold, and the boulders the cutter sets
//! loose, which roll and stop as rocks.
//!
//! Places are fine units (`lander::FINE`). A chunk's speed is kept in
//! 1/65536 m a tick (fine >> 4, an `i16`: up to 30 m/s) to keep the 24
//! chunks small; a boulder's in fine a tick.

use super::Below;
use super::lander::{FINE, MPS, TO_WORLD};
use super::site;
use super::tune::{
    BOULDER_DRAG, BOULDER_HIT, BOULDER_LIFE, BOULDER_PUSH, BOULDER_R, BOULDER_TOP, CHUNK_DRAG,
    CHUNK_LIFE, CHUNK_PUSH, CHUNK_R, CHUNK_STOP_TENTHS, CHUNKS, KNOCK, PICKUP_R, PILOT_H, PILOT_R,
    SHIP_SOLID, STEP_UP, TAN5, THROW_OUT, THROW_UP, WALK_EDGE, WRECK_SOLID,
};
use super::walker::{self, acc, apart, ground};
use crate::fixed::{V3, cos, hash, isqrt, sin};
use crate::market::GOODS;
use crate::world::M;

/// A chunk's states.
pub const CHUNK_NONE: u8 = 0;
pub const CHUNK_AIR: u8 = 1;
pub const CHUNK_ROLL: u8 = 2;
pub const CHUNK_REST: u8 = 3;

/// A tonne of a good, loose on the ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk {
    /// Its middle (fine).
    pub p: V3,
    /// Its speed (1/65536 m a tick).
    pub v: [i16; 3],
    pub good: u8,
    pub state: u8,
    /// Ticks since the burst.
    pub t: u16,
    /// HOLD FULL was said for it.
    pub full: bool,
}

impl Chunk {
    pub const NONE: Chunk = Chunk {
        p: [0; 3],
        v: [0; 3],
        good: 0,
        state: CHUNK_NONE,
        t: 0,
        full: false,
    };

    pub fn at(&self) -> V3 {
        self.p.map(|v| v >> TO_WORLD)
    }
}

/// A boulder's states.
pub const ROCK_NONE: u8 = 0;
pub const ROCK_REST: u8 = 1;
pub const ROCK_ROLL: u8 = 2;
pub const ROCK_STOPPED: u8 = 3;

/// A boulder (radius `BOULDER_R`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rock {
    /// Its middle (fine) and its speed across (fine a tick).
    pub p: V3,
    pub v: [i32; 2],
    pub state: u8,
    /// It has hit the pilot on this pass.
    pub hit: bool,
    /// Ticks rolling.
    pub t: u16,
    /// How far it has rolled (1/256 m; its turn in the picture).
    pub rolled: u16,
}

impl Rock {
    pub const NONE: Rock = Rock {
        p: [0; 3],
        v: [0; 2],
        state: ROCK_NONE,
        hit: false,
        t: 0,
        rolled: 0,
    };

    pub fn at(&self) -> V3 {
        self.p.map(|v| v >> TO_WORLD)
    }
}

/// The boulders at rest where the site put them.
pub fn rocks_at(b: &mut Below) {
    for (k, r) in b.rocks.iter_mut().enumerate() {
        *r = match b.places.boulders[k] {
            Some([x, z]) => {
                let y = site::surface(&b.site, x, z) + BOULDER_R;
                Rock {
                    p: [x << TO_WORLD, y << TO_WORLD, z << TO_WORLD],
                    state: ROCK_REST,
                    ..Rock::NONE
                }
            }
            None => Rock::NONE,
        };
    }
}

/// The note for a tonne picked up.
pub fn got(good: u8) -> &'static str {
    match good {
        0 => "+1 T GRAIN",
        1 => "+1 T ICE",
        2 => "+1 T ORE",
        _ => "+1 T SPARES",
    }
}

/// `n` chunks of `good` burst out of (x, y, z) (world units): out at
/// 2..4 m/s in turns spread round, up at 3 m/s. Returns how many found
/// a place (always `n` while `CHUNKS` holds a whole site).
pub fn burst(b: &mut Below, at: V3, good: u8, n: u8) -> u8 {
    let mut made = 0;
    let unit = FINE >> 4;
    for slot in b.chunks.iter_mut() {
        if made == n {
            break;
        }
        if slot.state != CHUNK_NONE {
            continue;
        }
        let h = hash(b.t ^ (u32::from(made) << 20), 0xC4_0C);
        let a = (u32::from(made) * 65536 / u32::from(n.max(1)) + (h & 0x1FFF)) as u16;
        let span = THROW_OUT[1] - THROW_OUT[0];
        let out = (THROW_OUT[0] * 256 + (((h >> 16) & 0xFF) as i32) * span) * (unit / 60) / 256;
        *slot = Chunk {
            p: [
                at[0] << TO_WORLD,
                (at[1] + M / 2) << TO_WORLD,
                at[2] << TO_WORLD,
            ],
            v: [
                ((sin(a) * out) >> 14) as i16,
                (THROW_UP * unit / 60) as i16,
                ((cos(a) * out) >> 14) as i16,
            ],
            good: good.min(GOODS as u8 - 1),
            state: CHUNK_AIR,
            t: 0,
            full: false,
        };
        made += 1;
    }
    made
}

/// The solid things a body bumps into (the ship and the wreck): it
/// stops there. World units.
fn bumps(b: &Below, at: [i32; 2], r: i32) -> bool {
    let ship = super::lander::at(b);
    apart(at, [ship[0], ship[2]]) < SHIP_SOLID + r || apart(at, b.places.wreck) < WRECK_SOLID + r
}

/// The roll's push down the slope at (x, z) (world): `k`/100 of g times
/// the gradient, in fine a tick^2; and whether the slope is under 5
/// degrees.
fn downhill(b: &Below, x: i32, z: i32, k: i32) -> ([i32; 2], bool) {
    let (gx, gz) = site::slope(&b.site, x, z);
    let flat = i64::from(gx).pow(2) + i64::from(gz).pow(2) < i64::from(TAN5).pow(2);
    let a = i64::from(b.g) * i64::from(k) / 100;
    (
        [
            (-a * i64::from(gx) / 16384) as i32,
            (-a * i64::from(gz) / 16384) as i32,
        ],
        flat,
    )
}

/// Slows (x, z) by `by`, to 0 at most.
fn slow(v: [i32; 2], by: i32) -> [i32; 2] {
    let d = isqrt((i64::from(v[0]).pow(2) + i64::from(v[1]).pow(2)) as u64) as i32;
    if d <= by {
        [0, 0]
    } else {
        [
            v[0] - (i64::from(v[0]) * i64::from(by) / i64::from(d)) as i32,
            v[1] - (i64::from(v[1]) * i64::from(by) / i64::from(d)) as i32,
        ]
    }
}

fn len(v: [i32; 2]) -> i32 {
    isqrt((i64::from(v[0]).pow(2) + i64::from(v[1]).pow(2)) as u64) as i32
}

/// One tick of every chunk: flight, roll, stop, pickup.
#[inline(never)]
pub fn chunks(b: &mut Below) {
    let g16 = b.g >> 4;
    let stop = CHUNK_STOP_TENTHS * (MPS >> 4) / 10;
    let drag = acc(CHUNK_DRAG) >> 4;
    let r_fine = CHUNK_R << TO_WORLD;
    for k in 0..CHUNKS {
        let mut c = b.chunks[k];
        if c.state == CHUNK_NONE {
            continue;
        }
        c.t = c.t.saturating_add(1);
        if c.state == CHUNK_AIR || c.state == CHUNK_ROLL {
            let mut v = [i32::from(c.v[0]), i32::from(c.v[1]), i32::from(c.v[2])];
            let mut flat = true;
            if c.state == CHUNK_AIR {
                v[1] -= g16;
            } else {
                let w = c.at();
                let (a, fl) = downhill(b, w[0], w[2], CHUNK_PUSH);
                flat = fl;
                let h = slow([v[0], v[2]], drag);
                v[0] = h[0] + (a[0] >> 4);
                v[2] = h[1] + (a[1] >> 4);
                v[1] = 0;
            }
            let mut n = [
                c.p[0] + (v[0] << 4),
                c.p[1] + (v[1] << 4),
                c.p[2] + (v[2] << 4),
            ];
            let w = [n[0] >> TO_WORLD, n[2] >> TO_WORLD];
            let floor = ground(b, n[0], n[2]) + r_fine;
            let wall = floor > c.p[1] + (STEP_UP << TO_WORLD) && c.state == CHUNK_ROLL;
            if wall || bumps(b, w, CHUNK_R) || len(w) > WALK_EDGE - 2 * M {
                n[0] = c.p[0];
                n[2] = c.p[2];
                v[0] = 0;
                v[2] = 0;
            }
            let floor = ground(b, n[0], n[2]) + r_fine;
            if c.state == CHUNK_AIR {
                if n[1] <= floor {
                    n[1] = floor;
                    v[1] = 0;
                    c.state = CHUNK_ROLL;
                }
            } else {
                n[1] = floor;
            }
            c.p = n;
            let lim = i32::from(i16::MAX);
            c.v = v.map(|x| x.clamp(-lim, lim) as i16);
            let slow_now = len([v[0], v[2]]) < stop;
            if c.state == CHUNK_ROLL && ((slow_now && flat) || c.t >= CHUNK_LIFE) {
                c.state = CHUNK_REST;
                c.v = [0; 3];
            } else if c.state == CHUNK_ROLL && slow_now && c.t >= 30 {
                // Slow on a slope: it rests where the roll's push does not
                // beat the drag (0.6 g tan under 2 m/s^2).
                let (a, _) = downhill(b, w[0], w[1], CHUNK_PUSH);
                if len(a) >> 4 <= drag {
                    c.state = CHUNK_REST;
                    c.v = [0; 3];
                }
            }
        }
        b.chunks[k] = c;
    }
    pick_up(b);
}

/// Chunks within `PICKUP_R` of the pilot go into the hold while it has
/// room; with the hold full, HOLD FULL once per chunk.
fn pick_up(b: &mut Below) {
    let me = b.pilot.at();
    for k in 0..CHUNKS {
        let c = b.chunks[k];
        if c.state == CHUNK_NONE {
            continue;
        }
        let at = c.at();
        if apart([at[0], at[2]], [me[0], me[2]]) > PICKUP_R
            || at[1] < me[1] - M
            || at[1] > me[1] + PILOT_H + M
        {
            continue;
        }
        if b.cargo.total() < u32::from(b.room) {
            let g = usize::from(c.good);
            b.cargo.0[g] = b.cargo.0[g].saturating_add(1);
            b.haul = b.haul.saturating_add(1);
            b.chunks[k] = Chunk::NONE;
            b.note = Some(got(c.good));
        } else if !c.full {
            b.chunks[k].full = true;
            b.note = Some("HOLD FULL");
        }
    }
}

/// One tick of every boulder: rolling ones roll, stop, and hit the
/// pilot.
#[inline(never)]
pub fn rocks(b: &mut Below) {
    let r_fine = BOULDER_R << TO_WORLD;
    let top = BOULDER_TOP * MPS;
    let stop = CHUNK_STOP_TENTHS * MPS / 10;
    for k in 0..b.rocks.len() {
        let mut r = b.rocks[k];
        if r.state != ROCK_ROLL {
            continue;
        }
        r.t = r.t.saturating_add(1);
        let w = r.at();
        let (a, flat) = downhill(b, w[0], w[2], BOULDER_PUSH);
        let mut v = [r.v[0] + a[0], r.v[1] + a[1]];
        if flat {
            v = slow(v, acc(BOULDER_DRAG));
        }
        let s = len(v);
        if s > top {
            v = [
                (i64::from(v[0]) * i64::from(top) / i64::from(s)) as i32,
                (i64::from(v[1]) * i64::from(top) / i64::from(s)) as i32,
            ];
        }
        let n = [r.p[0] + v[0], r.p[2] + v[1]];
        let nw = [n[0] >> TO_WORLD, n[1] >> TO_WORLD];
        let floor = ground(b, n[0], n[1]) + r_fine;
        let wall = floor > r.p[1] + (STEP_UP << TO_WORLD);
        if wall || bumps(b, nw, BOULDER_R) || len(nw) > WALK_EDGE {
            r.state = ROCK_STOPPED;
            r.v = [0, 0];
        } else {
            r.rolled = r.rolled.wrapping_add((len(v) >> TO_WORLD) as u16);
            r.p = [n[0], floor, n[1]];
            r.v = v;
            if (len(v) < stop && flat) || r.t >= BOULDER_LIFE {
                r.state = ROCK_STOPPED;
                r.v = [0, 0];
            }
        }
        b.rocks[k] = r;
        hit(b, k);
        // Step G4: it crushes drones low enough in its way, and leaves a
        // trail of dust while it rolls.
        if r.state == ROCK_ROLL {
            super::drones::crush(b, r.at());
            if r.t.is_multiple_of(3) {
                let a = r.at();
                let g = [a[0], a[1] - BOULDER_R, a[2]];
                super::sparks::add(&mut b.sparks, g, super::sparks::TRAIL, (r.t / 3 % 16) as u8);
            }
        }
    }
}

/// A rolling boulder that meets the pilot costs `BOULDER_HIT` and
/// knocks him away at `KNOCK` m/s, once a pass.
fn hit(b: &mut Below, k: usize) {
    let r = b.rocks[k];
    let at = r.at();
    let me = b.pilot.at();
    let d = apart([at[0], at[2]], [me[0], me[2]]);
    let reach = BOULDER_R + PILOT_R;
    if d > reach + 2 * M {
        b.rocks[k].hit = false;
        return;
    }
    let level = me[1] < at[1] + BOULDER_R && me[1] + PILOT_H > at[1] - BOULDER_R;
    if r.hit || d >= reach || !level || r.state != ROCK_ROLL {
        return;
    }
    b.rocks[k].hit = true;
    let pl = &mut b.pilot;
    pl.suit -= BOULDER_HIT * 60;
    let (dx, dz) = (me[0] - at[0], me[2] - at[2]);
    let d = d.max(1);
    pl.v[0] = dx * KNOCK * MPS / d;
    pl.v[2] = dz * KNOCK * MPS / d;
    pl.v[1] = 2 * MPS;
    pl.ground = false;
    pl.air = 0;
    b.jolt = 12;
    walker::suit_check(b);
}

/// Tonnes loose on the ground now (the tests' count).
pub fn loose_tonnes(b: &Below) -> u32 {
    b.chunks.iter().filter(|c| c.state != CHUNK_NONE).count() as u32
}
