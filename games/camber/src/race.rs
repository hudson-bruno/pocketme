//! One race in fixed ticks of 1/60 s: hover handling, walls, jumps and
//! falls, boost pads, the shield and the recharge lane, bumps between
//! ships, the rivals' driving, laps and places.
//!
//! Everything is worked out in the track's own frame: `s` along the
//! middle of the road (fine units, 1/65536 m), `x` across it (right is
//! positive), `h` above the road. A ship's velocity is kept as speed
//! along its nose (`vf`), slip sideways (`vl`) and climb (`vh`); its
//! heading `psi` is measured from the road's direction. When the road
//! turns, `psi` turns the other way under the ship, so a ship that does
//! not steer drifts to the outside of a turn, as it should.

use crate::fixed::{atan2, cos, hash, isqrt, sin, turn};
use crate::track::{CHARGE, FM, GAP, ROOF, SEG_BITS, TRACKS, Track, WALL_L, WALL_R};

/// Ships in a race (the player and seven rivals).
pub const SHIPS: usize = 8;
/// Laps in a race.
pub const LAPS: i16 = 3;
/// Ticks of the start count (3, 2, 1).
pub const COUNT: u32 = 180;

/// m/s in fine units a tick.
pub const fn mps(v: i32) -> i32 {
    v * FM / 60
}

/// The hover height.
pub const HOVER: i32 = FM * 6 / 10;
/// Half a ship's width (for walls) and its length and width (for bumps).
pub const SHIP_HW: i32 = FM * 13 / 10;
const BUMP_S: i32 = FM * 42 / 10;
const BUMP_X: i32 = FM * 24 / 10;
/// Gravity (an arcade 15 m/s^2) in fine units a tick a tick.
const GRAV: i32 = 15 * FM / 3600;
/// The tunnel roof: a ship never rises above this.
pub const ROOF_H: i32 = FM * 44 / 10;
/// The fastest turn of the nose: 100 degrees a second (angle units a tick).
pub const TURN_MAX: i32 = 100 * 65_536 / 360 / 60;
/// The nose never points further than this from the road's direction.
const PSI_MAX: i32 = 70 * 65_536 / 360;
/// A rival closes on a ship ahead in its own track at no more than
/// 1/FOLLOW_CLOSE of the gap (less 1 m past touching) a tick faster than
/// that ship: 30 ticks, so the gap shuts in about half a second.
const FOLLOW_CLOSE: i32 = 30;
/// Ticks of a boost pad's push.
const BOOST_TICKS: u8 = 75;
/// The shield's top.
pub const SHIELD_MAX: i16 = 1000;
/// Below this the ship limps (and until it is back above [`LIMP_END`]).
const LIMP_END: i16 = 250;
/// Ticks a falling ship falls before it is put back.
const FALL_TICKS: u8 = 40;

// ---- THE CARVE (L / R) and THE SLING ------------------------------------------
//
// Every number of the carve is here, to be tuned after playing. Units: a
// tick is 1/60 s; speeds in fine units a tick (`mps(m/s)`); angles in
// 65,536 a turn (`deg_u(degrees)`); "a 256th" is of [`TURN_MAX`], the
// nose's fastest normal turn (100 degrees a second).
//
// How it works (README.md, "The carve"): pressing L or R at speed starts
// a carve in the way the stick points (the button's own side with the
// stick in the middle). While held, the nose turns at a rate between
// WIDE and TIGHT (never the other way), and the slip is turned back
// towards the nose instead of being scrubbed off, so the ship keeps its
// speed and its nose points further into the turn than its path. HEAT
// grows with how far the PATH really turns in the world (the road's own
// turn under the ship plus how far the path points into the carve's side
// of the road), weighted by speed; letting go pays a SLING (a push above
// top speed, stronger and longer by the heat's step) and the slip left
// over is turned into the path without losing speed (the SETTLE).

/// No carve starts below this speed (30 m/s, 108 km/h).
pub const CARVE_START_SPEED: i32 = mps(30);
/// A carve ends (heat lost) when the ship slows below this (22 m/s).
pub const CARVE_END_SPEED: i32 = mps(22);
/// The stick decides the way when pushed more than this (a 256th of
/// full); nearer the middle, the button's own side does.
pub const CARVE_PICK: i32 = 40;
/// The nose's turn while carving, in 256ths of TURN_MAX: stick full out
/// of the turn (WIDE), in the middle (MID), full into it (TIGHT).
/// 8 / 120 / 240 are about 3 / 47 / 94 degrees a second. TIGHT is just
/// below a normal full turn (100 degrees a second), so the stick held into
/// a carve slides round a hairpin instead of cutting into its inside; WIDE
/// nearly holds the nose still, so the stick can open the carve almost
/// straight.
pub const CARVE_WIDE: i32 = 8;
pub const CARVE_MID: i32 = 120;
pub const CARVE_TIGHT: i32 = 240;
/// Grip while carving: each tick the slip turns 1/CARVE_GRIP of the way
/// back to the nose, keeping the speed (a normal turn scrubs 1/12 of the
/// slip off). Bigger: a wider angle between nose and path (about
/// CARVE_GRIP ticks of the nose's turn: 20 x 0.78 degree = 16 degrees at
/// MID, 31 at TIGHT). Only on the ground; in the air the air's grip.
pub const CARVE_GRIP: i32 = 20;
/// What carving costs: the speed loses 1/CARVE_DRAG a tick (thrust on
/// keeps most of it).
pub const CARVE_DRAG: i32 = 700;
/// The settle after letting go: for this many ticks the slip the carve
/// left is turned into the path, 1/CARVE_SETTLE_PART of it a tick, with
/// no loss of speed (only slip made after the let-go is scrubbed as in a
/// normal turn). A hard wall hit or a fall ends the settle; off the
/// ground it turns nothing (its ticks still run out).
/// Only a carve held at least CARVE_SETTLE_MIN ticks settles (so tapping
/// L or R cannot turn every slip into the path for free).
pub const CARVE_SETTLE: u8 = 16;
pub const CARVE_SETTLE_PART: i32 = 4;
pub const CARVE_SETTLE_MIN: u8 = 12;
/// A wall ends a carve (heat lost) only when the ship hits it faster
/// than this across the road (4 m/s); a lighter brush keeps the carve,
/// but the wall's push turns the path back and so takes heat away.
pub const CARVE_WALL_SPEED: i32 = mps(4);
/// Heat counts the path's turn at this speed and above in full, less
/// below (in proportion).
pub const CARVE_FULL_SPEED: i32 = mps(60);
/// The heat's three steps: degrees of real turn at full speed. A whole
/// corner of 30 degrees reaches step 1, 55 step 2, 85 step 3. A straight
/// gives no more heat than the angle the path points across it, and the
/// road's width keeps that under HEAT_1.
pub const HEAT_1: i32 = 30 * 65_536 / 360;
pub const HEAT_2: i32 = 55 * 65_536 / 360;
pub const HEAT_3: i32 = 85 * 65_536 / 360;
/// Ticks off the ground before a carve ends (heat lost): a short hop
/// over a bump keeps it, a crest or the jump ends it.
pub const CARVE_AIR_TICKS: u8 = 8;
/// The size of the carve's reward: the sling of each heat step (1, 2, 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reward {
    /// The sling's top speed, 1024ths of the ship's own top speed.
    pub top: [i32; 3],
    /// Ticks of push.
    pub ticks: [u8; 3],
    /// How quickly the sling reaches its top: 1/`pull` of the difference
    /// a tick (it pushes even without thrust, but never while an air
    /// brake is on; its ticks run on meanwhile).
    pub pull: i32,
}

/// The reward of every ship: 123 / 134 / 146 % of the ship's own top
/// for 1.5 / 2.25 / 3.25 s. It is row B of the four the owner played on
/// the board and chose on 2026-11-22 (README.md, "The carve's reward",
/// keeps the comparison). MEASURED ON THE PC (a bench kept outside the repository,
/// TIME TRIAL, nine starts on each of the three tracks): the scripted
/// good carver's best flying lap 11.2 % and its 3-lap total 7.1 % faster
/// than the clean driver's at BRISK. Hard wall hits in the 2 s after a
/// let-go, against the same runs with the sling taken away: +4.1 % at
/// STEADY, -0.4 % at BRISK, +4.7 % at WILD.
pub const REWARD: Reward = Reward {
    top: [1_260, 1_370, 1_500],
    ticks: [90, 135, 195],
    pull: 30,
};

/// The heat's step (0: not yet the first).
pub fn heat_step(heat: i32) -> u8 {
    if heat >= HEAT_3 {
        3
    } else if heat >= HEAT_2 {
        2
    } else if heat >= HEAT_1 {
        1
    } else {
        0
    }
}

/// The nose's turn rate wanted while carving the way `dir` (-1 left,
/// 1 right) with `steer` (-256..256), angle units a tick (its sign is
/// always `dir`'s).
pub fn carve_rate(dir: i32, steer: i32) -> i32 {
    let a = steer.clamp(-256, 256) * dir.signum();
    let k = if a >= 0 {
        CARVE_MID + (CARVE_TIGHT - CARVE_MID) * a / 256
    } else {
        CARVE_MID + (CARVE_MID - CARVE_WIDE) * a / 256
    };
    dir.signum() * k * TURN_MAX / 256
}

/// The stick (-256..256) that makes a carve the way `dir` turn the nose
/// at `rate` (angle units a tick), as near as the carve can: the inverse
/// of [`carve_rate`], held between WIDE and TIGHT.
pub fn carve_stick(dir: i32, rate: i32) -> i32 {
    let d = dir.signum();
    let k = rate * d * 256 / TURN_MAX;
    let a = if k >= CARVE_MID {
        (k - CARVE_MID) * 256 / (CARVE_TIGHT - CARVE_MID)
    } else {
        (k - CARVE_MID) * 256 / (CARVE_MID - CARVE_WIDE)
    };
    a.clamp(-256, 256) * d
}

/// How the computer's pilots drive, one row a class (STEADY, BRISK,
/// WILD): how they carve (the same buttons and rules as the player's
/// carve) and how fast they dare to go through a corner. This row is the
/// only difference between the classes' rivals: the code is the same, so
/// it carries to every track. [`Race::setup`] copies the class's row into
/// [`Race::skill`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skill {
    /// The share of corners a pilot carves, in 256ths (which ones: a
    /// fixed hash of the ship, the lap and the corner's place, so a race
    /// plays the same every time).
    pub share: u32,
    /// The heat step it lets go at (3: it holds to the corner's end).
    pub step: u8,
    /// The room it wants to both walls to go on carving (fine units).
    pub room: i32,
    /// Ticks after a let-go before it presses again.
    pub rest: u8,
    /// How much the corner must turn over the next 64 m (8 segments,
    /// angle units) for a press: a shorter corner ends before the carve
    /// reaches its first heat step, so the carve would pay nothing.
    pub turn: i32,
    /// The corner speed it dares, in 1024ths of the bake's plan
    /// ([`crate::track::Seg::fast`]) at its ship's pace.
    pub dare: i32,
    /// Both air brakes only when over that speed by more than 1/`brake`
    /// of it (less over: it only lifts off the thrust) ...
    pub brake: i32,
    /// ... and by more than 1/`sling_brake` of it while a sling pushes
    /// (a brake stops the sling's push, so it keeps it longer).
    pub sling_brake: i32,
    /// Carving, the turn rate it plans the corner speed with, in 1024ths
    /// of the tightest carve's.
    pub carve_rate: i32,
    /// It also carves every corner it comes to over its plan's speed.
    pub need: bool,
}

/// The rivals by class: one row each, the same code for every class and
/// track. Every class carves each corner it comes to over its plan's
/// speed (`need`), and where the corner turns at least `turn` over the
/// next 64 m also on a fixed share of the others. STEADY lets go at the
/// first heat step and dares the plan's speed less 6 %; BRISK holds to
/// the second step and dares 7 % over it; WILD holds to the corner's end
/// and dares 32 % over it, with less room to the walls. While a sling
/// pushes, BRISK and WILD brake only when 1/5 over the speed they plan
/// (STEADY 1/3), so they keep the sling longer. The room never grows with
/// the class (3, 3, 2.5 m): the roads are 6 to 7.7 m from the middle to a
/// wall, and a carve throws the ship outwards. `dare`, `sling_brake` and
/// `need` were set against the owner's recorded LAGOON races with the
/// catch-up OFF (README, "The rivals"); the carve's share, step, room,
/// rest and turn come from the earlier sweeps (README, "The rivals'
/// carve").
pub const SKILLS: [Skill; 3] = [
    Skill {
        share: 52,
        step: 1,
        room: FM * 3,
        rest: 40,
        turn: 6_000,
        dare: 940,
        brake: 12,
        sling_brake: 3,
        carve_rate: 700,
        need: true,
    },
    Skill {
        share: 170,
        step: 2,
        room: FM * 3,
        rest: 30,
        turn: 6_000,
        dare: 1_100,
        brake: 12,
        sling_brake: 5,
        carve_rate: 700,
        need: true,
    },
    Skill {
        share: 240,
        step: 3,
        room: FM * 5 / 2,
        rest: 20,
        turn: 4_000,
        dare: 1_350,
        brake: 12,
        sling_brake: 5,
        carve_rate: 700,
        need: true,
    },
];

/// A rival steers round a ship ahead from 16 m, and from as far as it
/// covers in this many ticks at the speed it closes with.
const PASS_LOOK: i32 = 60;

/// A carving rival's path reaches its aim in about this many ticks, and
/// its nose its wish in about [`CARVE_NOSE`].
const CARVE_AIM: i32 = 12;
const CARVE_NOSE: i32 = 6;

/// A corner is this sharp over the next 3 segments to be worth a carve
/// (angle units), and still this sharp over 2 to hold it.
pub const CARVE_BEND: i32 = 1_400;
pub const CARVE_HOLD_BEND: i32 = 400;

/// The speed classes: top speed in m/s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Steady,
    Brisk,
    Wild,
}

impl Class {
    pub const ALL: [Class; 3] = [Class::Steady, Class::Brisk, Class::Wild];

    pub fn name(self) -> &'static str {
        match self {
            Class::Steady => "STEADY",
            Class::Brisk => "BRISK",
            Class::Wild => "WILD",
        }
    }

    pub fn top(self) -> i32 {
        mps(match self {
            Class::Steady => 58,
            Class::Brisk => 72,
            Class::Wild => 86,
        })
    }

    /// How the rivals drive in this class.
    pub fn skill(self) -> Skill {
        SKILLS[match self {
            Class::Steady => 0,
            Class::Brisk => 1,
            Class::Wild => 2,
        }]
    }

    /// What a wall costs the shield, in quarters of the full cost: half
    /// in the easy class, so a beginner who scrapes along does not end up
    /// limping for the rest of the race.
    fn harm(self) -> i32 {
        match self {
            Class::Steady => 2,
            Class::Brisk => 4,
            Class::Wild => 4,
        }
    }
}

/// The catch-up, a player's setting (the title's CATCH-UP line): with
/// LIGHT a rival far behind the player flies a little faster and one far
/// ahead a little slower, both ways the same; OFF leaves every ship as it
/// is. Its size: nothing while the rival is within [`CATCH_FREE`] of the
/// player along the race, then the rival's pace (its top speed and the
/// corner speed it plans) grows or shrinks evenly with the gap up to
/// [`CATCH_MAX`] 1024ths at [`CATCH_FULL`] and beyond. Only while the
/// race runs and neither the player nor that rival has finished, and
/// never in a time trial (no rivals).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatchUp {
    Off,
    Light,
}

impl CatchUp {
    pub const ALL: [CatchUp; 2] = [CatchUp::Light, CatchUp::Off];

    pub fn name(self) -> &'static str {
        match self {
            CatchUp::Off => "OFF",
            CatchUp::Light => "LIGHT",
        }
    }
}

/// The catch-up's gap with no effect (40 m), the gap of its full effect
/// (200 m), and that full effect: 20 / 1024 = 2.0 % of the rival's pace.
pub const CATCH_FREE: i32 = FM * 40;
pub const CATCH_FULL: i32 = FM * 200;
pub const CATCH_MAX: i32 = 20;

/// A race or a time trial (alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Race,
    Trial,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Race => "RACE",
            Mode::Trial => "TIME TRIAL",
        }
    }
}

/// The carve buttons as a pilot holds them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Carve {
    #[default]
    Off,
    /// L (or the touch strip's left end).
    Left,
    /// R (or the touch strip's right end).
    Right,
    /// L and R together: the stick alone picks the way.
    Both,
    /// Holds a carve already going but never starts one (a finger in
    /// the middle of the touch strip).
    Keep,
}

impl Carve {
    /// A carve button is down (not just kept).
    pub fn pressed(self) -> bool {
        matches!(self, Carve::Left | Carve::Right | Carve::Both)
    }
}

/// What a pilot asks of the ship in one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ctl {
    /// -256 (full left) to 256 (full right).
    pub steer: i32,
    pub thrust: bool,
    /// The air brakes. The player's controls and the computer's pilots
    /// ([`Race::drive`]) pull both together (B); the physics still knows
    /// one side alone (a tighter turn and a drift), reached today only by
    /// hand-built controls in the tests.
    pub brake_l: bool,
    pub brake_r: bool,
    /// The carve buttons (the player's, and the rivals' through
    /// [`Race::drive`]).
    pub carve: Carve,
}

/// One ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ship {
    /// Along the lap, across, above the road (fine units).
    pub s: i32,
    pub x: i32,
    pub h: i32,
    /// Times over the line: 0 on the grid, 1 on lap 1, LAPS + 1 finished.
    pub lap: i16,
    /// The highest `lap` seen (a lap is timed once, going forwards).
    pub counted: i16,
    /// Speed along the nose, slip, climb (fine units a tick).
    pub vf: i32,
    pub vl: i32,
    pub vh: i32,
    /// The nose's direction from the road's (angle units).
    pub psi: i32,
    /// How fast the nose turns now (angle units a tick).
    pub rate: i32,
    pub shield: i16,
    pub limp: bool,
    /// Ticks of boost left.
    pub boost: u8,
    /// Ticks fallen (0: not falling).
    pub fall: u8,
    /// Ticks it passes through others (after being put back).
    pub ghost: u8,
    /// Ticks since a wall or ship was hit (for sparks and the shake).
    pub hit: u8,
    /// Top speed and corner speed in 1/1024 of the class's.
    pub pace: i16,
    /// The rival's own lane next to the racing line (1/4 m).
    pub lane: i8,
    /// Last tick's steering and brakes (for the picture).
    pub ctl: Ctl,
    /// The road's climb under the ship last tick (for leaving crests).
    pub pitch: i32,
    /// The tick the current lap began, the best lap, the finish (0: not).
    pub lap_start: u32,
    pub best: u32,
    pub finish: u32,
    /// Where the ship was last on safe road (fine units along the lap).
    pub safe: i32,
    /// Carving: -1 left, 1 right, 0 not.
    pub carve: i8,
    /// The carve's heat (angle units of real turn, at most [`HEAT_3`]).
    pub heat: i32,
    /// Ticks of sling left, and its step (for the picture).
    pub sling: u8,
    pub sling_step: u8,
    /// Ticks off the ground while carving.
    pub air: u8,
    /// Ticks the carve has been held (at most 255).
    pub held: u8,
    /// Ticks of settle left after a let-go, and the slip (fine units a
    /// tick, the sign of `vl`) the carve left that it still turns into
    /// the path.
    pub settle: u8,
    pub lean: i32,
    /// Ticks since the last carve ended (at most 255; a pilot's rest).
    pub rest: u8,
}

impl Ship {
    pub const fn new() -> Ship {
        Ship {
            s: 0,
            x: 0,
            h: HOVER,
            lap: 0,
            counted: 0,
            vf: 0,
            vl: 0,
            vh: 0,
            psi: 0,
            rate: 0,
            shield: SHIELD_MAX,
            limp: false,
            boost: 0,
            fall: 0,
            ghost: 0,
            hit: 0,
            pace: 1024,
            lane: 0,
            ctl: Ctl {
                steer: 0,
                thrust: false,
                brake_l: false,
                brake_r: false,
                carve: Carve::Off,
            },
            pitch: 0,
            lap_start: 0,
            best: 0,
            finish: 0,
            safe: 0,
            carve: 0,
            heat: 0,
            sling: 0,
            sling_step: 0,
            air: 0,
            held: 0,
            settle: 0,
            lean: 0,
            rest: 0,
        }
    }

    /// Velocity in the track's frame: along and across (fine a tick).
    #[inline(never)]
    pub fn v_track(&self) -> (i32, i32) {
        let (sp, cp) = (sin(self.psi as u16), cos(self.psi as u16));
        (
            mul14(self.vf, cp) - mul14(self.vl, sp),
            mul14(self.vf, sp) + mul14(self.vl, cp),
        )
    }

    /// Sets the velocity from the track's frame.
    #[inline(never)]
    fn set_v_track(&mut self, vs: i32, vx: i32) {
        let (sp, cp) = (sin(self.psi as u16), cos(self.psi as u16));
        self.vf = mul14(vs, cp) + mul14(vx, sp);
        self.vl = -mul14(vs, sp) + mul14(vx, cp);
    }

    /// Distance along the race from the start line (fine units, may be
    /// below zero on the grid).
    pub fn prog(&self, len: i32) -> i64 {
        i64::from(self.lap - 1) * i64::from(len) + i64::from(self.s)
    }

    pub fn finished(&self) -> bool {
        self.finish != 0
    }

    /// Ends a carve and loses its heat (a wall, the air, too slow, a fall);
    /// no settle.
    pub fn lose_carve(&mut self) {
        self.carve = 0;
        self.heat = 0;
        self.air = 0;
        self.held = 0;
        self.settle = 0;
        self.lean = 0;
    }

    /// How far the path points into the carve's side of the road (`dir`:
    /// -1 left, 1 right), angle units, 0 when it points the other way.
    fn lean_in(&self, dir: i32) -> i32 {
        let (vs, vx) = self.v_track();
        let a = i32::from(atan2(i64::from(vx), i64::from(vs)) as i16);
        (a * dir).max(0)
    }
}

/// The length of (f, l) (speeds in fine units a tick).
fn mag(f: i32, l: i32) -> i64 {
    isqrt((i64::from(f) * i64::from(f) + i64::from(l) * i64::from(l)) as u64) as i64
}

/// Takes `part` off the slip `vl`, keeping the speed `|(vf, vl)|` (less
/// 1/`drag` of it; 0: none): the path turns towards the nose.
fn turn_slip(vf: &mut i32, vl: &mut i32, part: i32, drag: i64) {
    let v = mag(*vf, *vl);
    let nl = *vl - part;
    let m = mag(*vf, nl).max(1);
    let keep = if drag > 0 { v - v / drag } else { v };
    *vf = (i64::from(*vf) * keep / m) as i32;
    *vl = (i64::from(nl) * keep / m) as i32;
}

impl Default for Ship {
    fn default() -> Self {
        Ship::new()
    }
}

#[inline(never)]
fn mul14(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 14) as i32
}

/// Where the race is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The start count: ships wait.
    Count,
    Run,
    /// The player is over the line; the others finish.
    Done,
}

/// Something worth telling (the log, the sounds a later version has).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ev {
    Go,
    Lap { ship: u8, lap: i16, time: u32 },
    Finish { ship: u8, place: u8, time: u32 },
    Fell { ship: u8 },
    Wall { ship: u8 },
    Boost { ship: u8 },
}

/// A race.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Race {
    pub track: u8,
    pub class: Class,
    pub mode: Mode,
    pub ships: [Ship; SHIPS],
    /// Ships in this race (1 in a time trial).
    pub n: u8,
    pub phase: Phase,
    /// Ticks since the race was set up.
    pub tick: u32,
    /// Ship numbers in race order (first is leading).
    pub order: [u8; SHIPS],
    /// The places given out so far.
    pub placed: u8,
    /// Things that happened since the last [`Race::take_ev`].
    evs: [Option<Ev>; 8],
    /// Ticks since the player finished.
    pub after: u32,
    /// How the rivals drive: their class's row of [`SKILLS`] (set by
    /// [`Race::setup`]; a measuring tool may change it after that).
    pub skill: Skill,
    /// The catch-up (OFF after [`Race::setup`]; the game sets the
    /// player's choice right after it).
    pub catchup: CatchUp,
}

/// The rivals' ships: their top speed and planned corner speed, of 1024
/// of the class's (the player's ship is 1024). The first three are the
/// player's own ship; the other four, the pack, are 5.5 to 10.2 % slower
/// and 1.6 % apart, so they spread out. The seed turns the list round
/// over the grid.
pub const PACE: [i16; SHIPS - 1] = [1024, 1024, 1024, 968, 952, 936, 920];
const LANES: [i8; SHIPS - 1] = [-4, 5, -6, 3, 6, -3, 0];

impl Race {
    pub const fn new() -> Race {
        Race {
            track: 0,
            class: Class::Brisk,
            mode: Mode::Race,
            ships: [Ship::new(); SHIPS],
            n: SHIPS as u8,
            phase: Phase::Count,
            tick: 0,
            order: [0, 1, 2, 3, 4, 5, 6, 7],
            placed: 0,
            evs: [None; 8],
            after: 0,
            skill: SKILLS[1],
            catchup: CatchUp::Off,
        }
    }

    pub fn tr(&self) -> &'static Track {
        &TRACKS[usize::from(self.track) % TRACKS.len()]
    }

    /// Sets up a race: the grid two by two behind the line, the player
    /// at the back of the field (`grid` 7) or alone.
    pub fn setup(&mut self, track: u8, class: Class, mode: Mode, seed: u32) {
        *self = Race::new();
        self.track = track;
        self.class = class;
        self.mode = mode;
        self.skill = class.skill();
        self.n = if mode == Mode::Trial { 1 } else { SHIPS as u8 };
        let len = self.tr().len();
        // Rival 0 starts at the front; the player last.
        let shift = (seed % 7) as usize;
        for i in 0..usize::from(self.n) {
            let grid = if i == 0 {
                usize::from(self.n) - 1
            } else {
                i - 1
            };
            let tr = self.tr();
            let sh = &mut self.ships[i];
            sh.s = len - FM * 10 - (grid as i32) * FM * 9;
            sh.x = if grid % 2 == 0 { -FM * 3 } else { FM * 3 };
            if i > 0 {
                sh.pace = PACE[(i - 1 + shift) % (SHIPS - 1)];
                sh.lane = LANES[i - 1];
            }
            sh.safe = sh.s;
            sh.pitch = tr.at(sh.s).pitch;
        }
        self.sort();
    }

    pub fn take_ev(&mut self) -> Option<Ev> {
        let e = self.evs[0];
        if e.is_some() {
            self.evs.copy_within(1.., 0);
            self.evs[7] = None;
        }
        e
    }

    #[inline(never)]
    fn note(&mut self, e: Ev) {
        if let Some(slot) = self.evs.iter_mut().find(|x| x.is_none()) {
            *slot = Some(e);
        }
    }

    /// The player's place (1 is first).
    pub fn place(&self, ship: u8) -> u8 {
        self.order
            .iter()
            .take(usize::from(self.n))
            .position(|&o| o == ship)
            .unwrap_or(0) as u8
            + 1
    }

    /// Ticks the race clock shows (0 during the count).
    pub fn clock(&self) -> u32 {
        self.tick.saturating_sub(COUNT)
    }

    /// One tick. `player` is what the player asks; `None` lets the
    /// computer drive the player's ship too (after the finish, and in
    /// the tests).
    pub fn step(&mut self, player: Option<Ctl>) {
        self.tick = self.tick.saturating_add(1);
        if self.phase == Phase::Count {
            if self.tick >= COUNT {
                self.phase = Phase::Run;
                self.note(Ev::Go);
                for sh in self.ships.iter_mut() {
                    sh.lap_start = COUNT;
                }
            } else {
                return;
            }
        }
        if self.phase == Phase::Done {
            self.after = self.after.saturating_add(1);
        }
        let n = usize::from(self.n);
        for i in 0..n {
            let ctl = match player {
                _ if i == 0 && self.ships[0].finished() => self.cruise(0),
                Some(c) if i == 0 => c,
                _ => self.drive(i),
            };
            self.move_ship(i, ctl);
        }
        if n > 1 {
            self.bumps();
        }
        self.sort();
    }

    /// The race order: laps, then distance (finished ships by place).
    fn sort(&mut self) {
        let n = usize::from(self.n);
        let len = self.tr().len();
        let key = |sh: &Ship| -> i64 {
            if sh.finished() {
                i64::MAX - i64::from(sh.finish)
            } else {
                sh.prog(len)
            }
        };
        for i in 1..n {
            let mut j = i;
            while j > 0
                && key(&self.ships[usize::from(self.order[j])])
                    > key(&self.ships[usize::from(self.order[j - 1])])
            {
                self.order.swap(j, j - 1);
                j -= 1;
            }
        }
    }

    // ---- Moving one ship ---------------------------------------------------

    /// Ship `i`'s pace now (1/1024 of the class's top and plan): its own,
    /// changed by the catch-up for a rival (see [`CatchUp`]).
    pub fn pace_of(&self, i: usize) -> i32 {
        let sh = &self.ships[i];
        let p = i32::from(sh.pace);
        let me = &self.ships[0];
        if i == 0
            || self.catchup == CatchUp::Off
            || self.phase != Phase::Run
            || sh.finished()
            || me.finished()
        {
            return p;
        }
        let len = self.tr().len();
        // Positive: the rival is behind the player.
        let d = me.prog(len) - sh.prog(len);
        let span = i64::from(CATCH_FULL - CATCH_FREE);
        let m = (d.abs() - i64::from(CATCH_FREE)).clamp(0, span);
        let k = (m * i64::from(CATCH_MAX) / span) as i32 * d.signum() as i32;
        p + p * k / 1024
    }

    fn move_ship(&mut self, i: usize, ctl: Ctl) {
        let pace = self.pace_of(i);
        let tr = self.tr();
        let len = tr.len();
        let top = self.class.top();
        let harm = self.class.harm();
        let tick = self.tick;
        let rw = REWARD;
        let mut ev: Option<Ev> = None;
        let mut fin: Option<u32> = None;
        {
            let sh = &mut self.ships[i];
            let was = sh.ctl.carve;
            sh.ctl = ctl;
            sh.rest = if sh.carve != 0 {
                0
            } else {
                sh.rest.saturating_add(1)
            };
            sh.hit = sh.hit.saturating_sub(1);
            sh.ghost = sh.ghost.saturating_sub(1);
            if sh.fall > 0 {
                // Falling: no road, no control; put back after a moment.
                sh.lose_carve();
                sh.fall = sh.fall.saturating_add(1);
                sh.vh -= GRAV;
                sh.h = sh.h.saturating_add(sh.vh);
                let (vs, _) = sh.v_track();
                sh.s += vs.max(0) / 2;
                if sh.fall > FALL_TICKS {
                    respawn(sh, tr);
                }
                wrap(sh, len);
                return;
            }
            let here = tr.seg(tr.index(sh.s));
            let spot = tr.at(sh.s);
            let grounded = sh.h < HOVER + FM * 3 / 10;

            // The carve: a press at speed starts it, letting go slings
            // and settles, the air or a crawl ends it (a wall: below).
            if sh.carve == 0 {
                if ctl.carve.pressed() && !was.pressed() && grounded && sh.vf >= CARVE_START_SPEED {
                    let dir = if ctl.steer.abs() > CARVE_PICK {
                        ctl.steer.signum()
                    } else {
                        match ctl.carve {
                            Carve::Left => -1,
                            Carve::Right => 1,
                            _ => 0,
                        }
                    };
                    if dir != 0 {
                        sh.lose_carve();
                        sh.carve = dir as i8;
                    }
                }
            } else if ctl.carve == Carve::Off {
                let step = heat_step(sh.heat);
                if step > 0 {
                    sh.sling = rw.ticks[usize::from(step - 1)];
                    sh.sling_step = step;
                }
                let dir = i32::from(sh.carve);
                let held = sh.held;
                sh.lose_carve();
                // The slip a real carve leaves (on the side away from its
                // turn) settles into the path; a tap leaves none.
                if held >= CARVE_SETTLE_MIN && sh.vl * dir < 0 {
                    sh.settle = CARVE_SETTLE;
                    sh.lean = sh.vl;
                }
            } else {
                sh.held = sh.held.saturating_add(1);
                sh.air = if grounded {
                    0
                } else {
                    sh.air.saturating_add(1)
                };
                if sh.air > CARVE_AIR_TICKS || sh.vf < CARVE_END_SPEED {
                    sh.lose_carve();
                }
            }
            let dir = i32::from(sh.carve);
            // Where the path pointed into the carve's side at the start
            // (for the heat, below).
            let lean0 = if dir != 0 { sh.lean_in(dir) } else { 0 };

            // Steering: the nose turns at most TURN_MAX (more with the air
            // brake on the same side), less at a crawl and in the air;
            // carving, between WIDE and TIGHT, always the carve's way.
            let mut want = if dir != 0 {
                carve_rate(dir, ctl.steer)
            } else {
                ctl.steer.clamp(-256, 256) * TURN_MAX / 256
            };
            if dir == 0 && ((ctl.steer < 0 && ctl.brake_l) || (ctl.steer > 0 && ctl.brake_r)) {
                want = want * 3 / 2;
            }
            let slow = sh.vf.clamp(0, mps(15));
            want = want * slow / mps(15);
            if !grounded {
                want /= 2;
            }
            sh.rate += (want - sh.rate) / 4;
            if sh.rate * dir < 0 {
                sh.rate = 0;
            }
            let w = sh.rate;
            if w != 0 {
                // The velocity stays; the nose turns under it.
                let (sw, cw) = (sin(w as u16), cos(w as u16));
                let vf = mul14(sh.vf, cw) + mul14(sh.vl, sw);
                let vl = -mul14(sh.vf, sw) + mul14(sh.vl, cw);
                sh.vf = vf;
                sh.vl = vl;
                sh.psi += w;
            }
            // Thrust, drag and the air brakes, along the nose.
            let mut vtop = top * pace / 1024;
            if sh.limp {
                vtop = vtop * 6 / 10;
            }
            let own_top = vtop;
            if sh.boost > 0 {
                vtop = vtop * 13 / 10;
                sh.boost -= 1;
            }
            // The sling pushes for its ticks, but not while an air brake
            // is on: its ticks run on, so braking for the next corner
            // costs the push and the brake always works.
            let slinging = sh.sling > 0 && !ctl.brake_l && !ctl.brake_r;
            if sh.sling > 0 {
                if slinging {
                    let k = usize::from(sh.sling_step.clamp(1, 3) - 1);
                    vtop = vtop.max(own_top * rw.top[k] / 1024);
                }
                sh.sling -= 1;
                if sh.sling == 0 {
                    sh.sling_step = 0;
                }
            }
            if (ctl.thrust || slinging) && grounded {
                let k = if sh.boost > 0 { 40 } else { 96 };
                let k = if slinging { k.min(rw.pull.max(1)) } else { k };
                if sh.vf < vtop {
                    sh.vf += (vtop - sh.vf) / k + 1;
                } else {
                    sh.vf -= (sh.vf - vtop) / 64;
                }
            } else {
                sh.vf -= sh.vf / 512;
            }
            match (ctl.brake_l, ctl.brake_r) {
                (true, true) => sh.vf -= sh.vf / 90,
                (true, false) | (false, true) => sh.vf -= sh.vf / 400,
                _ => {}
            }
            // Grip: the slip dies away, slower with an air brake on (a
            // drift) and much slower in the air.
            let grip = if !grounded {
                200
            } else if ctl.brake_l != ctl.brake_r {
                40
            } else {
                12
            };
            if dir != 0 && grounded {
                // Carving: the slip turns back towards the nose and the
                // speed stays (less a little drag).
                let part = sh.vl / CARVE_GRIP;
                turn_slip(&mut sh.vf, &mut sh.vl, part, i64::from(CARVE_DRAG));
            } else if dir == 0 && sh.settle > 0 {
                // The settle: the carve's slip left (no more than the slip
                // there is on its side) turns into the path at no cost;
                // any other slip is scrubbed as usual.
                sh.settle -= 1;
                let lean = if sh.lean.signum() == sh.vl.signum() {
                    sh.lean.signum() * sh.lean.abs().min(sh.vl.abs())
                } else {
                    0
                };
                let part = if grounded {
                    lean / CARVE_SETTLE_PART + lean.signum()
                } else {
                    0
                };
                let part = if part.abs() > lean.abs() { lean } else { part };
                turn_slip(&mut sh.vf, &mut sh.vl, part, 0);
                let left = lean - part;
                sh.vl -= (sh.vl - left) / grip;
                sh.lean = if sh.settle == 0 { 0 } else { left };
            } else {
                sh.vl -= sh.vl / grip;
            }

            // Bank: gravity pulls along a tilted road.
            let (vs, mut vx) = sh.v_track();
            if grounded {
                vx += mul14(GRAV, sin(spot.bank as u16));
            }

            // Along the road. The middle of the road moves faster than an
            // inner lane and slower than an outer one.
            let dyaw = turn(here.yaw, tr.seg(tr.index(sh.s) + 1).yaw);
            let d: i64 = 10_430 * (1 << SEG_BITS);
            let den = (d - i64::from(dyaw) * i64::from(sh.x)).max(d / 2);
            let ds = (i64::from(vs) * d / den) as i32;
            let dth = ((i64::from(dyaw) * i64::from(ds)) >> SEG_BITS) as i32;
            sh.s += ds;
            sh.x += vx;
            sh.psi -= dth;
            // The world velocity stays as the road turns under it:
            // speed along the nose and slip are unchanged.
            if sh.psi.abs() > PSI_MAX {
                let lim = PSI_MAX * sh.psi.signum();
                let (vs2, vx2) = sh.v_track();
                sh.psi = lim;
                sh.set_v_track(vs2, vx2);
                sh.rate = 0;
            }

            // Up and down: leaving a crest throws the ship up; gravity and
            // the hover cushion bring it back.
            let at = tr.at(sh.s);
            let dp = sh.pitch - at.pitch;
            sh.pitch = at.pitch;
            sh.vh += (i64::from(vs.max(0)) * i64::from(dp) / 10_430) as i32;
            sh.vh -= GRAV;
            let seg = tr.seg(at.seg);
            let off = sh.x.abs() > at.half * 256 + FM * 3 / 10;
            let floor = seg.kind & GAP == 0 && !off;
            if floor && sh.h > -FM / 2 {
                if sh.h < HOVER {
                    sh.vh += (HOVER - sh.h) / 8 - sh.vh / 4;
                }
                if sh.h < FM / 5 {
                    sh.h = FM / 5;
                    sh.vh = sh.vh.max(0);
                }
            }
            sh.h += sh.vh;
            if seg.kind & ROOF != 0 && sh.h > ROOF_H {
                sh.h = ROOF_H;
                sh.vh = sh.vh.min(0);
            }
            if sh.h < -FM / 2 {
                sh.fall = 1;
                sh.lose_carve();
                ev = Some(Ev::Fell { ship: i as u8 });
            }

            // Walls: the ship is held inside, bounced off and loses shield.
            let half = at.half * 256;
            let lim = (half - SHIP_HW).max(0);
            let mut wall_hit = 0;
            let mut wall_into = 0;
            for side in [-1i32, 1] {
                let has = if side < 0 {
                    seg.kind & WALL_L != 0
                } else {
                    seg.kind & WALL_R != 0
                };
                if has && sh.x * side > lim && sh.fall == 0 {
                    let (vs2, vx2) = sh.v_track();
                    sh.x = lim * side;
                    let into = (vx2 * side).max(0);
                    let vx3 = if into > 0 { -vx2 * 3 / 10 } else { vx2 };
                    let vs3 = vs2 - vs2 / 48;
                    // The nose turns along the wall.
                    if sh.psi * side > 0 {
                        sh.psi -= sh.psi / 4;
                    }
                    sh.set_v_track(vs3, vx3);
                    if into > 0 {
                        wall_hit = wall_hit.max(into / 384 + 1);
                        wall_into = wall_into.max(into);
                    }
                }
            }
            if wall_into > CARVE_WALL_SPEED {
                // A hard hit ends a carve; its heat is lost.
                sh.lose_carve();
            }
            if sh.carve != 0 {
                // Heat: how far the path really turned in the world this
                // tick, the carve's way: the road's own turn under the
                // ship and the change of how far the path points into the
                // carve's side of the road (a wall's push takes it back).
                // Weighted by speed.
                let d = i32::from(sh.carve);
                let turned = dth * d + sh.lean_in(d) - lean0;
                let v = mag(sh.vf, sh.vl).min(i64::from(CARVE_FULL_SPEED));
                let gain = i64::from(turned) * v / i64::from(CARVE_FULL_SPEED);
                sh.heat = (i64::from(sh.heat) + gain).clamp(0, i64::from(HEAT_3)) as i32;
            }
            if wall_hit > 0 {
                if sh.hit == 0 && wall_hit > 8 {
                    ev = Some(Ev::Wall { ship: i as u8 });
                }
                sh.hit = 8;
                let cost = (wall_hit * harm / 4).max(1);
                sh.shield = (sh.shield - cost as i16).max(0);
            }

            // Pads and the recharge lane (on the road, low enough).
            if sh.h < HOVER + FM && floor {
                if seg.kind & crate::track::BOOST != 0 {
                    let pad = i32::from(seg.pad) * FM / 2;
                    if (sh.x - pad).abs() < FM * 17 / 10 {
                        if sh.boost < BOOST_TICKS - 10 {
                            ev = Some(Ev::Boost { ship: i as u8 });
                        }
                        sh.boost = BOOST_TICKS;
                    }
                }
                if seg.kind & CHARGE != 0 && sh.x < -half + FM * 35 / 10 {
                    sh.shield = (sh.shield + 4).min(SHIELD_MAX);
                }
            }
            if sh.shield == 0 {
                sh.limp = true;
            } else if sh.shield > LIMP_END {
                sh.limp = false;
            }
            if floor && sh.h >= 0 && seg.kind & GAP == 0 {
                sh.safe = sh.s;
            }

            // Laps.
            wrap(sh, len);
            let new_lap = sh.lap > sh.counted;
            if new_lap {
                sh.counted = sh.lap;
            }
            if new_lap && sh.lap >= 2 && !sh.finished() {
                let t = tick - sh.lap_start;
                if sh.best == 0 || t < sh.best {
                    sh.best = t;
                }
                sh.lap_start = tick;
                if sh.lap > LAPS {
                    sh.finish = tick - COUNT;
                    fin = Some(sh.finish);
                } else {
                    ev = Some(Ev::Lap {
                        ship: i as u8,
                        lap: sh.lap,
                        time: t,
                    });
                }
            }
        }
        if let Some(t) = fin {
            self.placed += 1;
            self.note(Ev::Finish {
                ship: i as u8,
                place: self.placed,
                time: t,
            });
            if i == 0 {
                self.phase = Phase::Done;
            }
        }
        if let Some(e) = ev {
            self.note(e);
        }
    }

    // ---- Bumps -------------------------------------------------------------

    /// Ships close together push apart. A finished ship is out of the
    /// race: it touches nobody (a slow cruising ship would otherwise stand
    /// in the way of ships still racing).
    fn bumps(&mut self) {
        let n = usize::from(self.n);
        let len = self.tr().len();
        for a in 0..n {
            for b in a + 1..n {
                let (sa, sb) = (&self.ships[a], &self.ships[b]);
                if sa.fall > 0
                    || sb.fall > 0
                    || sa.ghost > 0
                    || sb.ghost > 0
                    || sa.finished()
                    || sb.finished()
                {
                    continue;
                }
                if (sa.h - sb.h).abs() > FM * 2 {
                    continue;
                }
                let ds = gap(sa.s, sb.s, len);
                let dx = sb.x - sa.x;
                if ds.abs() >= BUMP_S || dx.abs() >= BUMP_X {
                    continue;
                }
                let ox = BUMP_X - dx.abs();
                let os = BUMP_S - ds.abs();
                let (va, xa) = self.ships[a].v_track();
                let (vb, xb) = self.ships[b].v_track();
                if ox * 7 < os * 4 {
                    // Side by side: pushed apart, the sideways speeds shared.
                    let sgn = if dx >= 0 { 1 } else { -1 };
                    self.ships[a].x -= sgn * (ox / 2 + 1);
                    self.ships[b].x += sgn * (ox / 2 + 1);
                    let mid = (xa + xb) / 2;
                    let push = sgn * mps(3);
                    self.ships[a].set_v_track(va, mid - push);
                    self.ships[b].set_v_track(vb, mid + push);
                } else {
                    // Nose to tail: the one behind gives speed to the one
                    // ahead.
                    let sgn = if ds >= 0 { 1 } else { -1 };
                    self.ships[a].s -= sgn * (os / 2 + 1);
                    self.ships[b].s += sgn * (os / 2 + 1);
                    let (back, front) = if ds >= 0 { (a, b) } else { (b, a) };
                    let (vback, vfront) = if ds >= 0 { (va, vb) } else { (vb, va) };
                    if vback > vfront {
                        let d = (vback - vfront) * 6 / 10;
                        let xs = if ds >= 0 { (xa, xb) } else { (xb, xa) };
                        self.ships[back].set_v_track(vback - d, xs.0);
                        self.ships[front].set_v_track(vfront + d / 2, xs.1);
                    }
                }
                for k in [a, b] {
                    let sh = &mut self.ships[k];
                    if sh.hit == 0 {
                        sh.shield = (sh.shield - 6).max(0);
                    }
                    sh.hit = 8;
                }
                for k in [a, b] {
                    wrap(&mut self.ships[k], len);
                }
            }
        }
    }

    // ---- The rivals ---------------------------------------------------------

    /// The computer's driving for ship `i`: [`Race::drive_plain`], and
    /// the carve in the corners its class's [`Skill`] picks, with the
    /// player's buttons and rules (a press with the stick in the middle,
    /// the stick held for the arc the pilot wants, letting go where the
    /// corner ends, a wall comes close or the class's heat step is
    /// reached). It never pulls one air brake alone (the player cannot),
    /// and presses only with the class's room to both walls.
    pub fn drive(&self, i: usize) -> Ctl {
        let mut c = self.drive_plain(i);
        // No air brake on one side alone: the player cannot pull one.
        let both = c.brake_l && c.brake_r;
        c.brake_l = both;
        c.brake_r = both;
        let sh = &self.ships[i];
        let tr = self.tr();
        let sk = self.skill;
        let bend = |n: usize| {
            let k = tr.index(sh.s);
            turn(tr.seg(k).yaw, tr.seg(k + n).yaw)
        };
        // Room to the wall on side `side` (-1 left, 1 right).
        let half = tr.at(sh.s).half * 256;
        let room = |side: i32| half - sh.x * side;
        let side = |dir: i32| if dir < 0 { Carve::Left } else { Carve::Right };
        let dir = i32::from(sh.carve);
        if dir != 0 {
            if bend(2) * dir > CARVE_HOLD_BEND
                && room(dir) > sk.room
                && room(-dir) > sk.room
                && (heat_step(sh.heat) < sk.step || sk.step >= 3)
                && c.steer * dir > -200
            {
                c.carve = side(dir);
                c.steer = carve_stick(dir, c.steer * TURN_MAX / 256);
            }
            return c;
        }
        let b = bend(3);
        // Over its plan for the corner (no thrust, maybe braking): a carve
        // takes the corner without the grip running out.
        let need = !c.thrust && sk.need;
        if sh.rest > sk.rest
            && b.abs() > CARVE_BEND
            && (need || bend(8) * b.signum() >= sk.turn)
            && sh.vf > CARVE_START_SPEED + FM / 6
            && sh.fall == 0
            && !sh.ctl.carve.pressed()
            && room(-1) > sk.room
            && room(1) > sk.room
        {
            // Which corners: a fixed hash of ship, lap and the corner's
            // place (8 segments a place).
            let place = (tr.index(sh.s) >> 3) as u32;
            let lap = sh.lap.max(0) as u32;
            if need || hash(i as u32 * 1_009 + lap * 7, place) % 256 < sk.share {
                c.carve = side(b.signum());
                c.steer = 0;
            }
        }
        c
    }

    /// The computer's driving for ship `i` without the carve: follow the
    /// racing line in its own lane, look for room to pass, take boost pads
    /// and the recharge lane when the shield is low, and brake for what
    /// the bake says each turn allows.
    pub fn drive_plain(&self, i: usize) -> Ctl {
        let tr = self.tr();
        let len = tr.len();
        let sh = &self.ships[i];
        let (vs, _) = sh.v_track();
        let v = vs.max(mps(10));
        let look = FM * 10 + v * 21;
        let st = sh.s + look;
        let at = tr.at(st);
        // The narrowest of here, halfway and the look point: a road that
        // narrows ahead must not hold the aim past its wall.
        let half = at.half.min(tr.half(sh.s)).min(tr.half(sh.s + look / 2)) * 256;
        // Carving, the aim keeps clear of the room the carve needs, so the
        // carve is not let go (into a wall) by the room rule.
        let edge = if sh.carve != 0 {
            self.skill.room + FM * 7 / 10
        } else {
            FM * 18 / 10
        };
        let room = (half - edge).max(0);
        let mut tx = tr.line(st) + i32::from(sh.lane) * FM / 4;
        // A pad anywhere from the ship's segment to the look point's: aim
        // at the nearest, so the ship is on it when it gets there and the
        // aim does not jump once the look point has passed it.
        let k0 = tr.index(sh.s);
        let n = ((st >> SEG_BITS) - (sh.s >> SEG_BITS)).max(0) as usize;
        let mut k = 0;
        while k <= n {
            let p = tr.seg(k0 + k);
            if p.kind & crate::track::BOOST != 0 {
                tx = i32::from(p.pad) * FM / 2;
                break;
            }
            k += 1;
        }
        // The recharge lane close ahead.
        let ahead = tr.seg(at.seg);
        if sh.shield < 400 && ahead.kind & CHARGE != 0 {
            tx = -half + FM * 2;
        }
        // Room to pass: steer round a ship close ahead. And no ramming:
        // a ship ahead in this one's own track (it would touch it as it
        // is now) may be closed on only gently, so this one slows to
        // its speed until the steering has found the room beside it. A
        // finished ship touches nobody ([`Race::bumps`]) and is let be.
        let mut behind = i32::MAX;
        for (j, o) in self.ships.iter().enumerate().take(usize::from(self.n)) {
            if j == i || o.fall > 0 || o.finished() {
                continue;
            }
            let ds = gap(sh.s, o.s, len);
            // Nothing to do with a ship behind or past the longest look
            // (and its speed is worked out only once).
            if ds <= 0 || ds >= FM * 16 + mps(30) * PASS_LOOK {
                continue;
            }
            let (ov, _) = o.v_track();
            if ds < FM * 24
                && (o.x - sh.x).abs() < BUMP_X + FM * 3 / 10
                && (o.h - sh.h).abs() < FM * 2
            {
                behind = behind.min(ov + (ds - BUMP_S - FM).max(0) / FOLLOW_CLOSE);
            }
            // Look for the room sooner the faster this one closes.
            let near = FM * 16 + (vs - ov).clamp(0, mps(30)) * PASS_LOOK;
            if ds < near && (o.x - tx).abs() < FM * 26 / 10 {
                tx = if o.x > sh.x {
                    o.x - FM * 3
                } else {
                    o.x + FM * 3
                };
                if tx.abs() > room {
                    tx = if o.x > 0 { o.x - FM * 3 } else { o.x + FM * 3 };
                }
            }
        }
        let tx = tx.clamp(-room, room);
        // The heading that leads to the target, and the turn of the road.
        let psi_want = (i64::from(tx - sh.x) * 10_430 / i64::from(look)) as i32;
        let psi_want = psi_want.clamp(-8_000, 8_000);
        let here = tr.seg(tr.index(sh.s));
        let dyaw = turn(here.yaw, tr.seg(tr.index(sh.s) + 1).yaw);
        let follow = ((i64::from(dyaw) * i64::from(vs)) >> SEG_BITS) as i32;
        // Steer the way the ship travels, not where its nose points: the
        // slip of a drift counts. At a crawl the slip is mostly a bump's
        // push, so it counts less there: two ships side by side must not
        // hold their noses into each other.
        // While carving and in the settle after a let-go the slip turns
        // into the nose's way at no cost, so the ship will travel where
        // the nose points: steer by the nose there (else it is caught
        // cutting into the inside wall as the slip goes).
        let slip = if sh.settle > 0 {
            sh.vl - sh.lean
        } else {
            sh.vl
        };
        let travel = sh.psi + (i64::from(slip) * 10_430 / i64::from(sh.vf.max(mps(30)))) as i32;
        let want_rate = if sh.carve == 0 {
            follow + (psi_want - travel) / 6
        } else {
            // Carving, the way the ship travels turns 1/CARVE_GRIP of the
            // slip towards the nose a tick: point the nose where that
            // brings the path round with the road and onto the aim.
            // The slip the nose needs is the road's turn where the path
            // will be once the slip has turned (CARVE_GRIP ticks on), so
            // the nose unwinds before the corner's end and the let-go
            // leaves little slip to settle into a wall.
            let k = tr.index(sh.s + vs * CARVE_GRIP);
            let ahead = ((i64::from(turn(tr.seg(k).yaw, tr.seg(k + 1).yaw)) * i64::from(vs))
                >> SEG_BITS) as i32;
            let aim = travel + (psi_want - travel) * CARVE_GRIP / CARVE_AIM + ahead * CARVE_GRIP;
            follow + (aim - sh.psi) / CARVE_NOSE
        };
        // Lead: the nose's turn rate moves only a quarter of the way to
        // the stick's each tick (`move_ship`), so ask four times the
        // change wanted, and one unit more past that step's rounding: the
        // rate lands on the wish in about a tick instead of lagging and
        // overshooting (the swing from side to side).
        let d = want_rate - sh.rate;
        let want_rate = sh.rate + d * 4 + d.signum();
        let steer = (want_rate * 256 / TURN_MAX).clamp(-256, 256);
        // Speed: what the coming turns allow, at the ship's pace and the
        // class's dare, and never faster into the ship ahead (above).
        let sk = self.skill;
        let pace = self.pace_of(i);
        let cap_at = |s: i32| i32::from(tr.seg(tr.index(s)).fast);
        let cap = cap_at(sh.s)
            .min(cap_at(sh.s + look))
            .min(cap_at(sh.s + look * 2));
        let mut cap = mps(cap) * pace / 1024 * sk.dare / 1024;
        if sh.carve != 0 {
            // Carving there is no grip to run out of, only the nose's
            // turn rate: as fast as the tightest carve still follows the
            // road at these points, with some to spare for the aim.
            let bend_at = |s: i32| {
                let k = tr.index(s);
                turn(tr.seg(k).yaw, tr.seg(k + 1).yaw).abs().max(1)
            };
            let b = bend_at(sh.s)
                .max(bend_at(sh.s + look))
                .max(bend_at(sh.s + look * 2));
            let rate = TURN_MAX * CARVE_TIGHT / 256 * sk.carve_rate / 1024 * pace / 1024;
            cap = cap.max(
                ((i64::from(rate) << SEG_BITS) / i64::from(b)).min(i64::from(i32::MAX)) as i32,
            );
        }
        let cap = cap.min(behind.max(mps(10)));
        let over = sh.vf - cap;
        // A sling stops pushing while an air brake is on: with one
        // running the pilot brakes only when further over.
        let margin = if sh.sling > 0 {
            sk.sling_brake
        } else {
            sk.brake
        };
        let both = over > cap / margin.max(1);
        Ctl {
            steer,
            thrust: over < 0,
            brake_l: both,
            brake_r: both,
            carve: Carve::Off,
        }
    }

    /// The player's ship after its finish: both air brakes while it is
    /// over three quarters of the class's top speed (a sling's speed
    /// would carry it into the next wall), then no thrust until it has
    /// slowed to half the top, then just enough to hold that (from
    /// [`CRUISE_JUMP`] segments before a jump, [`CRUISE_JUMP_SPEED`] and
    /// no brake, so it clears the gap); no carve;
    /// it eases back to the racing line along a long
    /// look (no lead on the stick and half the plain pilot's pull, so
    /// the nose settles instead of swinging from side to side).
    pub fn cruise(&self, i: usize) -> Ctl {
        let tr = self.tr();
        let sh = &self.ships[i];
        let (vs, _) = sh.v_track();
        let v = vs.max(mps(10));
        let look = FM * 20 + v * 40;
        let st = sh.s + look;
        let room = (tr.at(st).half * 256 - FM * 2).max(0);
        let tx = tr.line(st).clamp(-room, room);
        let psi_want = (i64::from(tx - sh.x) * 10_430 / i64::from(look)) as i32;
        let psi_want = psi_want.clamp(-4_000, 4_000);
        let here = tr.seg(tr.index(sh.s));
        let dyaw = turn(here.yaw, tr.seg(tr.index(sh.s) + 1).yaw);
        let follow = ((i64::from(dyaw) * i64::from(vs)) >> SEG_BITS) as i32;
        let travel = sh.psi + (i64::from(sh.vl) * 10_430 / i64::from(sh.vf.max(mps(30)))) as i32;
        let want_rate = follow + (psi_want - travel) / 12;
        let k = tr.index(sh.s);
        let jump = (1..=CRUISE_JUMP).any(|n| tr.seg(k + n).kind & GAP != 0);
        let fast = sh.vf > self.class.top() * 3 / 4 && !jump;
        Ctl {
            steer: (want_rate * 256 / TURN_MAX).clamp(-256, 256),
            thrust: sh.vf < self.class.top() / 2 || (jump && sh.vf < CRUISE_JUMP_SPEED),
            brake_l: fast,
            brake_r: fast,
            carve: Carve::Off,
        }
    }
}

/// How far before a jump (segments of 8 m) a finished player's ship
/// speeds up again ([`Race::cruise`]).
pub const CRUISE_JUMP: usize = 30;
/// The speed a finished player's ship takes into a jump (the class's top
/// speed if that is lower; STEADY's ships fell at 3/4 of their top).
pub const CRUISE_JUMP_SPEED: i32 = mps(60);

impl Default for Race {
    fn default() -> Self {
        Race::new()
    }
}

/// How far `b` is ahead of `a` on a lap of `len` (the short way round).
#[inline(never)]
fn gap(a: i32, b: i32, len: i32) -> i32 {
    let ds = b - a;
    if ds > len / 2 {
        ds - len
    } else if ds < -len / 2 {
        ds + len
    } else {
        ds
    }
}

/// Keeps `s` inside the lap and counts the line crossings.
#[inline(never)]
fn wrap(sh: &mut Ship, len: i32) {
    if sh.s >= len {
        sh.s -= len;
        // A finished ship goes on round, but its laps stop counting.
        if !sh.finished() {
            sh.lap = sh.lap.saturating_add(1);
        }
    } else if sh.s < 0 {
        sh.s += len;
        if !sh.finished() {
            sh.lap = sh.lap.saturating_sub(1);
        }
    }
}

/// Puts a fallen ship back on the road: on the far side of a gap, or
/// where it fell off an open edge, slowly, and passing through others
/// for a moment.
fn respawn(sh: &mut Ship, tr: &Track) {
    let len = tr.len();
    // Forward to the first segment with road after any gap.
    let mut i = tr.index(sh.s);
    let mut s = sh.s;
    let mut moved = false;
    for _ in 0..16 {
        if tr.seg(i).kind & GAP == 0 {
            break;
        }
        i += 1;
        moved = true;
    }
    if moved {
        let to = ((i % tr.n()) as i32) * (1 << SEG_BITS) + FM * 4;
        if to < s {
            // Over the line on the way.
            sh.lap = sh.lap.saturating_add(1);
        }
        s = to;
    }
    if s >= len {
        s -= len;
        sh.lap = sh.lap.saturating_add(1);
    }
    if sh.finished() {
        sh.lap = sh.lap.min(LAPS + 1);
    }
    sh.s = s;
    sh.x = tr.line(s);
    sh.h = HOVER + FM;
    sh.vh = 0;
    sh.vf = sh.vf.max(0) / 2;
    sh.vl = 0;
    sh.psi = 0;
    sh.rate = 0;
    sh.fall = 0;
    sh.ghost = 90;
    sh.boost = 0;
    sh.lose_carve();
    sh.sling = 0;
    sh.sling_step = 0;
    sh.pitch = tr.at(s).pitch;
}
