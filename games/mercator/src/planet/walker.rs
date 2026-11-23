//! Step G3: the pilot on foot (SPEC 3.6, "The pilot"): the walk in the
//! camera's frame, slopes, the step up, the jump, the hop jets, falls,
//! the suit, the hatch and the walk's limit.
//!
//! Positions are the pilot's feet in fine units (`lander::FINE`, 1/2^20
//! m) and speeds fine a tick, as the lander's. The site's things he
//! walks round are upright cylinders ([`solids`]).

use super::Below;
use super::lander::{FINE, MPS, TO_WORLD};
use super::loose::{ROCK_REST, ROCK_STOPPED};
use super::site;
use super::tune::{
    AIR_ACC, AIR_TOP, BOULDER_R, FALL_COST, FALL_SAFE, HATCH_OUT, HATCH_R, HOP_ACC, HOP_AFTER,
    HOP_BURN, HOP_CLIMB_TENTHS, HOP_ENERGY, HOP_REFILL, HOP_REFILL_AFTER, JUMP_TENTHS, PILOT_R,
    SEAM_SOLID, SEAM_STAGES, SHIP_SOLID, SLIDE, STEP_UP, SUIT_LOW, SUIT_MAX, SUIT_REFILL, TAN35,
    WADE_TENTHS, WALK, WALK_ACC, WALK_BRAKE, WALK_EDGE, WALK_R, WALK_TURN, WRECK_SOLID,
};
use crate::fixed::{V3, cos, isqrt, sin};
use crate::world::M;

/// Things said once a visit (`Pilot::said`).
pub const SAID_BOARD: u8 = 1;
pub const SAID_RANGE: u8 = 2;
pub const SAID_LOW: u8 = 4;
pub const SAID_WARN: u8 = 8;
/// Step G4: the storm's start was said.
pub const SAID_STORM: u8 = 32;
/// The pilot has been away from the hatch (so the board note can come).
pub const AWAY: u8 = 16;

/// The pilot (kept in `Below`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pilot {
    /// His feet (fine) and his speed (fine a tick).
    pub p: V3,
    pub v: V3,
    /// Where he faces (65536 a turn).
    pub yaw: u16,
    pub ground: bool,
    /// Jetting this tick (for the picture).
    pub jets: bool,
    /// Ticks in the air, ticks on the ground since the last landing.
    pub air: u16,
    pub down: u16,
    /// Jet energy and the suit, both in 1/60 (a second's rate is a
    /// tick's step); the suit's most, in points.
    pub energy: i32,
    pub suit: i32,
    pub suit_max: u8,
    /// The walk's phase (1/256 m walked; the legs swing by it).
    pub stride: u32,
    /// The camera's lift from the right stick (world units).
    pub tilt: i32,
    /// `SAID_*` and `AWAY` bits.
    pub said: u8,
}

impl Pilot {
    pub const NONE: Pilot = Pilot {
        p: [0; 3],
        v: [0; 3],
        yaw: 0,
        ground: true,
        jets: false,
        air: 0,
        down: 0,
        energy: HOP_ENERGY,
        suit: 0,
        suit_max: 0,
        stride: 0,
        tilt: 0,
        said: 0,
    };

    /// His feet in world units.
    pub fn at(&self) -> V3 {
        self.p.map(|v| v >> TO_WORLD)
    }
}

/// m/s^2 in fine a tick^2.
pub const fn acc(a: i32) -> i32 {
    a * FINE / 3600
}

/// The ground (fine) under a place given in fine units.
pub fn ground(b: &Below, x: i32, z: i32) -> i32 {
    site::surface(&b.site, x >> TO_WORLD, z >> TO_WORLD) << TO_WORLD
}

/// The hatch (world units, x and z): `HATCH_OUT` to the ship's right.
pub fn hatch(b: &Below) -> [i32; 2] {
    let at = super::lander::at(b);
    let (s, c) = (sin(b.yaw), cos(b.yaw));
    [
        at[0] + ((c * (HATCH_OUT >> 2)) >> 12),
        at[2] - ((s * (HATCH_OUT >> 2)) >> 12),
    ]
}

/// Horizontal distance between two places (world units).
pub fn apart(a: [i32; 2], b: [i32; 2]) -> i32 {
    let (dx, dz) = (i64::from(a[0] - b[0]), i64::from(a[1] - b[1]));
    isqrt((dx * dx + dz * dz) as u64) as i32
}

/// The pilot is at the hatch.
pub fn at_hatch(b: &Below) -> bool {
    let p = b.pilot.at();
    apart([p[0], p[2]], hatch(b)) <= HATCH_R
}

/// Puts the pilot out at the hatch, facing away from the ship, the
/// camera behind him; the suit by shield level (`shield` 0..=2).
pub fn step_out(b: &mut Below, shield: u8) {
    let h = hatch(b);
    let y = site::surface(&b.site, h[0], h[1]);
    let max = SUIT_MAX[usize::from(shield.min(2))];
    let keep = b.pilot.said & !AWAY;
    let suit = if b.pilot.suit_max == 0 {
        i32::from(max) * 60
    } else {
        b.pilot.suit
    };
    b.pilot = Pilot {
        p: [h[0] << TO_WORLD, y << TO_WORLD, h[1] << TO_WORLD],
        yaw: b.yaw.wrapping_add(0x4000),
        suit,
        suit_max: max,
        said: keep,
        ..Pilot::NONE
    };
    b.cam_goal = b.pilot.yaw;
}

/// Up to 10 solid things (x, z, radius; world units): the ship, the
/// wreck, the seams not yet burst, the boulders at rest.
pub fn solids(b: &Below, out: &mut [[i32; 3]]) -> usize {
    let mut n = 0;
    let mut put = |x: i32, z: i32, r: i32| {
        if n < out.len() {
            out[n] = [x, z, r];
            n += 1;
        }
    };
    let ship = super::lander::at(b);
    put(ship[0], ship[2], SHIP_SOLID);
    let w = b.places.wreck;
    put(w[0], w[1], WRECK_SOLID);
    for (k, s) in b.places.seams.iter().enumerate() {
        if b.seams[k] < SEAM_STAGES {
            put(s[0], s[1], SEAM_SOLID);
        }
    }
    for r in &b.rocks {
        if r.state == ROCK_REST || r.state == ROCK_STOPPED {
            let a = r.at();
            put(a[0], a[2], BOULDER_R);
        }
    }
    n
}

/// Moves a place (fine x, z) out of the solid cylinders, `r` (world)
/// being its own radius; the speed into a cylinder is taken off.
/// Returns whether it touched one. Step G4: the rock props of the 3 x 3
/// cells round the place are solid too.
pub fn push_out(b: &Below, p: &mut V3, v: &mut [i32; 2], r: i32) -> bool {
    let mut list = [[0i32; 3]; 19];
    let mut n = solids(b, &mut list[..10]);
    n += super::props::rocks_near(
        b,
        p[0] >> TO_WORLD,
        p[2] >> TO_WORLD,
        &mut list[n..],
        super::tune::ROCK_PROP_R,
    );
    let mut hit = false;
    for s in &list[..n] {
        let (dx, dz) = ((p[0] >> TO_WORLD) - s[0], (p[2] >> TO_WORLD) - s[1]);
        let need = s[2] + r;
        let d = apart([dx, dz], [0, 0]);
        if d >= need {
            continue;
        }
        hit = true;
        let (ux, uz, d) = if d == 0 { (1, 0, 1) } else { (dx, dz, d) };
        p[0] = (s[0] + ux * need / d) << TO_WORLD;
        p[2] = (s[1] + uz * need / d) << TO_WORLD;
        // The part of the speed toward the middle goes.
        let inward = i64::from(v[0]) * i64::from(ux) + i64::from(v[1]) * i64::from(uz);
        if inward < 0 {
            let dd = i64::from(d) * i64::from(d);
            v[0] -= (inward * i64::from(ux) / dd) as i32;
            v[1] -= (inward * i64::from(uz) / dd) as i32;
        }
    }
    hit
}

/// The length of (x, z).
fn len2(x: i32, z: i32) -> i32 {
    isqrt((i64::from(x).pow(2) + i64::from(z).pow(2)) as u64) as i32
}

/// Moves (x, z) toward (tx, tz) by at most `step`.
fn toward(v: [i32; 2], t: [i32; 2], step: i32) -> [i32; 2] {
    let (dx, dz) = (t[0] - v[0], t[1] - v[1]);
    let d = len2(dx, dz);
    if d <= step {
        t
    } else {
        [
            v[0] + (i64::from(dx) * i64::from(step) / i64::from(d)) as i32,
            v[1] + (i64::from(dz) * i64::from(step) / i64::from(d)) as i32,
        ]
    }
}

/// Caps (x, z) at length `top`.
fn cap(v: [i32; 2], top: i32) -> [i32; 2] {
    let d = len2(v[0], v[1]);
    if d <= top {
        v
    } else {
        [
            (i64::from(v[0]) * i64::from(top) / i64::from(d)) as i32,
            (i64::from(v[1]) * i64::from(top) / i64::from(d)) as i32,
        ]
    }
}

/// One tick of the walk. `push` is the stick (-256..=256, the camera's
/// frame: right, ahead), `jump` a press of A, `jet` A held.
#[inline(never)]
pub fn walk(b: &mut Below, push: [i32; 2], jump: bool, jet: bool) {
    let mut pl = b.pilot;
    let g = b.g;
    // The stick in the site's frame (at most 256 long).
    let (s, c) = (sin(b.cam_yaw), cos(b.cam_yaw));
    let (pr, pf) = (push[0].clamp(-256, 256), push[1].clamp(-256, 256));
    let want = cap([(pr * c + pf * s) >> 14, (pf * c - pr * s) >> 14], 256);
    let wx = pl.p[0] >> TO_WORLD;
    let wz = pl.p[2] >> TO_WORLD;
    let from_pad = len2(wx, wz);
    let wading = site::height(&b.site, wx, wz) < b.site.wet;
    let mut top = if wading {
        WADE_TENTHS * MPS / 10
    } else {
        WALK * MPS
    };
    if from_pad > WALK_R {
        top /= 4;
        if pl.said & SAID_RANGE == 0 {
            pl.said |= SAID_RANGE;
            b.note = Some("SUIT RANGE: TURN BACK");
        }
    } else if from_pad < WALK_R - 10 * M {
        pl.said &= !SAID_RANGE;
    }
    let mut hv = [pl.v[0], pl.v[2]];
    pl.jets = false;
    if pl.ground {
        let (gx, gz) = site::slope(&b.site, wx, wz);
        let steep = i64::from(gx).pow(2) + i64::from(gz).pow(2) > i64::from(TAN35).pow(2);
        if steep {
            // Sliding down: the stick has no hold here.
            let d = i64::from(len2(gx, gz).max(1));
            let sl = i64::from(SLIDE * MPS);
            let goal = [
                (-i64::from(gx) * sl / d) as i32,
                (-i64::from(gz) * sl / d) as i32,
            ];
            hv = toward(hv, goal, acc(WALK_ACC));
        } else {
            let goal = [want[0] * top / 256, want[1] * top / 256];
            let rate = if want == [0, 0] {
                acc(WALK_BRAKE)
            } else {
                acc(WALK_ACC)
            };
            hv = toward(hv, goal, rate);
        }
        if jump {
            pl.v[1] = JUMP_TENTHS * MPS / 10;
            pl.ground = false;
            pl.air = 0;
        }
    } else {
        pl.v[1] -= g;
        let add = [want[0] * acc(AIR_ACC) / 256, want[1] * acc(AIR_ACC) / 256];
        let was = len2(hv[0], hv[1]);
        hv = [hv[0] + add[0], hv[1] + add[1]];
        // The stick may not push past AIR_TOP (a knock may be faster).
        hv = cap(hv, (AIR_TOP * MPS).max(was));
        pl.air = pl.air.saturating_add(1);
        let climb = HOP_CLIMB_TENTHS * MPS / 10;
        if jet && pl.air >= HOP_AFTER && pl.energy > 0 && pl.v[1] < climb {
            pl.v[1] = (pl.v[1] + g + acc(HOP_ACC)).min(climb);
            pl.energy = (pl.energy - HOP_BURN).max(0);
            pl.jets = true;
        }
    }
    // He turns to face the stick (or his slide).
    let face = if want != [0, 0] { want } else { [0, 0] };
    if face != [0, 0] {
        let turn = super::lander::turn_at(pl.yaw, face[0], face[1], WALK_TURN);
        pl.yaw = pl.yaw.wrapping_add(turn as u16);
    }
    // The move across, with the walk's edge, the step up and the walls.
    let mut n = [pl.p[0] + hv[0], pl.p[1], pl.p[2] + hv[1]];
    let edge = len2(n[0] >> TO_WORLD, n[2] >> TO_WORLD);
    if edge > WALK_EDGE {
        let (ex, ez) = (n[0] >> TO_WORLD, n[2] >> TO_WORLD);
        let out =
            (i64::from(hv[0]) * i64::from(ex) + i64::from(hv[1]) * i64::from(ez)) / i64::from(edge);
        if out > 0 {
            hv[0] -= (out * i64::from(ex) / i64::from(edge)) as i32;
            hv[1] -= (out * i64::from(ez) / i64::from(edge)) as i32;
        }
        n[0] = ((i64::from(ex) * i64::from(WALK_EDGE) / i64::from(edge)) as i32) << TO_WORLD;
        n[2] = ((i64::from(ez) * i64::from(WALK_EDGE) / i64::from(edge)) as i32) << TO_WORLD;
    }
    let step = STEP_UP << TO_WORLD;
    let ng = ground(b, n[0], n[2]);
    let blocked = if ng > pl.p[1] + step {
        true
    } else if pl.ground && ng > pl.p[1] {
        // A steep rise: blocked when it goes on past a step's height
        // within the next 0.75 m.
        let (gx, gz) = site::slope(&b.site, n[0] >> TO_WORLD, n[2] >> TO_WORLD);
        let steep = i64::from(gx).pow(2) + i64::from(gz).pow(2) > i64::from(TAN35).pow(2);
        let uphill = i64::from(gx) * i64::from(hv[0]) + i64::from(gz) * i64::from(hv[1]) > 0;
        if steep && uphill {
            let d = len2(hv[0], hv[1]).max(1);
            let ahead = (3 * FINE / 4) / d;
            let far = ground(b, n[0] + hv[0] * ahead, n[2] + hv[1] * ahead);
            far > pl.p[1] + step
        } else {
            false
        }
    } else {
        false
    };
    if blocked {
        n[0] = pl.p[0];
        n[2] = pl.p[2];
        hv = [0, 0];
    }
    push_out(b, &mut n, &mut hv, PILOT_R);
    let ng = ground(b, n[0], n[2]);
    pl.p[0] = n[0];
    pl.p[2] = n[2];
    if pl.ground {
        if ng < pl.p[1] - step {
            // Off a ledge.
            pl.ground = false;
            pl.air = 0;
            pl.v[1] = 0;
        } else {
            pl.p[1] = ng;
            pl.v[1] = 0;
        }
        pl.stride = pl
            .stride
            .wrapping_add(len2(hv[0], hv[1]) as u32 >> TO_WORLD);
    }
    if !pl.ground {
        pl.p[1] += pl.v[1];
        if pl.p[1] <= ng {
            let impact = -pl.v[1];
            pl.p[1] = ng;
            pl.v[1] = 0;
            pl.ground = true;
            pl.down = 0;
            let safe = FALL_SAFE * MPS;
            if impact > safe {
                // (v - 12) x 4 points, in 1/60 of a point.
                let cost = i64::from(impact - safe) * i64::from(FALL_COST) * 60 / i64::from(MPS);
                pl.suit -= cost as i32;
                b.jolt = 8;
            }
        }
    }
    if pl.ground {
        pl.down = pl.down.saturating_add(1);
        if pl.down >= HOP_REFILL_AFTER {
            pl.energy = (pl.energy + HOP_REFILL).min(HOP_ENERGY);
        }
    }
    pl.v[0] = hv[0];
    pl.v[2] = hv[1];
    b.pilot = pl;
    // The hatch: the suit refills, the jets fill.
    let p = b.pilot.at();
    let near = apart([p[0], p[2]], hatch(b)) <= HATCH_R;
    let pl = &mut b.pilot;
    if near {
        pl.suit = (pl.suit + SUIT_REFILL).min(i32::from(pl.suit_max) * 60);
        pl.energy = HOP_ENERGY;
        if pl.said & AWAY != 0 && pl.said & SAID_BOARD == 0 {
            pl.said |= SAID_BOARD;
            b.note = Some("HOLD B TO LIFT OFF");
        }
    } else {
        pl.said |= AWAY;
    }
    suit_check(b);
}

/// The suit's low note (once until it is refilled over the mark).
pub fn suit_check(b: &mut Below) {
    let pl = &mut b.pilot;
    let low = pl.suit * 100 < i32::from(pl.suit_max) * 60 * SUIT_LOW;
    if low && pl.suit > 0 && pl.said & SAID_LOW == 0 {
        pl.said |= SAID_LOW;
        b.note = Some("SUIT LOW");
    } else if !low {
        pl.said &= !SAID_LOW;
    }
}

/// The ticks' worth of rock under the pilot's feet would never be under
/// the ground: the tests use this.
pub fn under(b: &Below) -> i32 {
    ground(b, b.pilot.p[0], b.pilot.p[2]) - b.pilot.p[1]
}

/// Points (0..) of suit left.
pub fn suit_points(b: &Below) -> i32 {
    b.pilot.suit.max(0) / 60
}

/// Boulder radius as fine units (the boulders' own module uses it).
pub const BOULDER_FINE: i32 = BOULDER_R << TO_WORLD;
