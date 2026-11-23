//! Step G3: the cutter (SPEC 3.6, "The cutter"): X held fires it; it
//! locks on the nearest seam, the wreck or a boulder's base within 30 m
//! and 25 degrees of the pilot's facing; its heat cuts it out at 100
//! until 40. A seam cracks in three stages and bursts into chunks, the
//! wreck in two, a boulder comes loose. The laser's level (`car.laser`)
//! is the cutter's strength (`STAGE_TIME`). Step G4: a drone is a target
//! too; the beam takes `CUT_DPS` of its points a second by laser level
//! (no stages).

use super::Below;
use super::drones;
use super::loose::{self, ROCK_REST, ROCK_ROLL};
use super::site;
use super::tune::{
    BOULDER_LOOSE, BOULDER_R, CUT_CONE, CUT_DPS, CUT_HEAT_DOWN, CUT_HEAT_MAX, CUT_HEAT_UP,
    CUT_REACH, CUT_RESUME, SEAM_STAGES, SEAM_TONNES, SEAMS, STAGE_TIME, WRECK_STAGES, WRECK_TONNES,
};
use super::walker::apart;
use crate::fixed::{V3, cos, isqrt, sin};
use crate::world::M;

/// What the cutter is locked on: nothing, seam `k`, the wreck, boulder
/// `k`, drone `k` (step G4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lock {
    None,
    Seam(u8),
    Wreck,
    Boulder(u8),
    Drone(u8),
}

/// The cutter (kept in `Below`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cutter {
    /// Heat in 1/60 (100 is `CUT_HEAT_MAX`), cut out while `hot`.
    pub heat: i32,
    pub hot: bool,
    /// The beam is on this tick.
    pub on: bool,
    pub lock: Lock,
    /// Ticks of beam on the locked target's stage.
    pub work: u16,
    /// Where the beam ends (world units), and whether it ends on the
    /// ground (the dust spot).
    pub end: V3,
    pub dust: bool,
}

impl Cutter {
    pub const NONE: Cutter = Cutter {
        heat: 0,
        hot: false,
        on: false,
        lock: Lock::None,
        work: 0,
        end: [0; 3],
        dust: false,
    };
}

/// The bit of `taken` for seam `k` (0..4) and the wreck.
pub const fn seam_bit(k: usize) -> u8 {
    1 << k
}
pub const WRECK_BIT: u8 = 1 << SEAMS;
/// Every seam and the wreck.
pub const ALL_TAKEN: u8 = (1 << (SEAMS + 1)) - 1;

/// The good a look's seams hold: ORE (DESERT, RUST, LAVA), ICE (ICE),
/// GRAIN (OCEAN, JUNGLE, HAZE) (`market::NAMES` order).
pub fn seam_good(look: u8) -> u8 {
    match look {
        1 | 3 | 7 => 2,
        2 => 1,
        _ => 0,
    }
}
/// SPARES (the wreck's).
pub const SPARES: u8 = 4;

/// Where the cutter hits target `l` (world units).
pub fn aim(b: &Below, l: Lock) -> Option<V3> {
    let on_ground = |p: [i32; 2], up: i32| [p[0], site::surface(&b.site, p[0], p[1]) + up, p[1]];
    match l {
        Lock::None => None,
        Lock::Seam(k) => Some(on_ground(b.places.seams[usize::from(k)], M * 4 / 10)),
        Lock::Wreck => Some(on_ground(b.places.wreck, M)),
        Lock::Boulder(k) => {
            let a = b.rocks[usize::from(k)].at();
            Some([a[0], a[1] - BOULDER_R * 2 / 3, a[2]])
        }
        Lock::Drone(k) => {
            let d = &b.drones[usize::from(k)];
            d.alive().then_some(d.p)
        }
    }
}

/// The pilot's hand (world units): 1.1 m up, a little to his right.
pub fn hand(b: &Below) -> V3 {
    let p = b.pilot.at();
    let (s, c) = (sin(b.pilot.yaw), cos(b.pilot.yaw));
    let side = M * 3 / 10;
    [
        p[0] + ((c * side) >> 14),
        p[1] + M * 11 / 10,
        p[2] - ((s * side) >> 14),
    ]
}

/// The targets the cutter could lock on now.
fn targets(b: &Below, out: &mut [Lock; TARGETS]) -> usize {
    let mut n = 0;
    for k in 0..SEAMS {
        if b.seams[k] < SEAM_STAGES {
            out[n] = Lock::Seam(k as u8);
            n += 1;
        }
    }
    if b.wreck < WRECK_STAGES {
        out[n] = Lock::Wreck;
        n += 1;
    }
    for (k, r) in b.rocks.iter().enumerate() {
        if r.state == ROCK_REST {
            out[n] = Lock::Boulder(k as u8);
            n += 1;
        }
    }
    for (k, d) in b.drones.iter().enumerate() {
        if d.alive() {
            out[n] = Lock::Drone(k as u8);
            n += 1;
        }
    }
    n
}

/// The most targets at once: four seams, the wreck, four boulders, four
/// drones.
const TARGETS: usize = 13;

/// The nearest target within reach and the cone of the pilot's facing.
pub fn find(b: &Below) -> Lock {
    let mut list = [Lock::None; TARGETS];
    let n = targets(b, &mut list);
    let h = hand(b);
    let (fx, fz) = (sin(b.pilot.yaw), cos(b.pilot.yaw));
    let mut best = (Lock::None, i64::MAX);
    for &l in &list[..n] {
        let Some(t) = aim(b, l) else { continue };
        let d = [t[0] - h[0], t[1] - h[1], t[2] - h[2]].map(i64::from);
        let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        if d2 > i64::from(CUT_REACH).pow(2) {
            continue;
        }
        let across = isqrt((d[0] * d[0] + d[2] * d[2]) as u64) as i64;
        let along = d[0] * i64::from(fx) + d[2] * i64::from(fz);
        // Within the cone, or right at the pilot's hand.
        let ok = across < i64::from(M) || along >= across * i64::from(CUT_CONE);
        if ok && d2 < best.1 {
            best = (l, d2);
        }
    }
    best.0
}

/// With nothing locked: the beam runs straight ahead from the hand and
/// stops where it meets the ground (dust), at `CUT_REACH` at most.
fn ahead(b: &Below) -> (V3, bool) {
    let h = hand(b);
    let (s, c) = (sin(b.pilot.yaw), cos(b.pilot.yaw));
    // A little down: 1 m in 30.
    let mut d = M;
    while d <= CUT_REACH {
        let p = [
            h[0] + ((s * (d >> 4)) >> 10),
            h[1] - d / 30,
            h[2] + ((c * (d >> 4)) >> 10),
        ];
        let g = site::surface(&b.site, p[0], p[2]);
        if g >= p[1] {
            return ([p[0], g, p[2]], true);
        }
        d += M;
    }
    let d = CUT_REACH;
    (
        [
            h[0] + ((s * (d >> 4)) >> 10),
            h[1] - d / 30,
            h[2] + ((c * (d >> 4)) >> 10),
        ],
        false,
    )
}

/// The ticks of beam a stage of `l` takes at laser level `laser`.
pub fn need(l: Lock, laser: u8) -> u16 {
    match l {
        Lock::Boulder(_) => BOULDER_LOOSE,
        _ => STAGE_TIME[usize::from(laser.min(2))],
    }
}

/// One tick of the cutter (`firing`: X held).
#[inline(never)]
pub fn cut(b: &mut Below, firing: bool) {
    let mut c = b.cutter;
    if firing && !c.hot {
        c.heat = (c.heat + CUT_HEAT_UP).min(CUT_HEAT_MAX);
    } else {
        c.heat = (c.heat - CUT_HEAT_DOWN).max(0);
    }
    if c.heat >= CUT_HEAT_MAX && !c.hot {
        c.hot = true;
        b.note = Some("CUTTER HOT");
    } else if c.hot && c.heat <= CUT_RESUME {
        c.hot = false;
    }
    c.on = firing && !c.hot;
    if !c.on {
        c.lock = Lock::None;
        c.work = 0;
        b.cutter = c;
        return;
    }
    let l = find(b);
    if l != c.lock {
        c.lock = l;
        c.work = 0;
    }
    match aim(b, l) {
        None => {
            let (end, dust) = ahead(b);
            c.end = end;
            c.dust = dust;
            b.cutter = c;
            return;
        }
        Some(t) => {
            c.end = t;
            c.dust = false;
        }
    }
    if let Lock::Drone(k) = l {
        // Step G4: points off a drone every tick, no stages.
        b.cutter = c;
        drones::cut(b, usize::from(k), CUT_DPS[usize::from(b.laser.min(2))]);
        if !b.drones[usize::from(k)].alive() {
            b.cutter.lock = Lock::None;
        }
        return;
    }
    c.work += 1;
    let done = c.work >= need(l, b.laser);
    if done {
        c.work = 0;
    }
    b.cutter = c;
    if done {
        stage(b, l);
    }
}

/// A stage of `l` is cut.
fn stage(b: &mut Below, l: Lock) {
    match l {
        Lock::None => {}
        Lock::Seam(k) => {
            let k = usize::from(k);
            b.seams[k] += 1;
            if b.seams[k] >= SEAM_STAGES {
                let s = b.places.seams[k];
                let at = [s[0], site::surface(&b.site, s[0], s[1]), s[1]];
                let n = SEAM_TONNES[usize::from(b.danger.min(3))];
                loose::burst(b, at, seam_good(b.site.look), n);
                b.taken |= seam_bit(k);
                b.cutter.lock = Lock::None;
                b.note = Some("SEAM CRACKED");
            }
        }
        Lock::Wreck => {
            b.wreck += 1;
            if b.wreck >= WRECK_STAGES {
                let w = b.places.wreck;
                let at = [w[0], site::surface(&b.site, w[0], w[1]) + M, w[1]];
                loose::burst(b, at, SPARES, WRECK_TONNES);
                b.taken |= WRECK_BIT;
                b.cutter.lock = Lock::None;
                b.note = Some("WRECK STRIPPED");
            }
        }
        Lock::Boulder(k) => {
            let r = &mut b.rocks[usize::from(k)];
            r.state = ROCK_ROLL;
            r.t = 0;
            b.cutter.lock = Lock::None;
        }
        Lock::Drone(_) => {}
    }
}

/// How far a target is from the pilot across (world units; the tests'
/// and the scripted pilot's).
pub fn across(b: &Below, at: [i32; 2]) -> i32 {
    let p = b.pilot.at();
    apart([p[0], p[2]], at)
}
