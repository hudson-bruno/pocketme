//! Step G4: the drones (SPEC 3.6, "Drones"). Each one guards a seam:
//! it circles it 3 m up at 12 m and 2 m/s; it sees the pilot within
//! 25 m and 60 degrees of its facing, or within 10 m any way round (no
//! line of sight test). Seeing him it stops and aims for a second: a red
//! line from its eye follows him; the last 0.15 s the line stops
//! following (locked, flashing white) and then the bolt flies to where
//! it was locked, at 30 m/s. A jump or a side step in that last instant
//! takes the pilot out of the bolt's way. After a bolt it cools 1.6 s,
//! then aims again if it still sees him; with the pilot past 40 m from
//! its seam it goes back to its round.
//!
//! The cutter hurts it ([`cut`]); a rolling boulder crushes it
//! ([`crush`]); at 0 it bursts ([`burst`]): a short flash, sparks and
//! three parts in the particle pool, and a chunk of SPARES. A drone
//! downed today stays down until the next day, as a cut seam does.
//!
//! The telegraph is never cut (SPEC 3.16).
//!
//! Places are world units (`M` = 1/256 m), from the site's middle.

use super::Below;
use super::cutter::SPARES;
use super::site;
use super::sparks;
use super::tune::{
    BOLT_BASE, BOLT_HIT, BOLT_PER, BOLT_STEP, BOLT_TICKS, BOULDER_R, BURST_TICKS, CRUSH_SLACK,
    DRONE_AIM, DRONE_AIM_H, DRONE_COOL, DRONE_HP, DRONE_HP_PER, DRONE_LEASH, DRONE_LOCK,
    DRONE_ORBIT, DRONE_R, DRONE_SPEED, DRONE_TONNES, DRONE_UP, DRONES_MAX, HIT_FLASH, PILOT_H,
    SEAMS, SIGHT_CONE, SIGHT_FAR, SIGHT_NEAR,
};
use super::walker::{self, apart};
use crate::fixed::{V3, cos, hash, isqrt, sin};
use crate::world::M;

/// A drone's modes.
pub const GONE: u8 = 0;
pub const PATROL: u8 = 1;
pub const AIM: u8 = 2;
pub const COOL: u8 = 3;
pub const BURST: u8 = 4;

/// How many drones `Below` keeps.
pub const N: usize = DRONES_MAX as usize;

/// The orbit's step a tick (65536 a turn): 2 m/s round 12 m.
pub const ORBIT_STEP: u16 =
    (65_536i64 * 100 * DRONE_SPEED as i64 * M as i64 / (628 * DRONE_ORBIT as i64 * 60)) as u16;
/// The patrol's speed a tick (world units), and its speed back to the
/// round (twice that).
const PATROL_STEP: i32 = DRONE_SPEED * M / 60;

/// A drone (kept in `Below`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drone {
    /// Its middle (world units).
    pub p: V3,
    /// Where it faces; where it is on its round (65536 a turn).
    pub yaw: u16,
    pub orbit: u16,
    pub mode: u8,
    /// The seam it guards.
    pub home: u8,
    /// Ticks in this mode.
    pub t: u16,
    /// Its points, in 1/60.
    pub hp: i32,
    /// Where the line points (the pilot's middle, frozen once locked).
    pub lock: V3,
    /// The bolt in flight: its head, its step a tick, ticks left (0:
    /// none).
    pub bolt: V3,
    pub step: [i16; 3],
    pub bolt_t: u8,
    /// Ticks the hit flash lasts.
    pub flash: u8,
}

impl Drone {
    pub const NONE: Drone = Drone {
        p: [0; 3],
        yaw: 0,
        orbit: 0,
        mode: GONE,
        home: 0,
        t: 0,
        hp: 0,
        lock: [0; 3],
        bolt: [0; 3],
        step: [0; 3],
        bolt_t: 0,
        flash: 0,
    };

    /// It is there to be hit (not gone or bursting).
    pub fn alive(&self) -> bool {
        matches!(self.mode, PATROL | AIM | COOL)
    }

    /// Locked: the last `DRONE_LOCK` ticks of the aim (its ticks are
    /// counted 1..=`DRONE_AIM`; the line follows the pilot through tick
    /// `DRONE_AIM - DRONE_LOCK` and stands still from the next one).
    pub fn locked(&self) -> bool {
        self.mode == AIM && self.t > DRONE_AIM - DRONE_LOCK
    }

    /// Its eye (world units): 0.5 m ahead of its middle.
    pub fn eye(&self) -> V3 {
        let r = M / 2;
        [
            self.p[0] + ((sin(self.yaw) * r) >> 14),
            self.p[1],
            self.p[2] + ((cos(self.yaw) * r) >> 14),
        ]
    }
}

/// A drone's points at `danger` (1/60).
pub fn points(danger: u8) -> i32 {
    (DRONE_HP + DRONE_HP_PER * i32::from(danger)) * 60
}

/// A bolt's damage at `danger` (suit points).
pub fn damage(danger: u8) -> i32 {
    BOLT_BASE + BOLT_PER * i32::from(danger)
}

/// The bit of `Below::downed` for drone `k`.
pub const fn bit(k: usize) -> u8 {
    1 << k
}

/// The point on drone `k`'s round at turn `a`.
fn round_at(b: &Below, home: u8, a: u16) -> V3 {
    let s = b.places.seams[usize::from(home) % SEAMS];
    let (x, z) = (
        s[0] + ((sin(a) * (DRONE_ORBIT >> 4)) >> 10),
        s[1] + ((cos(a) * (DRONE_ORBIT >> 4)) >> 10),
    );
    [x, site::surface(&b.site, x, z) + DRONE_UP, z]
}

/// The site's drones at the start of a visit (`places.drones` of them,
/// one a seam), those downed today left out.
pub fn start(b: &mut Below, downed: u8) {
    let n = usize::from(b.places.drones).min(N);
    for k in 0..N {
        b.drones[k] = Drone::NONE;
        if k >= n || downed & bit(k) != 0 {
            continue;
        }
        let home = (k % SEAMS) as u8;
        let orbit = hash(b.site.seed, 0xD0_0E + k as u32) as u16;
        let p = round_at(b, home, orbit);
        b.drones[k] = Drone {
            p,
            yaw: orbit.wrapping_add(0x4000),
            orbit,
            mode: PATROL,
            home,
            hp: points(b.danger),
            ..Drone::NONE
        };
    }
}

/// Moves `p` toward `t` by at most `step`.
fn toward(p: V3, t: V3, step: i32) -> V3 {
    let d = [t[0] - p[0], t[1] - p[1], t[2] - p[2]].map(i64::from);
    let len = isqrt((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as u64) as i64;
    if len <= i64::from(step) {
        return t;
    }
    [0, 1, 2].map(|i| p[i] + (d[i] * i64::from(step) / len) as i32)
}

/// The pilot's middle, where a drone aims.
fn middle(b: &Below) -> V3 {
    let a = b.pilot.at();
    [a[0], a[1] + DRONE_AIM_H, a[2]]
}

/// Drone `k` sees the pilot (on foot, out of the ship).
pub fn sees(b: &Below, k: usize) -> bool {
    if b.phase != super::lander::Phase::Foot {
        return false;
    }
    let d = &b.drones[k];
    let me = b.pilot.at();
    let (dx, dz) = (me[0] - d.p[0], me[2] - d.p[2]);
    let far = apart([dx, dz], [0, 0]);
    if far <= SIGHT_NEAR {
        return true;
    }
    if far > SIGHT_FAR {
        return false;
    }
    let along = i64::from(dx) * i64::from(sin(d.yaw)) + i64::from(dz) * i64::from(cos(d.yaw));
    along >= i64::from(far) * i64::from(SIGHT_CONE)
}

/// The pilot is past the leash of drone `k`'s seam.
fn gone_far(b: &Below, k: usize) -> bool {
    let me = b.pilot.at();
    let s = b.places.seams[usize::from(b.drones[k].home) % SEAMS];
    apart([me[0], me[2]], s) > DRONE_LEASH || b.phase != super::lander::Phase::Foot
}

/// One tick of every drone and its bolt.
#[inline(never)]
pub fn tick(b: &mut Below) {
    for k in 0..N {
        if b.drones[k].bolt_t > 0 {
            bolt(b, k);
        }
        let mut d = b.drones[k];
        d.t = d.t.saturating_add(1);
        d.flash = d.flash.saturating_sub(1);
        match d.mode {
            PATROL => {
                d.orbit = d.orbit.wrapping_add(ORBIT_STEP);
                let goal = round_at(b, d.home, d.orbit);
                let was = d.p;
                // Back to the round at twice the patrol's speed.
                d.p = toward(d.p, goal, PATROL_STEP * 2);
                let (mx, mz) = (d.p[0] - was[0], d.p[2] - was[2]);
                if mx != 0 || mz != 0 {
                    d.yaw = super::yaw_to(mx, mz);
                }
                b.drones[k] = d;
                if sees(b, k) {
                    aim(b, k);
                }
                continue;
            }
            AIM => {
                if gone_far(b, k) {
                    d.mode = PATROL;
                    d.t = 0;
                } else {
                    if !d.locked() {
                        d.lock = middle(b);
                        let me = b.pilot.at();
                        d.yaw = super::yaw_to(me[0] - d.p[0], me[2] - d.p[2]);
                    }
                    if d.t >= DRONE_AIM {
                        fire(&mut d);
                        d.mode = COOL;
                        d.t = 0;
                    }
                }
            }
            COOL => {
                if d.t >= DRONE_COOL {
                    b.drones[k] = d;
                    if !gone_far(b, k) && sees(b, k) {
                        aim(b, k);
                    } else {
                        b.drones[k].mode = PATROL;
                        b.drones[k].t = 0;
                    }
                    continue;
                }
            }
            BURST if d.t >= BURST_TICKS => {
                d.mode = GONE;
            }
            _ => {}
        }
        b.drones[k] = d;
    }
}

/// Drone `k` starts its telegraph (its line on the pilot).
fn aim(b: &mut Below, k: usize) {
    let lock = middle(b);
    let me = b.pilot.at();
    let d = &mut b.drones[k];
    d.mode = AIM;
    d.t = 0;
    d.lock = lock;
    d.yaw = super::yaw_to(me[0] - d.p[0], me[2] - d.p[2]);
}

/// The bolt leaves the eye toward the lock.
fn fire(d: &mut Drone) {
    let e = d.eye();
    let v = [d.lock[0] - e[0], d.lock[1] - e[1], d.lock[2] - e[2]].map(i64::from);
    let len = isqrt((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as u64).max(1) as i64;
    d.step = v.map(|x| (x * i64::from(BOLT_STEP) / len) as i16);
    d.bolt = e;
    d.bolt_t = BOLT_TICKS;
}

/// One tick of drone `k`'s bolt: it hits the pilot when its way this
/// tick passes within `BOLT_HIT` of his upright axis between his feet
/// and his head; it ends in the ground.
fn bolt(b: &mut Below, k: usize) {
    let d = b.drones[k];
    let s = d.step.map(i32::from);
    let a = d.bolt;
    let n = [a[0] + s[0], a[1] + s[1], a[2] + s[2]];
    b.drones[k].bolt_t -= 1;
    if super::on_foot(b) {
        let me = b.pilot.at();
        // The point of this tick's way nearest his axis, across.
        let (px, pz) = (i64::from(me[0] - a[0]), i64::from(me[2] - a[2]));
        let (sx, sz) = (i64::from(s[0]), i64::from(s[2]));
        let ss = (sx * sx + sz * sz).max(1);
        let u = ((px * sx + pz * sz) * 256 / ss).clamp(0, 256);
        let q = [0, 1, 2].map(|i| a[i] + (i64::from(s[i]) * u / 256) as i32);
        let across = apart([q[0], q[2]], [me[0], me[2]]);
        if across <= BOLT_HIT && q[1] >= me[1] && q[1] <= me[1] + PILOT_H {
            b.drones[k].bolt_t = 0;
            let pl = &mut b.pilot;
            pl.suit -= damage(b.danger) * 60;
            b.jolt = 10;
            b.note = Some("HIT");
            for v in 0..4 {
                sparks::add(&mut b.sparks, q, sparks::SPARK, v * 4);
            }
            walker::suit_check(b);
            return;
        }
    }
    if n[1] <= site::surface(&b.site, n[0], n[2]) {
        b.drones[k].bolt_t = 0;
        for v in 0..3 {
            sparks::add(&mut b.sparks, n, sparks::DUST, v * 5);
        }
        return;
    }
    b.drones[k].bolt = n;
}

/// The cutter on drone `k` this tick (`per_tick` in 1/60 of a point).
/// A drone on its round turns on its attacker.
pub fn cut(b: &mut Below, k: usize, per_tick: i32) {
    let d = &mut b.drones[k];
    if !d.alive() {
        return;
    }
    d.hp -= per_tick;
    d.flash = HIT_FLASH;
    let start = d.mode == PATROL;
    if d.hp <= 0 {
        burst(b, k);
    } else if start {
        aim(b, k);
    }
}

/// Drone `k` bursts: a 4-tick flash, ten sparks, three parts, a chunk
/// of SPARES, DRONE DOWN.
pub fn burst(b: &mut Below, k: usize) {
    let d = b.drones[k];
    if !d.alive() {
        return;
    }
    b.drones[k].mode = BURST;
    b.drones[k].t = 0;
    b.downed |= bit(k);
    b.kills = b.kills.saturating_add(1);
    for v in 0..10u8 {
        sparks::add(&mut b.sparks, d.p, sparks::SPARK, (v * 3) % 16);
    }
    for v in 0..3u8 {
        sparks::add(&mut b.sparks, d.p, sparks::PART, v * 5);
    }
    super::loose::burst(b, [d.p[0], d.p[1] - M, d.p[2]], SPARES, DRONE_TONNES);
    b.note = Some("DRONE DOWN");
}

/// A rolling boulder at `at` (its middle) crushes the drones it meets
/// (counted as the pilot's).
pub fn crush(b: &mut Below, at: V3) {
    for k in 0..N {
        let d = b.drones[k];
        if !d.alive() {
            continue;
        }
        let near = apart([d.p[0], d.p[2]], [at[0], at[2]]) < BOULDER_R + DRONE_R + CRUSH_SLACK;
        if near && d.p[1] - DRONE_R < at[1] + BOULDER_R {
            burst(b, k);
        }
    }
}

/// Drones still up (alive or bursting now), of the site's.
pub fn up(b: &Below) -> u32 {
    b.drones.iter().filter(|d| d.alive()).count() as u32
}
