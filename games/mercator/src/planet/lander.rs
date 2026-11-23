//! Step G2: the flown landing (SPEC 3.5): gravity per planet, the lift
//! by ship and load, the sink assist, the jets' heat, the touchdown
//! table, the bounce and the hull's damage.
//!
//! Lengths here are in 1/2^20 m ([`FINE`]) and speeds in 1/2^20 m a
//! tick: a position fits 2,048 m each way, and a tick's step is added
//! as it is (no rounding drift). `p >> TO_WORLD` is the world unit.

use super::Below;
use super::site;
use super::tune::{
    ASSIST_BRAKE, ASSIST_DIV, ASSIST_MAX, ASSIST_MIN, ASSIST_T, BOUNCE, CEILING, CRASH_HULL,
    CRASH_S, CRASH_V, DRAG, FOOT, G_LOW, G_R0, G_RSPAN, G_SPAN, GREASE_S, GREASE_V, HARD_HULL,
    HEAT_BACK, HEAT_COOL, HEAT_MAX, HEAT_PER, HEAT_REF, HPUSH, HSPEED_MAX, LAND_S, LAND_V, LIFT,
    LIFT_MIN, ROAM, TAN12, TIP_HULL, TURN_FROM, TURN_RATE,
};
use crate::fixed::{V3, cos, isqrt, sin};

/// One metre in the lander's units.
pub const FINE: i32 = 1 << 20;
/// `p >> TO_WORLD` is in world units (`M` = 1/256 m).
pub const TO_WORLD: u32 = 12;
/// 1 m/s in fine units a tick.
pub const MPS: i32 = FINE / 60;
/// The heat's unit: 1/60,000 (a thousandth a second, added each tick).
pub const HEAT_UNIT: i32 = 60;

/// The sink assist's modes: off, holding the sink rate, holding height.
pub const ASSIST_OFF: u8 = 0;
pub const ASSIST_SINK: u8 = 1;
pub const ASSIST_HOLD: u8 = 2;

/// What the lander is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The 3 s way down (`entry`).
    Entry,
    /// Flown by the player.
    Fly,
    /// On the ground, the pilot inside.
    Landed,
    /// Step G3: the pilot out on foot (`walker`).
    Foot,
    /// Step G3: the pilot going in at the hatch (`tune::BOARD_TICKS`).
    Board,
    /// The 3 s way up (`entry`).
    TakeOff,
}

/// A tick's input below, from the pad and the finger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct In {
    /// The buttons held (the lander finds the presses itself: at 30
    /// pictures a second two ticks share one call's keys).
    pub held: u32,
    /// Presses the console noted since the last call (a tap shorter than
    /// a picture is in here and never in `held`); the App gives them to
    /// the call's first tick only.
    pub pressed: u32,
    /// The push in the camera's frame, -256..=256: right, ahead.
    pub push: [i32; 2],
    /// Lift asked for, 0..=256 (the right stick is proportional).
    pub lift: i32,
    /// A finger on the screen (game pixels).
    pub finger: Option<(i16, i16)>,
    /// Step G3: the right stick on foot, -256..=256: turn right, tilt
    /// up (the App applies PITCH INVERTED).
    pub look: [i32; 2],
}

/// A touchdown's result (SPEC 3.5's table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Land {
    Greased,
    Landed,
    Hard,
    Tipped,
    Crash,
}

impl Land {
    /// The ship stays down (the others bounce).
    pub fn down(self) -> bool {
        matches!(self, Land::Greased | Land::Landed | Land::Hard)
    }

    pub fn note(self) -> &'static str {
        match self {
            Land::Greased => "GREASED IT: +20 S",
            Land::Landed => "LANDED",
            Land::Hard => "HARD LANDING",
            Land::Tipped => "TIPPED OVER",
            Land::Crash => "CRASH",
        }
    }
}

/// `c` in 1/100 m/s^2 as fine units a tick^2.
pub fn accel(c: i32) -> i32 {
    (i64::from(c) * i64::from(FINE) / 360_000) as i32
}

/// The site's gravity in 1/100 m/s^2 from the planet's radius (world
/// units): 400 at 2,200 m to 900 at 4,000 m.
pub fn gravity(planet_r: i32) -> i32 {
    G_LOW + G_SPAN * (planet_r - G_R0).clamp(0, G_RSPAN) / G_RSPAN
}

/// Full lift in 1/100 of g: the ship's `LIFT`, less 1/100 a tonne
/// carried, never under `LIFT_MIN`.
pub fn lift_k(ship: u8, tonnes: u32) -> u16 {
    let base = LIFT.get(usize::from(ship)).copied().unwrap_or(LIFT[0]);
    base.saturating_sub(tonnes.min(u32::from(u16::MAX)) as u16)
        .max(LIFT_MIN)
}

/// The touchdown table: `v` the speed down and `s` sideways (mm/s),
/// `tan2` the ground's slope under the feet squared (1/16384 squared).
/// The result and the hull it costs.
pub fn touchdown(v: i32, s: i32, tan2: i64) -> (Land, u8) {
    let up = |mm: i64, k: i32| -> u8 { ((mm * i64::from(k) + 999) / 1000).clamp(1, 255) as u8 };
    if v > CRASH_V || s > CRASH_S {
        let total = isqrt((i64::from(v).pow(2) + i64::from(s).pow(2)) as u64) as i64;
        (Land::Crash, up(total, CRASH_HULL))
    } else if tan2 > i64::from(TAN12).pow(2) {
        (Land::Tipped, TIP_HULL)
    } else if v <= GREASE_V && s <= GREASE_S {
        (Land::Greased, 0)
    } else if v <= LAND_V && s <= LAND_S {
        (Land::Landed, 0)
    } else {
        // Over LAND_V down, or over LAND_S sideways (the spec's table
        // names the speed down; sideways is charged the same way).
        let over = (v - LAND_V).max(s - LAND_S);
        (Land::Hard, up(i64::from(over), HARD_HULL))
    }
}

/// The ship's middle (at its feet) in world units.
pub fn at(b: &Below) -> V3 {
    b.p.map(|v| v >> TO_WORLD)
}

/// The ground under the four feet (world units): right-front,
/// left-front, right-back, left-back.
pub fn feet(b: &Below) -> [i32; 4] {
    let c = at(b);
    let (s, co) = (sin(b.yaw), cos(b.yaw));
    let r = [(co * FOOT) >> 14, (-s * FOOT) >> 14];
    let f = [(s * FOOT) >> 14, (co * FOOT) >> 14];
    [(1, 1), (-1, 1), (1, -1), (-1, -1)].map(|(i, j): (i32, i32)| {
        site::surface(
            &b.site,
            c[0] + i * r[0] + j * f[0],
            c[2] + i * r[1] + j * f[1],
        )
    })
}

/// The slope the feet stand on, squared (tan in 1/16384, squared): the
/// rise right and ahead over the feet's 6 m.
pub fn tan2(h: [i32; 4]) -> i64 {
    // Rises under 2^15 world units: products under 2^31.
    let r = (h[0] + h[2] - h[1] - h[3]) * 16 / 3;
    let f = (h[0] + h[1] - h[2] - h[3]) * 16 / 3;
    i64::from(r).pow(2) + i64::from(f).pow(2)
}

/// Horizontal speed (fine a tick).
pub fn hspeed(v: V3) -> i32 {
    isqrt((i64::from(v[0]).pow(2) + i64::from(v[2]).pow(2)) as u64) as i32
}

/// A speed in fine a tick as mm/s.
pub fn mm(v: i32) -> i32 {
    (i64::from(v) * 60_000 / i64::from(FINE)) as i32
}

/// Height over the ground under the middle (fine).
pub fn alt(b: &Below) -> i32 {
    let c = at(b);
    b.p[1] - (site::surface(&b.site, c[0], c[2]) << TO_WORLD)
}

/// One flown tick. `push` (-256..=256, the camera's frame) and `lift`
/// (0..=256) are the pilot's. Returns the touchdown when the feet met
/// the ground.
pub fn fly(b: &mut Below, push: [i32; 2], lift_in: i32) -> Option<Land> {
    let g = b.g;
    // The push, turned from the camera's frame into the site's.
    let (s, c) = (sin(b.cam_yaw), cos(b.cam_yaw));
    let (pr, pf) = (push[0].clamp(-256, 256), push[1].clamp(-256, 256));
    let px = (pr * c + pf * s) >> 14;
    let pz = (pf * c - pr * s) >> 14;
    let a = accel(HPUSH);
    b.v[0] += px * a / 256;
    b.v[2] += pz * a / 256;
    b.push = [pr, pf];
    // Drag: DRAG/100 of the speed a second.
    for k in [0, 2] {
        b.v[k] -= (i64::from(b.v[k]) * i64::from(DRAG) / 6000) as i32;
    }
    let top = HSPEED_MAX * MPS;
    let hs = hspeed(b.v);
    if hs > top {
        b.v[0] = (i64::from(b.v[0]) * i64::from(top) / i64::from(hs)) as i32;
        b.v[2] = (i64::from(b.v[2]) * i64::from(top) / i64::from(hs)) as i32;
    }
    // Not farther than ROAM from the pad: the way out is stopped.
    let out = [b.p[0], b.p[2]];
    let roam = ROAM << TO_WORLD;
    for (k, o) in [(0, out[0]), (2, out[1])] {
        if o.abs() > roam && (o > 0) == (b.v[k] > 0) {
            b.v[k] = 0;
        }
    }
    // The lift: the assist's (0 to full), plus the pilot's, up to full
    // thrust.
    let thrust = g * i32::from(b.k) / 100;
    let h = alt(b);
    let mut lift = 0;
    if b.assist != ASSIST_OFF {
        let want = if b.assist == ASSIST_HOLD {
            0
        } else {
            -sink(h, thrust - g)
        };
        // Never pulling down: a pilot's lift on top always lifts.
        lift = (g + (want - b.v[1]) / ASSIST_T).clamp(0, thrust);
    }
    lift += lift_in.clamp(0, 256) * thrust / 256;
    lift = lift.clamp(0, thrust);
    if b.hot {
        lift = 0;
    }
    if h > CEILING << TO_WORLD {
        lift = lift.min(g);
        b.v[1] = b.v[1].min(0);
    }
    b.lift = lift;
    // Heat: HEAT_PER x lift / (HEAT_REF/100 x g) - HEAT_COOL a second.
    let rate = i64::from(HEAT_PER) * i64::from(lift) * 100 / (i64::from(HEAT_REF) * i64::from(g));
    b.heat = (b.heat + rate as i32 - HEAT_COOL).clamp(0, HEAT_MAX * HEAT_UNIT);
    if b.heat >= HEAT_MAX * HEAT_UNIT && !b.hot {
        b.hot = true;
        b.note = Some("JETS HOT");
    } else if b.heat <= HEAT_BACK * HEAT_UNIT {
        b.hot = false;
    }
    b.v[1] += lift - g;
    for k in 0..3 {
        b.p[k] += b.v[k];
    }
    // The nose turns toward where the ship flies.
    if hs > TURN_FROM * MPS {
        b.yaw = b
            .yaw
            .wrapping_add(turn_toward(b.yaw, b.v[0], b.v[2]) as u16);
    }
    contact(b)
}

/// The assist's speed down (fine a tick) at height `h` (fine), with
/// `spare` (fine a tick^2) of lift over the weight: alt / 5 within 2..15
/// m/s, and never more than the jets can stop in the height left.
pub fn sink(h: i32, spare: i32) -> i32 {
    let line = (h / (ASSIST_DIV * 60)).clamp(ASSIST_MIN * MPS, ASSIST_MAX * MPS);
    let brake = i64::from(spare.max(0)) * i64::from(ASSIST_BRAKE) / 100;
    let stop = isqrt((2 * brake * i64::from(h.max(0))) as u64) as i32;
    line.min(stop.max(ASSIST_MIN * MPS))
}

/// The turn (65536 a turn, at most `TURN_RATE`) from `yaw` toward the
/// direction (x, z) (not both 0).
pub fn turn_toward(yaw: u16, x: i32, z: i32) -> i32 {
    turn_at(yaw, x, z, TURN_RATE)
}

/// The turn (65536 a turn, at most `rate`) from `yaw` toward the
/// direction (x, z) (not both 0). Step G3: the walk turns faster.
pub fn turn_at(yaw: u16, x: i32, z: i32, rate: i32) -> i32 {
    let (fx, fz) = (sin(yaw), cos(yaw));
    let len = isqrt((i64::from(x).pow(2) + i64::from(z).pow(2)) as u64).max(1) as i64;
    // Both in 1/16384 of the direction's length.
    let cross = (i64::from(fz) * i64::from(x) - i64::from(fx) * i64::from(z)) / len;
    let ahead = i64::from(fx) * i64::from(x) + i64::from(fz) * i64::from(z) >= 0;
    if !ahead {
        return if cross < 0 { -rate } else { rate };
    }
    // The sine of a small angle in 1/16384 is the angle in radians:
    // x 65536 / (2 pi 16384) is about x 7 / 11 in turn units.
    ((cross * 7 / 11) as i32).clamp(-rate, rate)
}

/// The feet and the ground: a touchdown when the highest ground under a
/// foot is at the ship's feet or over them.
fn contact(b: &mut Below) -> Option<Land> {
    let h = feet(b);
    let top = h.iter().copied().max().unwrap_or(0) << TO_WORLD;
    if b.p[1] > top {
        return None;
    }
    let v = mm((-b.v[1]).max(0));
    let s = mm(hspeed(b.v));
    let (land, cost) = touchdown(v, s, tan2(h));
    b.p[1] = top;
    b.hull = b.hull.saturating_sub(cost);
    if land.down() {
        b.v = [0; 3];
    } else {
        b.v = [b.v[0] / 2, BOUNCE * MPS, b.v[2] / 2];
        b.jolt = 12;
    }
    Some(land)
}
