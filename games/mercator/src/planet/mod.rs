//! MERCATOR's planet mode (SPEC "GROUNDSIDE", section 3). Step G1 built
//! its frame and its land; step G2 the way down ([`entry`]), the flown
//! landing ([`lander`]), the chase camera ([`camera`]), the ship's
//! shadow and the landing panel ([`panel`]).
//!
//! The App calls [`begin`] when the ship crosses the ring, then
//! [`tick`], [`draw`] and [`panel`] while below, and [`lift_off`] when
//! the take-off ends (or the loss path, `Outcome::Lost`). Space is
//! frozen while below: nothing here writes `World` except [`lift_off`]
//! (SPEC 3.3, 3.7); the hull's damage is kept here until then.
//!
//! The planet's code runs from the flash part (flash.list `cold` lines:
//! the walk from the per-picture roots stops at these calls). While
//! below, [`tick`], [`draw`] and [`panel`] run on every picture by
//! design (MODULES.md, "rule of thumb" and section 4). The space side
//! ([`space`]) stays in RAM.

pub mod camera;
pub mod clock;
pub mod cutter;
pub mod drones;
pub mod entry;
pub mod land;
pub mod lander;
pub mod loose;
pub mod models;
pub mod palette;
pub mod panel;
pub mod place;
pub mod props;
pub mod site;
pub mod sky;
pub mod space;
pub mod sparks;
pub mod tune;
pub mod walker;

use crate::art;
use crate::fixed::{Mat3, ONE14, V3, cos, cross14, dot, norm, scale14, sin, sub};
use crate::market::Hold;
use crate::models as ships;
use crate::poly::{Camera, NEAR, Object, Rect, Renderer, SUB, Shading, View, fill_tri, line};
use crate::scene::Detail;
use crate::world::{M, MAX_SPEED, World};
use game_kit::Frame;
use game_kit::button;
use lander::{ASSIST_HOLD, ASSIST_OFF, ASSIST_SINK, In, Land, Phase};
use place::Places;
use site::Site;
use tune::{
    ENTRY_SKIP, ENTRY_TICKS, LAND_SCALE, LEAN, LEGS_FROM, LIFT_FUEL, LIFT_HOLD, SHADOW_DARK,
    SHADOW_FROM, SHADOW_R, TAKEOFF_TICKS, TAP,
};

/// The view below: 160 x 96 over the 24 px panel, as in flight (SPEC
/// 3.9).
pub const VIEW: View = View::new(Rect::new(0, 0, 160, 96), 96);

/// The planet mode's state, kept in the App (at most 2,048 B,
/// `tests/planet.rs` checks it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Below {
    /// The ship is below (the App's scene is `Below`, or the title or the
    /// pause card over it, or the loss card).
    pub on: bool,
    pub site: Site,
    pub places: Places,
    /// The site's axes in space: east, up, north (16384 long).
    pub axes: [V3; 3],
    /// The ring's middle and the site's direction (for the lift-off).
    pub ring: V3,
    pub dir: V3,
    /// Toward the sun and the station, in the site's axes.
    pub sun: V3,
    pub station: V3,
    /// Ticks since the entry, and how long B (or a finger on JETS) has
    /// been held on the ground.
    pub t: u32,
    pub hold: u16,
    pub ship: u8,
    pub star: u8,
    // ---- Step G2: the lander ----
    pub phase: Phase,
    /// Ticks in this phase.
    pub phase_t: u16,
    /// The ship's feet (1/2^20 m) and its speed (1/2^20 m a tick).
    pub p: V3,
    pub v: V3,
    /// Gravity (1/2^20 m a tick^2), full lift (1/100 of g), the lift of
    /// the last tick (as gravity).
    pub g: i32,
    pub k: u16,
    pub lift: i32,
    /// The jets' heat (1/60,000), cut out while `hot`.
    pub heat: i32,
    pub hot: bool,
    /// `lander::ASSIST_OFF`, `_SINK` or `_HOLD`.
    pub assist: u8,
    /// The ship's nose (65536 a turn) and its lean (the push).
    pub yaw: u16,
    pub push: [i32; 2],
    /// The ship's middle over its feet (world units).
    pub ride: i32,
    /// The camera: where it is, where it looks, where it is turning to.
    pub cam: V3,
    pub cam_yaw: u16,
    pub cam_goal: u16,
    /// The buttons held last tick; how long L, R and a finger on JETS
    /// have been held.
    pub prev: u32,
    pub l_t: u8,
    pub r_t: u8,
    pub finger_t: u8,
    /// The hull (written back to the career at the lift-off), the fuel
    /// (read only: the lift-off's check).
    pub hull: u8,
    pub hull_max: u8,
    pub fuel: u8,
    /// Seconds a greased landing added to the site clock (G3's clock).
    pub bonus: u16,
    /// Ticks the view still shakes after a knock.
    pub jolt: u8,
    /// Where the take-off started (1/2^20 m).
    pub base: i32,
    /// The last touchdown.
    pub touched: Option<Land>,
    /// A note for the App to show (taken once).
    pub note: Option<&'static str>,
    // ---- Step G3: on foot ----
    pub pilot: walker::Pilot,
    pub cutter: cutter::Cutter,
    /// Each seam's stage (3: burst) and the wreck's (2: stripped).
    pub seams: [u8; tune::SEAMS],
    pub wreck: u8,
    pub chunks: [loose::Chunk; tune::CHUNKS],
    pub rocks: [loose::Rock; tune::BOULDERS],
    /// The hold below (written back at the lift-off) and its size.
    pub cargo: Hold,
    pub room: u16,
    /// Tonnes picked up on this visit.
    pub haul: u8,
    /// The system's danger, the laser's and the shield's level.
    pub danger: u8,
    pub laser: u8,
    pub shield: u8,
    /// Ticks since the touchdown (the site clock).
    pub since: u16,
    /// What is cut at this site today (`cutter::seam_bit`,
    /// `cutter::WRECK_BIT`) and the system and day that holds for. Kept
    /// from visit to visit (`begin` keeps it; the App clears it for a
    /// new career).
    pub taken: u8,
    pub taken_day: Option<(u8, u16)>,
    // ---- Step G4: drones, sparks, props, the scan ----
    pub drones: [drones::Drone; drones::N],
    pub sparks: sparks::Pool,
    pub flats: [props::Flat; tune::FLATS],
    /// Ticks since the scan (`u16::MAX`: none), where it was made, what
    /// it marked (seams bits 0-3, the wreck 4, drones 5-8, boulders
    /// 9-12; `MARK_*`).
    pub scan_t: u16,
    pub scan_at: V3,
    pub marks: u16,
    /// Drones met (there at the touchdown) and downed on this visit;
    /// drones downed at this site today (`drones::bit`, kept from visit
    /// to visit with `taken`).
    pub met: u8,
    pub kills: u8,
    pub downed: u8,
}

/// The scan's mark bits (`Below::marks`).
pub const MARK_WRECK: u16 = 1 << 4;
pub const fn mark_drone(k: usize) -> u16 {
    1 << (5 + k)
}
pub const fn mark_rock(k: usize) -> u16 {
    1 << (9 + k)
}

impl Below {
    pub const EMPTY: Below = Below {
        on: false,
        site: Site::NONE,
        places: Places::NONE,
        axes: [[0; 3]; 3],
        ring: [0; 3],
        dir: [0; 3],
        sun: [0; 3],
        station: [0; 3],
        t: 0,
        hold: 0,
        ship: 0,
        star: 0,
        phase: Phase::Entry,
        phase_t: 0,
        p: [0; 3],
        v: [0; 3],
        g: 0,
        k: 0,
        lift: 0,
        heat: 0,
        hot: false,
        assist: ASSIST_SINK,
        yaw: 0,
        push: [0; 2],
        ride: 0,
        cam: [0; 3],
        cam_yaw: 0,
        cam_goal: 0,
        prev: 0,
        l_t: 0,
        r_t: 0,
        finger_t: 0,
        hull: 0,
        hull_max: 0,
        fuel: 0,
        bonus: 0,
        jolt: 0,
        base: 0,
        touched: None,
        note: None,
        pilot: walker::Pilot::NONE,
        cutter: cutter::Cutter::NONE,
        seams: [0; tune::SEAMS],
        wreck: 0,
        chunks: [loose::Chunk::NONE; tune::CHUNKS],
        rocks: [loose::Rock::NONE; tune::BOULDERS],
        cargo: Hold([0; crate::market::GOODS]),
        room: 0,
        haul: 0,
        danger: 0,
        laser: 0,
        shield: 0,
        since: 0,
        taken: 0,
        taken_day: None,
        drones: [drones::Drone::NONE; drones::N],
        sparks: sparks::Pool::EMPTY,
        flats: [props::Flat::NONE; tune::FLATS],
        scan_t: u16::MAX,
        scan_at: [0; 3],
        marks: 0,
        met: 0,
        kills: 0,
        downed: 0,
    };

    /// A vector of space in the site's axes.
    pub fn local(&self, v: V3) -> V3 {
        self.axes.map(|a| (dot(v, a) >> 14) as i32)
    }
}

/// What a tick below ended in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    /// The take-off is over: back to space (the App calls [`lift_off`]).
    Lifted,
    /// The hull is gone: the ship is lost, as in space (SPEC 3.7).
    Lost,
}

/// Builds the site under the ring (the ship just crossed it): the land,
/// its things, the sky's sun and station, then the lander ([`ready`]).
/// Reads `World`, writes nothing there. None at a world with no surface
/// (never: the ring is not there).
#[inline(never)]
pub fn begin(b: &mut Below, w: &World) -> bool {
    let (Some(sp), Some(mut s)) = (space::site(w), site::make(w.sys.h, w.sys.look)) else {
        return false;
    };
    let places = place::place(&mut s, w.sys.danger);
    // The site's axes: up is the site's direction; north is the first of
    // `space::across`'s axes turned into the ground; east = up x north.
    let up = sp.dir;
    let (north, _) = space::across(up);
    let east = cross14(up, north);
    let c = &w.car;
    // Step G3: what was cut here today stays cut (a jump is a day).
    let today = Some((c.system, c.day));
    let taken = if b.taken_day == today { b.taken } else { 0 };
    // Step G4: so do the drones downed here today.
    let downed = if b.taken_day == today { b.downed } else { 0 };
    // Built in place (the whole value on the stack would cost a frame
    // of its size).
    *b = Below::EMPTY;
    let nb = &mut *b;
    nb.on = true;
    nb.site = s;
    nb.places = places;
    nb.axes = [east, up, north];
    nb.ring = sp.ring;
    nb.dir = sp.dir;
    nb.star = w.sys.star;
    nb.cargo = c.hold;
    nb.room = c.capacity().min(u32::from(u16::MAX)) as u16;
    nb.danger = w.sys.danger;
    nb.laser = c.laser;
    nb.shield = c.shield;
    nb.taken = taken;
    nb.taken_day = today;
    for k in 0..tune::SEAMS {
        if taken & cutter::seam_bit(k) != 0 {
            nb.seams[k] = tune::SEAM_STAGES;
        }
    }
    if taken & cutter::WRECK_BIT != 0 {
        nb.wreck = tune::WRECK_STAGES;
    }
    populate(nb, downed);
    let spot = scale14(up, w.bodies.planet_r);
    nb.sun = norm(nb.local(scale14(w.bodies.light, -ONE14)), ONE14);
    nb.station = norm(nb.local(sub(w.bodies.station_pos, spot)), ONE14);
    ready(
        nb,
        c.ship,
        c.hold.total(),
        lander::gravity(w.bodies.planet_r),
        (c.hull, c.hull_max().clamp(1, 255) as u8),
        c.fuel,
    );
    true
}

/// The yaw (65536 a turn) whose nose points along (x, z): the nose of
/// yaw `a` points along (sin a, cos a). Step G4: an octant arctangent
/// (a cubic in the octant's ratio, within 0.1 degree) in place
/// of G3's search over the sine table (450 bytes of flash code).
pub fn yaw_to(x: i32, z: i32) -> u16 {
    let (ax, az) = (x.unsigned_abs(), z.unsigned_abs());
    let hi = ax.max(az);
    if hi == 0 {
        return 0;
    }
    // Both to under 2^15 so the sums stay in 32 bits; u = lo / hi in
    // 1/32768, atan(u) in 1/65536 of a turn over 0..1.
    let s = (32 - hi.leading_zeros()).saturating_sub(15);
    let u = ((ax.min(az) >> s) << 15) / (hi >> s).max(1);
    let bend = ((u * (32768 - u)) >> 15) * (2552 + ((691 * u) >> 15));
    let a0 = (u >> 2) + (bend >> 15);
    let mut a = if az >= ax { a0 } else { 16384 - a0 };
    if z < 0 {
        a = 32768 - a;
    }
    if x < 0 {
        a = 65536 - a;
    }
    a as u16
}

/// The lander at the start of the entry: ship `ship` carrying `tonnes`,
/// gravity `gc` (1/100 m/s^2), the hull (now, most) and the fuel.
#[inline(never)]
pub fn ready(b: &mut Below, ship: u8, tonnes: u32, gc: i32, hull: (u8, u8), fuel: u8) {
    b.ship = ship;
    b.g = lander::accel(gc);
    b.k = lander::lift_k(ship, tonnes);
    b.hull = hull.0;
    b.hull_max = hull.1;
    b.fuel = fuel;
    b.phase = Phase::Entry;
    b.phase_t = 0;
    b.p = entry::entry_at(b.places.start, 0);
    b.v = [0; 3];
    b.heat = 0;
    b.hot = false;
    b.assist = ASSIST_SINK;
    // MEASURING ONLY (`--cfg merc_site_test`): the assist starts on
    // HOLD, so a pad script (no touch) can keep the 120 m view still;
    // Y twice (OFF, then SINK) sets the ship down. Not in any build for
    // use.
    #[cfg(merc_site_test)]
    {
        b.assist = ASSIST_HOLD;
    }
    let st = b.places.start;
    b.yaw = yaw_to(-st[0], -st[2]);
    b.cam_yaw = b.yaw;
    b.cam_goal = b.yaw;
    // The model's lowest point: the ship rides that far over its feet,
    // plus 1 m of legs.
    let lowest = ships::craft_model(ship)
        .verts
        .iter()
        .map(|v| i32::from(v[1]))
        .min()
        .unwrap_or(0);
    b.ride = -lowest * LAND_SCALE / 256 + M;
    camera::follow(b, true);
}

/// The JETS box on the panel (touch, SPEC 3.8).
fn in_jets(x: i16, y: i16) -> bool {
    x >= 124 && y >= 96
}

/// One tick below.
#[inline(never)]
pub fn tick(b: &mut Below, i: &In) -> Outcome {
    b.t = b.t.wrapping_add(1);
    b.phase_t = b.phase_t.saturating_add(1);
    b.jolt = b.jolt.saturating_sub(1);
    // A press: held now and not before, or noted by the console (a tap
    // between two pictures); counted once (`prev` keeps it).
    let pressed = (i.held | i.pressed) & !b.prev;
    b.prev = i.held | i.pressed;
    // L and R: a tap turns the camera 45 degrees; both: behind the nose.
    if i.held & button::L != 0 && i.held & button::R != 0 && pressed & (button::L | button::R) != 0
    {
        b.cam_goal = if on_foot(b) { b.pilot.yaw } else { b.yaw };
        b.l_t = u8::MAX;
        b.r_t = u8::MAX;
    }
    for (bit, turn) in [(button::L, 0u16.wrapping_sub(0x2000)), (button::R, 0x2000)] {
        let t = if bit == button::L {
            &mut b.l_t
        } else {
            &mut b.r_t
        };
        if i.held & bit != 0 {
            *t = t.saturating_add(1);
        } else {
            let tapped = *t > 0 && *t < TAP;
            *t = 0;
            if tapped {
                b.cam_goal = b.cam_goal.wrapping_add(turn);
            }
        }
    }
    // The finger: on JETS a tap switches HOLD and SINK, kept on it is
    // full lift; in the view it pushes toward itself.
    let (mut push, mut lift) = (i.push, i.lift);
    let mut jets = false;
    match i.finger {
        Some((x, y)) if in_jets(x, y) => {
            b.finger_t = b.finger_t.saturating_add(1);
            if b.finger_t >= TAP {
                lift = 256;
                jets = true;
            }
        }
        Some((x, y)) => {
            b.finger_t = 0;
            if (10..96).contains(&y) {
                push = [
                    (i32::from(x) - 80) * 256 / 40,
                    (48 - i32::from(y)) * 256 / 30,
                ];
            }
        }
        None => {
            if b.finger_t > 0 && b.finger_t < TAP && b.phase == Phase::Fly {
                b.assist = if b.assist == ASSIST_HOLD {
                    ASSIST_SINK
                } else {
                    ASSIST_HOLD
                };
            }
            b.finger_t = 0;
        }
    }
    match b.phase {
        Phase::Entry => {
            if pressed & button::A != 0 && b.phase_t >= ENTRY_SKIP {
                b.phase_t = ENTRY_TICKS;
            }
            let was = b.p;
            b.p = entry::entry_at(b.places.start, b.phase_t.min(ENTRY_TICKS));
            // The path's speed, for the panel; at rest at its end.
            b.v = [0, 1, 2].map(|k| b.p[k] - was[k]);
            if b.phase_t >= ENTRY_TICKS {
                b.phase = Phase::Fly;
                b.phase_t = 0;
                b.v = [0; 3];
            }
        }
        Phase::Fly => {
            if pressed & button::Y != 0 {
                b.assist = if b.assist == ASSIST_OFF {
                    ASSIST_SINK
                } else {
                    ASSIST_OFF
                };
            }
            if let Some(l) = lander::fly(b, push, lift) {
                b.touched = Some(l);
                b.note = Some(l.note());
                if l == Land::Greased {
                    b.bonus = b.bonus.saturating_add(tune::GREASE_BONUS);
                }
                if b.hull == 0 {
                    return Outcome::Lost;
                }
                if l.down() {
                    // Step G4: the touchdown blows a ring of dust out
                    // and lays the plants round the ship down (once: in
                    // the air the props are neither drawn nor blown,
                    // BOARD-G3.md 4.1).
                    let at = lander::at(b);
                    let g = [at[0], site::surface(&b.site, at[0], at[2]) + M / 4, at[2]];
                    for k in 0..24u8 {
                        sparks::add(&mut b.sparks, g, sparks::RING, k % 16);
                    }
                    props::blow(b, g, 10 * M);
                    b.phase = Phase::Landed;
                    b.phase_t = 0;
                    b.hold = 0;
                    b.lift = 0;
                }
            }
        }
        Phase::Landed => {
            // The jets cool on the ground.
            b.heat = (b.heat - tune::HEAT_COOL).max(0);
            b.hot = b.hot && b.heat > tune::HEAT_BACK * lander::HEAT_UNIT;
            b.hold = if i.held & button::B != 0 || jets {
                b.hold.saturating_add(1)
            } else {
                0
            };
            if b.hold >= LIFT_HOLD {
                b.hold = 0;
                if b.fuel < LIFT_FUEL {
                    b.note = Some("NO FUEL TO LIFT OFF: 0.3 LY");
                } else {
                    b.phase = Phase::TakeOff;
                    b.phase_t = 0;
                    b.base = b.p[1];
                }
            } else if b.hold == 0 && b.phase_t >= tune::HATCH_OPEN {
                // Step G3: the hatch opens and the pilot steps out.
                walker::step_out(b, b.shield);
                b.phase = Phase::Foot;
                b.phase_t = 0;
                camera::foot_follow(b, true);
            }
        }
        Phase::Foot => {
            if let Some(out) = foot(b, i, pressed, push) {
                return out;
            }
        }
        Phase::Board => {
            if b.phase_t >= tune::BOARD_TICKS {
                b.phase = Phase::TakeOff;
                b.phase_t = 0;
                b.base = b.p[1];
            }
        }
        Phase::TakeOff => {
            b.p[1] = b.base + entry::takeoff_up(b.phase_t);
            if b.phase_t >= TAKEOFF_TICKS {
                return Outcome::Lifted;
            }
        }
    }
    clock::tick(b);
    effects(b);
    if on_foot(b) {
        if b.pilot.suit <= 0 {
            b.pilot.suit = 0;
            b.note = Some("PILOT DOWN");
            return Outcome::Lost;
        }
        camera::foot_follow(b, false);
    } else {
        camera::follow(b, false);
    }
    Outcome::Stay
}

/// The pilot is out (on foot, or going in).
pub fn on_foot(b: &Below) -> bool {
    matches!(b.phase, Phase::Foot | Phase::Board)
}

/// Step G4 (BOARD-G3.md 4.1): the ship is down (landed or the pilot
/// out). Only then are the drones about (they patrol, aim, shoot and
/// are drawn) and the props drawn: in the air (the 120 m view, the
/// descent, the take-off) they cost nothing, no tick and no drawing.
pub fn ship_down(b: &Below) -> bool {
    on_foot(b) || b.phase == Phase::Landed
}

/// Step G3: a tick on foot: the right stick's camera, the walk, the
/// cutter, the loose bodies, and B held at the hatch.
fn foot(b: &mut Below, i: &In, pressed: u32, push: [i32; 2]) -> Option<Outcome> {
    let turn = i.look[0].clamp(-256, 256) * tune::FOOT_TURN / 256;
    b.cam_goal = b.cam_goal.wrapping_add(turn as u16);
    b.cam_yaw = b.cam_yaw.wrapping_add(turn as u16);
    let tilt = b.pilot.tilt - i.look[1].clamp(-256, 256) * tune::FOOT_TILT_STEP / 256;
    b.pilot.tilt = tilt.clamp(tune::FOOT_TILT[0], tune::FOOT_TILT[1]);
    let stride = b.pilot.stride;
    walker::walk(b, push, pressed & button::A != 0, i.held & button::A != 0);
    steps(b, stride);
    cutter::cut(b, i.held & button::X != 0);
    chips(b);
    loose::chunks(b);
    loose::rocks(b);
    // Step G4: Y scans.
    if pressed & button::Y != 0 {
        scan_start(b);
    }
    b.hold = if i.held & button::B != 0 && walker::at_hatch(b) {
        b.hold.saturating_add(1)
    } else {
        0
    };
    if b.hold >= LIFT_HOLD {
        b.hold = 0;
        if b.fuel < LIFT_FUEL {
            b.note = Some("NO FUEL TO LIFT OFF: 0.3 LY");
        } else {
            b.phase = Phase::Board;
            b.phase_t = 0;
        }
    }
    None
}

/// Step G4: footsteps: two puffs of dust a step (0.7 m), glitter on
/// ICE, splashes in shallow water.
fn steps(b: &mut Below, was: u32) {
    let pl = b.pilot;
    if !pl.ground || pl.stride / 180 == was / 180 {
        return;
    }
    let at = pl.at();
    let kind = if site::height(&b.site, at[0], at[2]) < b.site.wet {
        sparks::SPLASH
    } else if b.site.look == 2 {
        sparks::GLITTER
    } else {
        sparks::DUST
    };
    let v = (pl.stride / 180) as u8;
    for k in 0..2u8 {
        sparks::add(
            &mut b.sparks,
            at,
            kind,
            v.wrapping_mul(7).wrapping_add(k * 8) % 16,
        );
    }
}

/// Step G4: the cutter's chips (four a tick) at the beam's end: the
/// seam's good, sparks off metal, dust off the ground.
fn chips(b: &mut Below) {
    let c = b.cutter;
    if !c.on {
        return;
    }
    let kind = match c.lock {
        cutter::Lock::Seam(_) => sparks::CHIP_ORE,
        cutter::Lock::None if !c.dust => return,
        cutter::Lock::None => sparks::CHIP_DUST,
        _ => sparks::CHIP_METAL,
    };
    for k in 0..4u32 {
        let v = (crate::fixed::hash(b.t, k) & 15) as u8;
        sparks::add(&mut b.sparks, c.end, kind, v);
    }
}

/// Step G4: the scan (Y on foot): 20 jet energy, or a note.
fn scan_start(b: &mut Below) {
    if b.pilot.energy < tune::SCAN_COST {
        b.note = Some("JETS LOW: NO SCAN");
        return;
    }
    b.pilot.energy -= tune::SCAN_COST;
    b.scan_t = 0;
    b.scan_at = b.pilot.at();
    b.marks = 0;
}

/// The scan ring's radius now (world units).
pub fn scan_r(b: &Below) -> i32 {
    tune::SCAN_R * i32::from(b.scan_t.min(tune::SCAN_GROW)) / i32::from(tune::SCAN_GROW)
}

/// Step G4: one tick of the scan: the ring grows and marks what it
/// passes; the marks last `SCAN_MARK` after the ring is out.
fn scan_tick(b: &mut Below) {
    if b.scan_t >= tune::SCAN_GROW + tune::SCAN_MARK {
        b.marks = 0;
        return;
    }
    b.scan_t += 1;
    let r = scan_r(b);
    let o = [b.scan_at[0], b.scan_at[2]];
    let mut m = b.marks;
    let mut mark = |at: [i32; 2], bit: u16| {
        if walker::apart(at, o) <= r {
            m |= bit;
        }
    };
    for (k, s) in b.places.seams.iter().enumerate() {
        mark(*s, cutter::seam_bit(k).into());
    }
    mark(b.places.wreck, MARK_WRECK);
    for (k, d) in b.drones.iter().enumerate() {
        if d.alive() {
            mark([d.p[0], d.p[2]], mark_drone(k));
        }
    }
    for (k, rk) in b.rocks.iter().enumerate() {
        if rk.state != loose::ROCK_NONE {
            let a = rk.at();
            mark([a[0], a[2]], mark_rock(k));
        }
    }
    b.marks = m;
}

/// Step G4: every tick below: the sparks, the laid plants, the drones
/// (while the ship is down), the scan; the pilot's hop jets (their
/// cone, the scorch ring, plants laid down). The ship's jets lay the
/// plants down once, at the touchdown.
fn effects(b: &mut Below) {
    sparks::tick(b);
    props::tick(b);
    if ship_down(b) {
        drones::tick(b);
    }
    scan_tick(b);
    let pl = b.pilot;
    if on_foot(b) && pl.jets {
        let p = pl.at();
        let pack = [p[0], p[1] + M * 6 / 10, p[2]];
        for k in 0..3u8 {
            sparks::add(
                &mut b.sparks,
                pack,
                sparks::JET,
                (b.t as u8).wrapping_add(k * 5) % 16,
            );
        }
        let g = walker::ground(b, pl.p[0], pl.p[2]) >> lander::TO_WORLD;
        if p[1] - g < 3 * M && b.t.is_multiple_of(6) {
            // A ring 1 m round the spot under him, one mark at a time.
            let v = (b.t / 6 % 16) as u8;
            let a = u16::from(v) << 12;
            let mark = [p[0] + (sin(a) >> 6), g + M / 8, p[2] + (cos(a) >> 6)];
            sparks::add(&mut b.sparks, mark, sparks::SCORCH, v);
        }
        if b.t.is_multiple_of(10) {
            props::blow(b, [p[0], g, p[2]], 0);
        }
    }
}

/// Step G4: the site's loose things at the start of a visit: the
/// boulders in their places, the drones on their rounds but those
/// downed today (`downed`, one bit a drone), DRONES n/m's m.
pub fn populate(b: &mut Below, downed: u8) {
    loose::rocks_at(b);
    b.downed = downed;
    b.kills = 0;
    drones::start(b, downed);
    b.met = drones::up(b) as u8;
}

/// The ship's place below (its middle, world units).
pub fn ship_pos(b: &Below) -> V3 {
    camera::ship_at(b)
}

/// The ship's turn: its nose, leaning into the push.
pub fn ship_rot(b: &Below) -> Mat3 {
    let pitch = -b.push[1] * LEAN / 256;
    let roll = b.push[0] * LEAN / 256;
    Mat3::rotation(b.yaw, pitch as u16, roll as u16)
}

/// A camera at `eye` looking at `at` (no roll).
pub fn look_at(eye: V3, at: V3) -> Camera {
    let f = norm(sub(at, eye), ONE14);
    let f = if f == [0, 0, 0] { [0, 0, ONE14] } else { f };
    Camera {
        pos: eye,
        rot: Mat3::looking(f, [0, ONE14, 0]).transposed(),
    }
}

/// A pilot's eye on the pad (1.7 m up), looking along `yaw` and a
/// little down (for the tests and pictures; G3's walk uses it).
pub fn walk_camera(b: &Below, at: [i32; 2], yaw: u16) -> Camera {
    let y = site::surface(&b.site, at[0], at[1]) + M * 17 / 10;
    let eye = [at[0], y, at[1]];
    let ahead = [
        at[0] + ((sin(yaw) * 40) >> 6),
        y - 3 * M,
        at[1] + ((cos(yaw) * 40) >> 6),
    ];
    look_at(eye, ahead)
}

/// The detail level's number (0 LOW, 1 MID, 2 HIGH).
pub fn level(d: Detail) -> usize {
    match d {
        Detail::Low => 0,
        Detail::Mid => 1,
        Detail::High => 2,
    }
}

/// A point of the site on the view (1/16 pixel), None when it is behind
/// the near plane.
fn on_view(cam: &Camera, p: V3) -> Option<[i32; 2]> {
    let c = cam.rot.apply(sub(p, cam.pos));
    (c[2] >= NEAR).then(|| VIEW.project(c))
}

/// The ship's shadow straight below it: a hexagon on the ground's
/// heights, under 80 m, darker under 30 m. Returns (triangles, pixels).
fn shadow(f: &mut Frame, cam: &Camera, b: &Below, at: V3, alt: i32) -> (u32, u32) {
    if alt >= SHADOW_FROM || b.phase == Phase::Entry {
        return (0, 0);
    }
    let colour = if alt < SHADOW_DARK {
        palette::SHADOW
    } else {
        palette::SHADOW2
    };
    let ground = |x: i32, z: i32| [x, site::surface(&b.site, x, z) + M / 8, z];
    let Some(mid) = on_view(cam, ground(at[0], at[2])) else {
        return (0, 0);
    };
    let mut corner = [[0i32; 2]; 6];
    for (k, c) in corner.iter_mut().enumerate() {
        let a = b.yaw.wrapping_add(k as u16 * 10_923);
        let p = ground(
            at[0] + ((sin(a) * (SHADOW_R >> 4)) >> 10),
            at[2] + ((cos(a) * (SHADOW_R >> 4)) >> 10),
        );
        let Some(s) = on_view(cam, p) else {
            return (0, 0);
        };
        *c = s;
    }
    let mut px = 0;
    for k in 0..6 {
        px += fill_tri(
            f,
            &VIEW.area,
            &[mid, corner[k], corner[(k + 1) % 6]],
            colour,
        );
    }
    (6, px)
}

/// The legs, out under 30 m: four lines from the hull to the feet.
fn legs(f: &mut Frame, cam: &Camera, b: &Below, at: V3, alt: i32) {
    if alt >= LEGS_FROM {
        return;
    }
    let (s, c) = (sin(b.yaw), cos(b.yaw));
    let foot = tune::FOOT;
    for (i, j) in [(1, 1), (-1, 1), (1, -1), (-1, -1)] {
        let off = |r: i32| {
            [
                (i * c * r + j * s * r) >> 14,
                (-i * s * r + j * c * r) >> 14,
            ]
        };
        let (o, h) = (off(foot), off(foot * 2 / 3));
        let low = [at[0] + o[0], at[1], at[2] + o[1]];
        let high = [at[0] + h[0], at[1] + b.ride * 2 / 3, at[2] + h[1]];
        if let (Some(p), Some(q)) = (on_view(cam, low), on_view(cam, high)) {
            line(
                f,
                &VIEW.area,
                [p[0].div_euclid(SUB), p[1].div_euclid(SUB)],
                [q[0].div_euclid(SUB), q[1].div_euclid(SUB)],
                art::TEXT_DIM,
            );
        }
    }
}

/// Draws the view below from `cam`: the sky (and the entry's stars),
/// the land (in the entry's fog), the shadow and the dust, the ship
/// with its legs, the entry's streaks.
pub fn draw_from(f: &mut Frame, r: &mut Renderer, b: &Below, d: Detail, cam: &Camera) {
    sky::draw(f, &VIEW, cam, b.sun, b.station, b.t, entry::bands_lit(b));
    entry::stars(f, &VIEW, cam, b);
    let mut stats = r.stats;
    let mut to = land::ToFrame { f, area: VIEW.area };
    let lv = level(d);
    // Step G3: the storm closes the fog too.
    let fog = entry::fog(b).min(clock::fog(b, lv));
    land::draw(&mut to, &VIEW, cam, &b.site, lv, b.sun, fog, &mut stats);
    let at = lander::at(b);
    let ground = site::surface(&b.site, at[0], at[2]);
    let alt = at[1] - ground;
    let (t, px) = shadow(f, cam, b, at, alt);
    stats.tris += t;
    stats.pixels += px;
    r.stats = stats;
    entry::dust(f, &VIEW, cam, b, [at[0], ground, at[2]], alt);
    let shading = Shading {
        mats: art::MATS,
        shades: art::SHADES,
        light: scale14(b.sun, -ONE14),
        glow: art::GLOW,
        fog_from: 0,
        fog_to: 0,
        fog_shades: 0,
        clear: 0,
        dark_side: 6,
    };
    legs(f, cam, b, at, alt);
    let o = Object {
        model: ships::craft_model(b.ship),
        pos: ship_pos(b),
        rot: ship_rot(b),
        scale: LAND_SCALE,
        swap: (0, 0),
    };
    // Step G3: the pad under the ship, the site's things, the pilot.
    let far = if fog < 256 {
        tune::MID_R[lv] * fog / 256
    } else {
        1000 * M
    };
    pad(f, r, b, cam, &shading);
    r.draw(f, &VIEW, cam, &shading, &o, 1000 * M);
    // Step G4: the scorch marks on the ground, the things with the
    // props and drones (far to near), the smoke, the other sparks, the
    // drones' lines and bolts, the scan.
    sparks::draw(f, cam, &b.sparks, true);
    things(f, r, b, cam, &shading, far, lv);
    smoke(f, b, cam);
    sparks::draw(f, cam, &b.sparks, false);
    if ship_down(b) {
        drone_marks(f, b, cam);
    }
    scan_marks(f, b, cam);
    beam(f, b, cam);
    entry::streaks(f, b);
    clock::streaks(f, &VIEW.area, b, b.cam_yaw, palette::DUST);
}

/// A pixel inside the view.
fn in_view(x: i32, y: i32) -> bool {
    let a = VIEW.area;
    (a.x0..a.x1).contains(&x) && (a.y0..a.y1 - 1).contains(&y)
}

/// The turn of thing `k` on the ground (fixed by the site's seed).
fn turn_of(b: &Below, k: u32) -> Mat3 {
    Mat3::rotation(crate::fixed::hash(b.site.seed, 0x7E_00 + k) as u16, 0, 0)
}

/// Whether the land hides the point `top` from the camera: the ground
/// at three points on the way is over the line of sight (the land is
/// drawn first and things after it, so a thing behind a hill must not
/// be drawn). Once a thing near enough to test (up to 80 a picture).
#[inline(never)]
fn hidden(b: &Below, cam: &Camera, top: V3) -> bool {
    let c = cam.pos;
    (1..4).any(|k| {
        let p = [0, 1, 2].map(|i| c[i] + (top[i] - c[i]) * k / 4);
        site::surface(&b.site, p[0], p[2]) > p[1] + M / 4
    })
}

/// The pad (a dark slab under the landing spot) and its eight lights
/// chasing round it.
fn pad(f: &mut Frame, r: &mut Renderer, b: &Below, cam: &Camera, sh: &Shading) {
    let y = b.site.pad;
    let o = Object {
        model: &models::PAD,
        pos: [0, y, 0],
        rot: Mat3::rotation(0, 0, 0),
        scale: 256,
        swap: (0, 0),
    };
    r.draw(f, &VIEW, cam, sh, &o, 400 * M);
    let h = models::PAD_HALF;
    let lit = (b.t / 6) % 8;
    for k in 0..8u32 {
        let (x, z) = match k {
            0 => (-h, -h),
            1 => (0, -h),
            2 => (h, -h),
            3 => (h, 0),
            4 => (h, h),
            5 => (0, h),
            6 => (-h, h),
            _ => (-h, 0),
        };
        if let Some(p) = on_view(cam, [x, y + M / 8, z]) {
            let (px, py) = (p[0].div_euclid(SUB), p[1].div_euclid(SUB));
            let c = if k == lit { art::ACCENT } else { art::WARN };
            if in_view(px, py) {
                f.pixel(px as i16, py as i16, c);
            }
        }
    }
}

/// What a thing in the picture's list is (`Item::what`).
const T_SEAM: u8 = 0;
const T_WRECK: u8 = 1;
const T_ROCK: u8 = 2;
const T_CHUNK: u8 = 3;
const T_PILOT: u8 = 4;
const T_DRONE: u8 = 5;
const T_PROP: u8 = 6;
/// The list's length: the site's 38 things and up to 42 props.
const ITEMS: usize = 80;

/// A thing to draw: its depth from the camera, what, which, and a
/// prop's cell.
#[derive(Clone, Copy)]
struct Item {
    d: i32,
    what: u8,
    k: u8,
    cx: i8,
    cz: i8,
}

/// The pilot's size while he goes in (256 out of the ship).
fn pilot_scale(b: &Below) -> i32 {
    if b.phase == Phase::Board {
        256 * i32::from(tune::BOARD_TICKS.saturating_sub(b.phase_t)) / i32::from(tune::BOARD_TICKS)
    } else {
        256
    }
}

/// The seams, the wreck, the boulders, the chunks, the pilot, the
/// drones and the props (step G4), drawn far to near so a near one
/// covers a far one.
#[allow(clippy::too_many_arguments)]
fn things(
    f: &mut Frame,
    r: &mut Renderer,
    b: &Below,
    cam: &Camera,
    sh: &Shading,
    far: i32,
    lv: usize,
) {
    let mut list = [Item {
        d: 0,
        what: 0,
        k: 0,
        cx: 0,
        cz: 0,
    }; ITEMS];
    let mut n = 0;
    let mut put = |p: V3, what: u8, k: usize, cx: i32, cz: i32| {
        if n < ITEMS {
            list[n] = Item {
                d: cam.rot.apply(sub(p, cam.pos))[2],
                what,
                k: k as u8,
                cx: cx as i8,
                cz: cz as i8,
            };
            n += 1;
        }
    };
    for (k, s) in b.places.seams.iter().enumerate() {
        put([s[0], 0, s[1]], T_SEAM, k, 0, 0);
    }
    let w = b.places.wreck;
    put([w[0], 0, w[1]], T_WRECK, 0, 0, 0);
    for (k, rk) in b.rocks.iter().enumerate() {
        if rk.state != loose::ROCK_NONE {
            put(rk.at(), T_ROCK, k, 0, 0);
        }
    }
    for (k, c) in b.chunks.iter().enumerate() {
        if c.state != loose::CHUNK_NONE {
            put(c.at(), T_CHUNK, k, 0, 0);
        }
    }
    if on_foot(b) && pilot_scale(b) > 0 {
        put(b.pilot.at(), T_PILOT, 0, 0, 0);
    }
    for (k, d) in b.drones.iter().enumerate() {
        if d.alive() && ship_down(b) {
            put(d.p, T_DRONE, k, 0, 0);
        }
    }
    // The props in view within their reach (cells round the camera).
    // Step G4 (BOARD-G3 item 1, BOARD-G3.md 4.1): only while the ship
    // is down, none while the camera is higher over the pad than the
    // reach, and the reach is measured in 3D.
    let reach = tune::PROP_REACH[lv].min(far);
    let up = i64::from((cam.pos[1] - b.site.pad).max(0));
    if ship_down(b) && reach > 0 && up < i64::from(reach) {
        let c = cam.pos;
        let flat = i64::from(reach) * i64::from(reach) - up * up;
        let (c0, c1) = (
            props::cell_of(c[0] - reach, c[2] - reach),
            props::cell_of(c[0] + reach, c[2] + reach),
        );
        for cz in c0.1..=c1.1 {
            for cx in c0.0..=c1.0 {
                let Some((x, z, h)) = props::spot(&b.site, cx, cz) else {
                    continue;
                };
                // In front, within the view's sides, within reach.
                let v = cam.rot.apply(sub([x, c[1] - 2 * M, z], c));
                let side = v[0].abs() * 96 / 84;
                let (dx, dz) = (i64::from(x - c[0]), i64::from(z - c[2]));
                if v[2] < -M || side > v[2] + 3 * M || dx * dx + dz * dz > flat {
                    continue;
                }
                if let Some(p) = props::keep(b, x, z, h) {
                    put(p.at, T_PROP, 0, cx, cz);
                }
            }
        }
    }
    // Far to near (insertion: at most 80, mostly in order already).
    for i in 1..n {
        let mut j = i;
        while j > 0 && list[j - 1].d < list[j].d {
            list.swap(j - 1, j);
            j -= 1;
        }
    }
    for it in &list[..n] {
        thing(f, r, b, cam, sh, far, it);
    }
}

/// Draws one thing of the list: picks its model and place (in flash,
/// inside `draw`, so the models stay with the flash constants) and hands
/// it to `show` (RAM).
fn thing(
    f: &mut Frame,
    r: &mut Renderer,
    b: &Below,
    cam: &Camera,
    sh: &Shading,
    far: i32,
    it: &Item,
) {
    let k = usize::from(it.k);
    let (o, top, test) = match it.what {
        T_SEAM => {
            let s = b.places.seams[k];
            let st = usize::from(b.seams[k].min(3));
            let o = Object {
                model: models::SEAMS[st],
                pos: [s[0], site::surface(&b.site, s[0], s[1]), s[1]],
                rot: turn_of(b, k as u32),
                scale: 256,
                swap: (0, 0),
            };
            (o, M, true)
        }
        T_WRECK => {
            let w = b.places.wreck;
            let o = Object {
                model: &models::WRECK,
                pos: [w[0], site::surface(&b.site, w[0], w[1]), w[1]],
                rot: turn_of(b, 9),
                scale: 256,
                swap: if b.wreck >= tune::WRECK_STAGES {
                    (art::mat::HULL2, art::mat::GUNMETAL)
                } else {
                    (0, 0)
                },
            };
            (o, 2 * M, true)
        }
        T_ROCK => {
            let rk = &b.rocks[k];
            let rock = &ships::ROCK;
            let o = Object {
                model: rock,
                pos: rk.at(),
                rot: Mat3::rotation(0, rk.rolled.wrapping_mul(27), 0),
                scale: tune::BOULDER_R * 256 / rock.radius.max(1),
                swap: (art::mat::STATION2, models::ROCK),
            };
            (o, tune::BOULDER_R, true)
        }
        T_CHUNK => {
            let c = &b.chunks[k];
            let spin = if c.state == loose::CHUNK_REST {
                k as u16 * 9_000
            } else {
                c.t.wrapping_mul(1_100)
            };
            let o = Object {
                model: &models::CHUNK,
                pos: c.at(),
                rot: Mat3::rotation(spin, spin.wrapping_mul(3), 0),
                scale: 256,
                swap: (0, 0),
            };
            (o, M / 4, true)
        }
        T_PILOT => {
            let pl = &b.pilot;
            let moving = lander::hspeed(pl.v) > lander::MPS / 2 && pl.ground;
            let pose = if moving {
                1 + ((pl.stride >> 7) & 1) as usize
            } else {
                0
            };
            let o = Object {
                model: models::PILOT[pose],
                pos: pl.at(),
                rot: Mat3::rotation(pl.yaw, 0, 0),
                scale: pilot_scale(b),
                swap: (0, 0),
            };
            r.draw(f, &VIEW, cam, sh, &o, far.max(40 * M));
            return;
        }
        T_DRONE => {
            let d = &b.drones[k];
            let o = Object {
                model: &models::DRONE,
                pos: d.p,
                rot: Mat3::rotation(d.yaw, 0, 0),
                scale: 256,
                swap: if d.flash > 0 {
                    (art::mat::GUNMETAL, art::mat::FLASH)
                } else {
                    (0, 0)
                },
            };
            (o, 0, it.d > 12 * M)
        }
        _ => {
            let (cx, cz) = (i32::from(it.cx), i32::from(it.cz));
            let Some(p) = props::at_cell(b, cx, cz) else {
                return;
            };
            let lean = props::laid(b, cx, cz) * 0x3000 / 256;
            let model = if p.rock {
                &models::ROCK_PROP
            } else {
                models::PLANTS[usize::from(b.site.look % 8)]
            };
            let o = Object {
                model,
                pos: p.at,
                rot: Mat3::rotation(p.yaw, lean as u16, 0),
                scale: 256,
                swap: (0, 0),
            };
            (o, M, it.d > 16 * M)
        }
    };
    show(f, r, b, cam, sh, far, it.d, &o, top, test);
}

/// Step G4: one thing's draw, once a thing (up to 80 a picture; RAM):
/// behind the camera, or smaller than a pixel across, nothing to draw
/// and no rays over the land for it (BOARD-G3 item 1); else, unless the
/// land hides its top (`test`), the model.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn show(
    f: &mut Frame,
    r: &mut Renderer,
    b: &Below,
    cam: &Camera,
    sh: &Shading,
    far: i32,
    d: i32,
    o: &Object,
    top: i32,
    test: bool,
) {
    let rad = o.model.radius * o.scale / 256;
    if d + rad < 0 || rad * 2 * VIEW.focal < d {
        return;
    }
    let p = o.pos;
    if !test || !hidden(b, cam, [p[0], p[1] + top, p[2]]) {
        r.draw(f, &VIEW, cam, sh, o, far);
    }
}

/// Step G4: the wreck's smoke column (until it is stripped): eight
/// puffs rising to 40 m, drifting with the wind, from the clock (no
/// slots of the pool). A guide from the air and from afar (SPEC 3.16:
/// cut 5).
fn smoke(f: &mut Frame, b: &Below, cam: &Camera) {
    if b.wreck >= tune::WRECK_STAGES || b.phase == Phase::Entry {
        return;
    }
    let w = b.places.wreck;
    let base = site::surface(&b.site, w[0], w[1]) + 2 * M;
    let span = tune::SMOKE_PUFFS * tune::SMOKE_EVERY;
    for k in 0..tune::SMOKE_PUFFS {
        let age = b.t.wrapping_add(k * tune::SMOKE_EVERY) % span;
        let up = tune::SMOKE_UP * age as i32 / span as i32;
        let p = [w[0] + up / 4, base + up, w[1]];
        let c = cam.rot.apply(sub(p, cam.pos));
        if c[2] < 2 * M || hidden(b, cam, p) {
            continue;
        }
        let q = VIEW.project(c);
        // 0.6 m wide at the base, 3 m at the top.
        let size = M * 6 / 10 + up * 6 / 100;
        let rad = (size * 96 / c[2]).clamp(1, 12);
        let colour = if age * 3 < span {
            art::TEXT_DIM
        } else {
            palette::SHADOW2
        };
        crate::scene::disc(
            f,
            &VIEW.area,
            q[0].div_euclid(SUB),
            q[1].div_euclid(SUB),
            rad,
            colour,
        );
    }
}

/// A point on the view in whole pixels.
fn px(cam: &Camera, p: V3) -> Option<[i32; 2]> {
    on_view(cam, p).map(|q| [q[0].div_euclid(SUB), q[1].div_euclid(SUB)])
}

/// Step G4: each drone's telegraph (a red line from its eye to where it
/// aims, white every other 2 ticks once locked), its bolt (a red 3 px
/// streak with a hot head), its burst's flash. Never cut.
fn drone_marks(f: &mut Frame, b: &Below, cam: &Camera) {
    for d in &b.drones {
        if d.mode == drones::AIM
            && let (Some(e), Some(l)) = (px(cam, d.eye()), px(cam, d.lock))
        {
            let c = if d.locked() && (d.t / 2) % 2 == 0 {
                art::TEXT
            } else {
                art::BAD
            };
            line(f, &VIEW.area, e, l, c);
        }
        if d.bolt_t > 0 {
            let s = d.step.map(i32::from);
            let tail = [d.bolt[0] - s[0], d.bolt[1] - s[1], d.bolt[2] - s[2]];
            if let (Some(h), Some(t)) = (px(cam, d.bolt), px(cam, tail)) {
                line(f, &VIEW.area, h, t, art::BAD);
                line(f, &VIEW.area, [h[0], h[1] + 1], [t[0], t[1] + 1], art::BAD);
                if in_view(h[0], h[1]) {
                    f.pixel(h[0] as i16, h[1] as i16, art::FIRE1);
                }
            }
        }
        if d.mode == drones::BURST
            && let Some(c) = on_view(cam, d.p)
        {
            let z = cam.rot.apply(sub(d.p, cam.pos))[2].max(M);
            let rad = (3 * M * 96 / z).clamp(2, 14) - i32::from(d.t);
            crate::scene::disc(
                f,
                &VIEW.area,
                c[0].div_euclid(SUB),
                c[1].div_euclid(SUB),
                rad.max(1),
                art::FIRE1,
            );
        }
    }
}

/// A diamond mark (5 px across) at (x, y).
fn diamond(f: &mut Frame, x: i32, y: i32, c: u8) {
    for (dx, dy) in [
        (0, -2),
        (-1, -1),
        (1, -1),
        (-2, 0),
        (2, 0),
        (-1, 1),
        (1, 1),
        (0, 2),
    ] {
        if in_view(x + dx, y + dy) {
            f.pixel((x + dx) as i16, (y + dy) as i16, c);
        }
    }
}

/// Step G4: the scan: its ring of 48 points riding the ground while it
/// grows (SPEC 3.16: cut 2), then the marks over what it found: seams
/// amber, the wreck white, drones red, boulders grey.
fn scan_marks(f: &mut Frame, b: &Below, cam: &Camera) {
    if b.scan_t < tune::SCAN_GROW {
        let r = scan_r(b);
        let o = b.scan_at;
        for k in 0..tune::SCAN_POINTS {
            let a = (k * 65_536 / tune::SCAN_POINTS) as u16;
            let (x, z) = (
                o[0] + ((sin(a) * (r >> 4)) >> 10),
                o[2] + ((cos(a) * (r >> 4)) >> 10),
            );
            let p = [x, site::surface(&b.site, x, z) + M / 3, z];
            if let Some(q) = px(cam, p) {
                crate::scene::dot_px(f, &VIEW.area, q[0], q[1], art::ACCENT, 1);
            }
        }
    }
    if b.marks == 0 {
        return;
    }
    let up = M * 5 / 2;
    let mut mark = |p: V3, c: u8| {
        if let Some(q) = px(cam, [p[0], p[1] + up, p[2]]) {
            diamond(f, q[0], q[1], c);
        }
    };
    for (k, s) in b.places.seams.iter().enumerate() {
        if b.marks & u16::from(cutter::seam_bit(k)) != 0 && b.seams[k] < tune::SEAM_STAGES {
            mark(
                [s[0], site::surface(&b.site, s[0], s[1]), s[1]],
                art::ACCENT,
            );
        }
    }
    let w = b.places.wreck;
    if b.marks & MARK_WRECK != 0 && b.wreck < tune::WRECK_STAGES {
        mark([w[0], site::surface(&b.site, w[0], w[1]), w[1]], art::TEXT);
    }
    for (k, d) in b.drones.iter().enumerate() {
        if b.marks & mark_drone(k) != 0 && d.alive() {
            mark([d.p[0], d.p[1] - 2 * M, d.p[2]], art::BAD);
        }
    }
    for (k, rk) in b.rocks.iter().enumerate() {
        if b.marks & mark_rock(k) != 0 && rk.state != loose::ROCK_NONE {
            mark(rk.at(), art::TEXT_DIM);
        }
    }
}

/// The cutter's beam: a 2 px line from the hand to where it ends, in the
/// laser level's colour, with a hot spot at its end.
fn beam(f: &mut Frame, b: &Below, cam: &Camera) {
    let c = &b.cutter;
    if !c.on || !on_foot(b) {
        return;
    }
    let (Some(p), Some(q)) = (on_view(cam, cutter::hand(b)), on_view(cam, c.end)) else {
        return;
    };
    let colour = crate::scene::BEAM_LV[usize::from(b.laser.min(2))];
    let (a, e) = (
        [p[0].div_euclid(SUB), p[1].div_euclid(SUB)],
        [q[0].div_euclid(SUB), q[1].div_euclid(SUB)],
    );
    line(f, &VIEW.area, a, e, colour);
    line(f, &VIEW.area, [a[0], a[1] + 1], [e[0], e[1] + 1], colour);
    // The chips and dust at its end are in the pool (step G4).
    if in_view(e[0] - 1, e[1] - 1) && in_view(e[0] + 1, e[1] + 1) {
        f.rect(e[0] as i16 - 1, e[1] as i16 - 1, 3, 3, art::FIRE1);
    }
}

/// The picture below, from the landing camera.
#[inline(never)]
pub fn draw(f: &mut Frame, r: &mut Renderer, b: &Below, d: Detail) {
    let cam = if on_foot(b) {
        camera::foot_camera(b)
    } else {
        camera::camera(b)
    };
    draw_from(f, r, b, d, &cam);
    report(f, b);
}

/// Step G3: the report card over the take-off after a walk: the time
/// below, the haul, the drones downed (step G4) and the suit (SPEC 3.7).
fn report(f: &mut Frame, b: &Below) {
    if b.phase != Phase::TakeOff || b.pilot.suit_max == 0 || !(20..170).contains(&b.phase_t) {
        return;
    }
    crate::hud::card(f, (22, 36, 116, 24));
    let secs = u32::from(b.since) / 60;
    let mut t = crate::hud::Txt::new();
    t.s("TIME ").num(secs / 60).s(":");
    if secs % 60 < 10 {
        t.s("0");
    }
    t.num(secs % 60).s("  HAUL ").num(u32::from(b.haul)).s(" T");
    crate::hud::centred(f, 40, t.bytes(), art::TEXT);
    // Step G4: the drones downed of those met.
    let mut t = crate::hud::Txt::new();
    t.s("DRONES ")
        .num(u32::from(b.kills))
        .s("/")
        .num(u32::from(b.met))
        .s("  SUIT ")
        .num(walker::suit_points(b) as u32);
    crate::hud::centred(f, 50, t.bytes(), art::GOOD);
}

/// The panel below (SPEC 3.9, "The panel, LANDING").
#[inline(never)]
pub fn panel(f: &mut Frame, b: &Below) {
    panel::draw(f, b);
}

/// The return to space (SPEC 3.7): the ship at the ring's middle,
/// flying out along the site's direction at half speed, the lift paid.
/// Everything else in `World` is as it was when the ship entered.
#[inline(never)]
pub fn lift_off(w: &mut World, b: &Below) {
    let up = w.me.rot.col(1);
    let me = &mut w.me;
    me.pos = b.ring;
    me.rot = Mat3::looking(b.dir, up);
    me.speed = MAX_SPEED / 2;
    me.throttle = MAX_SPEED / 2;
    me.heat = 0;
    me.cool = 30;
    w.car.fuel = w.car.fuel.saturating_sub(LIFT_FUEL);
    // Step G2: the hull as the landing left it.
    w.car.hull = b.hull;
    // Step G3: the hold as the walk left it.
    w.car.hold = b.cargo;
}
