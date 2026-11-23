//! SAIL (AREAS-SPEC.md, section 4): the glide, a clock and a best time.
//!
//! Hold B while falling in the air and the hero glides: the fall is
//! held to 1.5 m/s (`GLIDE_FALL`), the run in the air goes up to
//! 5.5 m/s (`GLIDE_RUN`), for at most 2 s (`GLIDE_STEPS`) per take-off,
//! refilled on any top. The glide itself is the frame's hero hook
//! ([`crate::hero::Hero::step_with`] with [`Body::glide`]); this file
//! gives the course, the clock, the instant retry and the look.
//!
//! The course: five pads over the void, each gap longer than the one
//! before; a jump alone crosses none of them.
//!
//! * gap 1, P0 to P1: 4.5 m, level;
//! * gap 2, P1 to P2: 6.5 m, 0.6 m down, P2 starts 1 m to the right;
//! * on P2 two steps (S1, S2), each 0.5 m up, after a turn of 90°;
//! * gap 3, S2 to P3: 8.5 m, 1.0 m down, a 2 m run-up on S2;
//! * gap 4, P3 to P4: 10.5 m, 1.5 m down, onto a 2.5 m goal pad.
//!
//! The clock starts in the first step the hero's middle is off P0's
//! footprint and stops on the landing on P4 ("done"). A fall puts the
//! hero back on the last safe ground after 4 steps; the clock runs on.
//!
//! Units: records in cm, rules in subs (1/4096 m), time in steps (1/30 s).

use game_3d::{ONE14, cos_fine, sin_fine};

use crate::area::{Extra, Hud};
use crate::draw::time_text;
use crate::hero::{Body, Hero, HeroEvents};
use crate::island::{Block, Prop, World, flag, mat, prop};
use crate::maths::{cm, fnv};
use crate::models;
use crate::play::{MAX_LIVE, Play};
use crate::step::Pad;
use crate::tune::{GLIDE_STEPS, JUMP, M, RUN};

// ---- Block numbers ------------------------------------------------------------

/// The start pad.
pub const P0: u8 = 0;
/// The first landing.
pub const P1: u8 = 1;
/// The turn pad, with its two steps.
pub const P2: u8 = 2;
pub const S1: u8 = 3;
pub const S2: u8 = 4;
/// The pad before the last gap.
pub const P3: u8 = 5;
/// The goal pad.
pub const P4: u8 = 6;

// ---- Numbers (cm unless said) -------------------------------------------------

/// The hero's start (feet), facing +z.
pub const START: [i16; 3] = [0, 0, -500];
/// P0's footprint (x and z, cm): the clock starts when the hero's
/// middle leaves it.
pub const P0_LO: [i16; 2] = [-200, -600];
pub const P0_HI: [i16; 2] = [200, 0];
/// Falling this far under the safe spot is falling off (subs, 4 m).
pub const FALL_ROOM: i32 = 4 * M;
/// Steps frozen after a fall before the hero is put back.
pub const RESPAWN: u8 = 4;
/// The goal's beacon: on P4's middle.
pub const BEACON_AT: [i16; 3] = [3475, -210, 1900];
/// The longest the sail bar is (px): `GLIDE_STEPS / 2`.
pub const BAR_MAX: u8 = GLIDE_STEPS / 2;

// ---- The records ----------------------------------------------------------------

const WR: u8 = flag::WALK | flag::RIM;

/// The area's blocks (no movers). Every pad is 0.6 m deep. The steps
/// stand on P2 as a stair: S1 from P2's top (-0.6 m) up to -0.1 m, under
/// S2 too, and S2 from S1's top up to +0.4 m, so that every side of a
/// step starts on the top under it (see the report: the spec's
/// `lo = top - 0.6 m` would have the steps' sides run into P2). The
/// tops one can walk on are the spec's: P2 from x 2.5 to 5 m, S1 from 5
/// to 6.5 m, S2 from 6.5 to 8.5 m. S1 is drawn with the raised blocks
/// ([`flag::UP`]) so that it is drawn after P2.
static BLOCKS: [Block; 7] = [
    // P0: the start pad.
    Block::flat([-200, -60, -600], [200, 0, 0], mat::GRASS, mat::DIRT, WR),
    // P1: 4.5 m on.
    Block::flat([-150, -60, 450], [150, 0, 1050], mat::GRASS, mat::DIRT, WR),
    // P2: 6.5 m on, 0.6 m down, 1 m to the right.
    Block::flat(
        [250, -120, 1700],
        [850, -60, 2100],
        mat::GRASS,
        mat::DIRT,
        WR,
    ),
    // S1 and S2: the steps.
    Block::flat(
        [500, -60, 1700],
        [850, -10, 2100],
        mat::STONE,
        mat::ROCK,
        WR | flag::UP,
    ),
    Block::flat([650, -10, 1700], [850, 40, 2100], mat::STONE, mat::ROCK, WR),
    // P3: 8.5 m past S2, 1.0 m under it.
    Block::flat(
        [1700, -120, 1750],
        [2300, -60, 2050],
        mat::GRASS,
        mat::DIRT,
        WR,
    ),
    // P4: the goal pad, 10.5 m past P3 and 1.5 m under it.
    Block::flat(
        [3350, -270, 1775],
        [3600, -210, 2025],
        mat::STONE,
        mat::ROCK,
        WR,
    ),
];

/// Rock points under the four big pads (the islet's cone: its ring's
/// corners land 2 m out on the diagonals; P4 is too small for it).
static PROPS: [Prop; 4] = [
    Prop::new(prop::CONE_ISLET, 0, 32, [0, -60, -300]),
    Prop::new(prop::CONE_ISLET, 0, 32, [0, -60, 750]),
    Prop::new(prop::CONE_ISLET, 0, 32, [550, -120, 1900]),
    Prop::new(prop::CONE_ISLET, 0, 32, [2000, -120, 1900]),
];

/// The area's records.
pub static WORLD: World = World {
    blocks: &BLOCKS,
    props: &PROPS,
    coins: &[],
    coins_low: 0,
    start: START,
    critters: &[],
};

/// How many records at the end of [`WORLD`]'s blocks are movers.
pub const MOVERS: usize = 0;

const _: () = assert!(BLOCKS.len() <= MAX_LIVE);
const _: () = assert!(core::mem::size_of::<Sail>() <= 32);

/// Whether a point (subs, x and z) is over P0's footprint (edges
/// included).
pub fn on_p0(x: i32, z: i32) -> bool {
    (cm(P0_LO[0])..=cm(P0_HI[0])).contains(&x) && (cm(P0_LO[1])..=cm(P0_HI[1])).contains(&z)
}

/// The sail bar's length (px) for the glide left, or `None` while the
/// glide is full (the bar is hidden then).
pub fn bar_of(glide_left: u8) -> Option<u8> {
    (glide_left < GLIDE_STEPS).then_some(glide_left / 2)
}

// ---- The state ---------------------------------------------------------------------

/// The area's state (at most 32 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sail {
    /// The clock runs (the hero has left P0).
    pub started: bool,
    /// The clock (steps since it started).
    pub steps: u16,
    /// The hero's glide left and whether he glided, after the last step
    /// (for the HUD's sail bar).
    pub glide_left: u8,
    pub gliding: bool,
}

impl Default for Sail {
    fn default() -> Sail {
        Sail::new()
    }
}

impl Sail {
    pub const fn new() -> Sail {
        Sail {
            started: false,
            steps: 0,
            glide_left: GLIDE_STEPS,
            gliding: false,
        }
    }

    /// No movers.
    pub fn movers(&self, _out: &mut [Block]) {}

    /// The hero's tuning: the normal run and jump, the glide on, and the
    /// instant retry (4 m under the safe spot, back after 4 steps).
    pub fn body(&self, hero: &Hero) -> Body {
        Body {
            run: RUN,
            jump: JUMP,
            glide: true,
            kill_y: hero.safe[1] - FALL_ROOM,
            respawn: RESPAWN,
        }
    }

    /// No button actions (X does nothing in SAIL).
    pub fn before(&mut self, _pad: &Pad, _hero: &Hero, _blocks: &[Block]) {}

    /// After the hero moved: the clock (it starts when the hero's middle
    /// is off P0, then counts every step, frozen ones too). True when the
    /// hero landed on P4.
    pub fn after(&mut self, hero: &mut Hero, ev: &HeroEvents, _blocks: &[Block]) -> bool {
        if !self.started && !on_p0(hero.pos[0], hero.pos[2]) {
            self.started = true;
        }
        if self.started {
            self.steps = self.steps.saturating_add(1);
        }
        self.glide_left = hero.glide_left;
        self.gliding = hero.gliding;
        ev.landed == Some(P4) && hero.frozen == 0
    }

    /// The HUD: the name, the clock with the best time under it, the sail
    /// bar while the glide is not full, and `GOAL` over the beacon.
    pub fn hud(&self, out: &mut Hud) {
        out.text[0].push("SAIL");
        time_text(&mut out.text[1], self.steps);
        out.best_line = true;
        out.low_bar = bar_of(self.glide_left);
        let b = BEACON_AT;
        out.label = Some(([cm(b[0]), cm(b[1] + 165), cm(b[2])], "GOAL"));
    }

    /// The beacon on P4, and the sail on the gliding hero.
    pub fn extras(&self, play: &Play, out: &mut [Extra]) -> usize {
        let mut n = 0;
        let mut push = |e: Extra| {
            if let Some(o) = out.get_mut(n) {
                *o = e;
                n += 1;
            }
        };
        push(Extra {
            model: &models::BEACON,
            pos: BEACON_AT.map(cm),
            yaw: 0,
            scale: 256,
        });
        let h = &play.hero;
        if h.gliding && h.frozen == 0 {
            push(Extra {
                model: &models::SAIL,
                pos: sail_pos(h),
                yaw: h.yaw,
                scale: 256,
            });
        }
        n
    }

    /// The clock (steps).
    pub fn time(&self) -> Option<u16> {
        Some(self.steps)
    }

    /// The state for the fingerprint.
    pub fn hash(&self, f: u64) -> u64 {
        let v = [
            i64::from(self.started),
            i64::from(self.steps),
            i64::from(self.glide_left),
            i64::from(self.gliding),
        ];
        v.iter().fold(f, |h, &x| fnv(h, x))
    }
}

/// The sail's origin (subs): [`models::SAIL_AT`] in the hero's space,
/// turned with him.
pub fn sail_pos(h: &Hero) -> [i32; 3] {
    let (s, c) = (sin_fine(h.yaw), cos_fine(h.yaw));
    let [x, y, z] = models::SAIL_AT.map(|v| cm(v as i16));
    // Hero space to world: right (cos, -sin), forward (sin, cos).
    [
        h.pos[0] + (x * c + z * s) / ONE14,
        h.pos[1] + y,
        h.pos[2] + (-x * s + z * c) / ONE14,
    ]
}
