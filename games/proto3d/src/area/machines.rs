//! MACHINES (AREAS-SPEC.md, section 3): weight. The hero counts 1, the
//! bob (a 0.5 m brass cube) counts 2, and carrying it makes the hero
//! heavy: a slower run and a jump of 0.56 m instead of 1.21 m.
//!
//! The route: pick the bob up (X), stand on the plate with it (three
//! pips for half a second latch the plate and the bridge rises), carry it
//! over the bridge and up the plank, which tips under the load, set it
//! down on the plank's left half (X), walk up to the raised right end
//! light and jump to the ledge D. Landing on D is the end ("done").
//!
//! Every machine is a block record (the movers at the end of the list),
//! rebuilt each step from the small state in [`Machines`]:
//!
//! * the plate sinks 3 cm while loaded and shows the load in pips;
//! * the bridge is switched off ([`flag::HIDE`]) until the latch, then
//!   rises 5 cm a step;
//! * the plank is an ordinary ramp record with two positions, its ends
//!   at 25 cm and 125 cm, swinging 20 cm a step towards the heavier half
//!   when that half carries at least the bob's weight (2): the hero alone
//!   never tips it;
//! * the bob is a box record where it rests, switched off while carried
//!   (then it is drawn as an extra model in front of the hero).
//!
//! Units: records in cm, rules in subs (1/4096 m), time in steps (1/30 s).

use game_3d::{ONE14, cos_fine, sin_fine};
use game_kit::button;

use crate::area::{Extra, Hud};
use crate::hero::{Body, Hero, HeroEvents};
use crate::island::{Block, Prop, World, body_fits, flag, mat, prop, ramp, top_under};
use crate::maths::{cm, fnv, len2};
use crate::models;
use crate::play::{MAX_LIVE, Play};
use crate::step::Pad;
use crate::tune::{HALF, HEAVY_JUMP, HEAVY_RUN, HERO_H, JUMP, M, RESPAWN_STEPS, RUN, STEP_UP};

// ---- Block numbers ------------------------------------------------------------

/// The start pad.
pub const A: u8 = 0;
/// The machine yard.
pub const C: u8 = 1;
/// The ledge with the goal.
pub const D: u8 = 2;
/// The plank's pivot (not walkable; under the plank).
pub const PIVOT: u8 = 3;
/// The movers.
pub const PLATE: u8 = 4;
pub const BRIDGE: u8 = 5;
pub const PLANK: u8 = 6;
pub const BOB: u8 = 7;

// ---- Numbers (cm unless said) ---------------------------------------------------

/// The hero's start (feet).
pub const START: [i16; 3] = [50, 0, -320];
/// The bob's start: the middle of its bottom.
pub const BOB_START: [i16; 3] = [-150, 0, -250];
/// Half the bob's side.
pub const BOB_HALF: i16 = 25;
/// The bob's side.
pub const BOB_SIDE: i16 = 50;
/// The weights: the hero, the bob, the hero carrying the bob.
pub const W_HERO: u8 = 1;
pub const W_BOB: u8 = 2;
pub const W_CARRY: u8 = 3;
/// Pick-up reach: the most horizontal distance between the hero's and
/// the bob's middles (subs, 0.9 m).
pub const REACH: i32 = 3_686;
/// The bob's top may be this far below the feet, or above them (subs).
pub const PICK_BELOW: i32 = cm(30);
pub const PICK_ABOVE: i32 = cm(80);
/// Set-down spot: this far in front of the hero's middle (subs, 0.75 m).
pub const PLACE_AHEAD: i32 = 3_072;
/// The set-down top may be at most this far below the feet (subs; at
/// most [`STEP_UP`] above them).
pub const PLACE_BELOW: i32 = cm(30);
/// The room the bob needs above the top starts 2 cm up.
pub const PLACE_CLEAR: i16 = 2;
/// Steps `NO ROOM` stays on the screen.
pub const NO_ROOM_STEPS: u8 = 20;
/// The carried bob: its bottom this far over the feet and its middle
/// this far in front of the hero's middle. Held over the head (the
/// hero is 1.26 m tall), not at chest height as the spec says: at chest
/// height the follow camera saw only its edges round the hero, and it
/// hid the set-down marker (see the report).
pub const CARRY_UP: i16 = 130;
pub const CARRY_AHEAD: i16 = 10;
/// The plate: x 1.6..2.6 m, z -1.4..-0.4 m, top 6 cm, 3 cm while loaded.
pub const PLATE_UP: i16 = 6;
pub const PLATE_DOWN: i16 = 3;
/// Steps in a row with three pips lit that latch the plate.
pub const LATCH: u8 = 15;
/// The bridge: top from -100 to 0, 5 cm a step, 60 cm thick.
pub const BRIDGE_LOW: i16 = -100;
pub const BRIDGE_RISE: i16 = 5;
pub const BRIDGE_THICK: i16 = 60;
/// The plank's end heights and its swing a step.
pub const END_LOW: i16 = 25;
pub const END_HIGH: i16 = 125;
pub const SWING: i16 = 20;
/// The pivot's top: under the plank's slab everywhere (see the report:
/// the spec's 75 cm pokes through the plank's top).
pub const PIVOT_TOP: i16 = 60;
/// Falling this far under the safe spot is falling off (subs).
pub const FALL_ROOM: i32 = 6 * M;
/// D's top, and where its beacon stands. 2.45 m: from the plank's raised
/// right end (1.25 m) a full jump reaches it; from the bob resting on the
/// plank's low end (0.75 m) it does not (2.20 m did, by a centimetre).
pub const D_TOP: i16 = 245;
pub const BEACON_AT: [i16; 3] = [430, D_TOP, 550];

// ---- The records ---------------------------------------------------------------

const WR: u8 = flag::WALK | flag::RIM;

/// The plate with its top at `top`.
pub const fn plate_rec(top: i16) -> Block {
    Block::flat(
        [160, 0, -140],
        [260, top, -40],
        mat::BRASS,
        mat::BRASS,
        flag::WALK,
    )
}

/// The bridge with its top at `h`, switched off until `shown`.
pub const fn bridge_rec(h: i16, shown: bool) -> Block {
    let hide = if shown { 0 } else { flag::HIDE };
    Block::flat(
        [-70, h - BRIDGE_THICK, 0],
        [70, h, 300],
        mat::SLATE,
        mat::SLATE,
        WR | hide,
    )
}

/// The plank with its right end at `right` (the left end at 150 -
/// `right`): a ramp drawn as a 12 cm slab, solid down to the lower end.
pub const fn plank_rec(right: i16) -> Block {
    let left = END_LOW + END_HIGH - right;
    let (lo, hi, rise) = if right > left {
        (left, right, ramp::UP_X)
    } else {
        (right, left, ramp::DOWN_X)
    };
    Block {
        lo: [-250, lo, 490],
        hi: [250, hi, 610],
        top: mat::BLOCK_TOP,
        side: mat::TRUNK,
        flags: flag::WALK | flag::SLAB,
        ramp: rise,
    }
}

/// The bob resting with the middle of its bottom at `at`, or switched
/// off while carried.
pub const fn bob_rec(at: [i16; 3], carried: bool) -> Block {
    let hide = if carried { flag::HIDE } else { 0 };
    Block::flat(
        [at[0] - BOB_HALF, at[1], at[2] - BOB_HALF],
        [at[0] + BOB_HALF, at[1] + BOB_SIDE, at[2] + BOB_HALF],
        mat::BRASS,
        mat::BRASS,
        WR | hide,
    )
}

/// The area's blocks: static ones first, then the movers' starting
/// records (the same as [`Machines::new`] writes).
static BLOCKS: [Block; 8] = [
    // A: the start pad.
    Block::flat([-300, -60, -400], [300, 0, 0], mat::GRASS, mat::DIRT, WR),
    // C: the machine yard.
    Block::flat([-350, -60, 300], [350, 0, 800], mat::GRASS, mat::DIRT, WR),
    // D: the ledge, top 2.45 m. Its sides start on C's top (y 0), so
    // they never reach down into C (a raised block's buried faces would
    // be painted over the floor's top).
    Block::flat(
        [310, 0, 450],
        [550, D_TOP, 650],
        mat::STONE,
        mat::ROCK,
        WR | flag::FADE,
    ),
    // The pivot under the plank.
    Block::flat(
        [-20, 0, 520],
        [20, PIVOT_TOP, 580],
        mat::BRASS,
        mat::BRASS,
        0,
    ),
    plate_rec(PLATE_UP),
    bridge_rec(BRIDGE_LOW, false),
    plank_rec(END_LOW),
    bob_rec(BOB_START, false),
];

/// Rock points under A and C (the islet's cone fits both: its ring's
/// corners land 2 m out on the diagonals), and grass tufts (two at MID,
/// four at HIGH) away from the route.
static PROPS: [Prop; 6] = [
    Prop::new(prop::CONE_ISLET, 0, 32, [0, -60, -200]),
    Prop::new(prop::CONE_ISLET, 0, 32, [0, -60, 550]),
    Prop::new(prop::TUFTS, 1, 0, [-180, 0, -100]),
    Prop::new(prop::TUFTS, 1, 0, [-220, 0, 700]),
    Prop::new(prop::TUFTS, 2, 0, [200, 0, -300]),
    Prop::new(prop::TUFTS, 2, 0, [150, 0, 380]),
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
pub const MOVERS: usize = 4;

const _: () = assert!(BLOCKS.len() <= MAX_LIVE);
const _: () = assert!(core::mem::size_of::<Machines>() <= 32);

// ---- Small helpers ---------------------------------------------------------------

/// Subs to cm, rounded to the nearest.
pub fn to_cm(s: i32) -> i16 {
    ((i64::from(s) * 25 + 512).div_euclid(1024)) as i16
}

/// The hero's forward direction (x, z) in 1/16384.
pub fn forward(yaw: u16) -> [i32; 2] {
    [sin_fine(yaw), cos_fine(yaw)]
}

/// Whether a box `lo..hi` (subs) overlaps a block (strictly; a ramp by
/// its highest point over the overlap).
fn box_hits(b: &Block, lo: [i32; 3], hi: [i32; 3]) -> bool {
    if !b.solid() {
        return false;
    }
    let bb = b.bounds();
    let (x0, x1) = (lo[0].max(bb.lo[0]), hi[0].min(bb.hi[0]));
    let (z0, z1) = (lo[2].max(bb.lo[2]), hi[2].min(bb.hi[2]));
    if x0 >= x1 || z0 >= z1 {
        return false;
    }
    let top = [(x0, z0), (x1, z0), (x0, z1), (x1, z1)]
        .iter()
        .map(|&(x, z)| b.surface(x, z))
        .max()
        .unwrap_or(bb.hi[1]);
    lo[1] < top && hi[1] > bb.lo[1]
}

/// Whether a box `lo..hi` (subs) overlaps the hero's body.
fn hits_hero(h: &Hero, lo: [i32; 3], hi: [i32; 3]) -> bool {
    h.frozen == 0
        && h.pos[0] - HALF < hi[0]
        && h.pos[0] + HALF > lo[0]
        && h.pos[2] - HALF < hi[2]
        && h.pos[2] + HALF > lo[2]
        && h.pos[1] < hi[1]
        && h.pos[1] + HERO_H > lo[1]
}

/// The bob's box (subs) with the middle of its bottom at `at` (cm),
/// from `up` cm over the bottom to its top + `up`.
fn bob_box(at: [i16; 3], up: i16) -> ([i32; 3], [i32; 3]) {
    (
        [cm(at[0] - BOB_HALF), cm(at[1] + up), cm(at[2] - BOB_HALF)],
        [
            cm(at[0] + BOB_HALF),
            cm(at[1] + up + BOB_SIDE),
            cm(at[2] + BOB_HALF),
        ],
    )
}

/// The loads on the plate and on the plank's two halves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Loads {
    pub plate: u8,
    pub left: u8,
    pub right: u8,
}

impl Loads {
    /// Adds `w` on block `on`, at x (subs) for the plank's halves.
    fn add(&mut self, on: u8, x: i32, w: u8) {
        match on {
            PLATE => self.plate += w,
            PLANK if x < 0 => self.left += w,
            PLANK => self.right += w,
            _ => {}
        }
    }
}

/// Where the bob would go if set down now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    /// The middle of the bob's bottom (cm): x, the top's height, z.
    pub at: [i16; 3],
    /// The block it would rest on.
    pub on: Option<u8>,
    /// It may be set down there.
    pub ok: bool,
    /// The marker's middle (subs): on the highest top under the
    /// footprint, 1 cm up.
    pub mark: [i32; 3],
}

// ---- The state -------------------------------------------------------------------

/// The area's state (at most 32 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Machines {
    /// Steps since the start (the clock).
    pub steps: u16,
    /// The hero carries the bob.
    pub carry: bool,
    /// Where the bob rests: the middle of its bottom (cm), and the block
    /// under it.
    pub bob: [i16; 3],
    pub rest: u8,
    /// The plate's load in the last step.
    pub plate_load: u8,
    /// Steps in a row with three pips lit.
    pub held3: u8,
    /// The plate is latched: the bridge is on.
    pub latched: bool,
    /// The bridge's top (cm).
    pub bridge: i16,
    /// The plank's goal: left end down (else right end down).
    pub left_down: bool,
    /// The plank's right end (cm); the left end is 150 - this.
    pub right: i16,
    /// Steps `NO ROOM` is still shown.
    pub no_room: u8,
}

impl Default for Machines {
    fn default() -> Machines {
        Machines::new()
    }
}

impl Machines {
    pub const fn new() -> Machines {
        Machines {
            steps: 0,
            carry: false,
            bob: BOB_START,
            rest: A,
            plate_load: 0,
            held3: 0,
            latched: false,
            bridge: BRIDGE_LOW,
            left_down: false,
            right: END_LOW,
            no_room: 0,
        }
    }

    /// The plank's left end (cm).
    pub fn left(&self) -> i16 {
        END_LOW + END_HIGH - self.right
    }

    /// Mover `i`'s record for this state.
    pub fn mover(&self, i: u8) -> Block {
        match i {
            PLATE => plate_rec(if self.plate_load > 0 {
                PLATE_DOWN
            } else {
                PLATE_UP
            }),
            BRIDGE => bridge_rec(self.bridge, self.latched),
            PLANK => plank_rec(self.right),
            _ => bob_rec(self.bob, self.carry),
        }
    }

    /// Writes the movers' records for now ([`MOVERS`] of them).
    pub fn movers(&self, out: &mut [Block]) {
        for (k, o) in out.iter_mut().enumerate().take(MOVERS) {
            *o = self.mover(PLATE + k as u8);
        }
    }

    /// The hero's tuning this step: heavy while carrying; falling 6 m
    /// under the safe spot is falling off.
    pub fn body(&self, hero: &Hero) -> Body {
        Body {
            run: if self.carry { HEAVY_RUN } else { RUN },
            jump: if self.carry { HEAVY_JUMP } else { JUMP },
            glide: false,
            kill_y: hero.safe[1] - FALL_ROOM,
            respawn: RESPAWN_STEPS,
        }
    }

    /// The loads the hero (after moving) and the bob put on the plate and
    /// the plank's halves.
    pub fn loads(&self, hero: &Hero) -> Loads {
        let mut l = Loads::default();
        let hw = if self.carry { W_CARRY } else { W_HERO };
        let bob_x = cm(self.bob[0]);
        if hero.frozen == 0 {
            match hero.ground {
                Some(BOB) if !self.carry => l.add(self.rest, bob_x, hw),
                Some(g) => l.add(g, hero.pos[0], hw),
                None => {}
            }
        }
        if !self.carry {
            l.add(self.rest, bob_x, W_BOB);
        }
        l
    }

    /// Whether X now picks the bob up: standing (not on the bob), not
    /// carrying, the middles at most [`REACH`] apart and the bob's top
    /// from 0.3 m under the feet to 0.8 m over them.
    pub fn can_pick(&self, hero: &Hero) -> bool {
        if self.carry || hero.ground.is_none() || hero.ground == Some(BOB) || hero.frozen > 0 {
            return false;
        }
        let d = len2(hero.pos[0] - cm(self.bob[0]), hero.pos[2] - cm(self.bob[2]));
        let up = cm(self.bob[1] + BOB_SIDE) - hero.pos[1];
        d <= REACH && (-PICK_BELOW..=PICK_ABOVE).contains(&up)
    }

    /// Where the bob would be set down now, and whether it may: 0.75 m
    /// in front of the hero, a walkable top under its middle and its four
    /// corners (the same block), that top at most 0.3 m under the feet and
    /// at most [`STEP_UP`] over them, and room for the bob from 2 cm over
    /// the top (no solid block but its support, not the hero).
    pub fn spot(&self, hero: &Hero, blocks: &[Block]) -> Spot {
        let f = forward(hero.yaw);
        let mx = to_cm(hero.pos[0] + f[0] * PLACE_AHEAD / ONE14);
        let mz = to_cm(hero.pos[2] + f[1] * PLACE_AHEAD / ONE14);
        let feet = hero.pos[1];
        let reach = feet + STEP_UP;
        let mut on: Option<u8> = None;
        let mut same = true;
        let mut mark = feet;
        let mut any = false;
        let pts = [
            (0, 0),
            (-BOB_HALF, -BOB_HALF),
            (BOB_HALF, -BOB_HALF),
            (-BOB_HALF, BOB_HALF),
            (BOB_HALF, BOB_HALF),
        ];
        for (k, (dx, dz)) in pts.iter().enumerate() {
            let t = top_under(blocks, cm(mx + dx), cm(mz + dz), reach);
            if let Some((_, s)) = t {
                mark = if any { mark.max(s) } else { s };
                any = true;
            }
            let i = t.map(|(i, _)| i);
            if k == 0 {
                on = i;
            } else if i != on {
                same = false;
            }
        }
        let mid = cm(mx);
        let midz = cm(mz);
        let (top, ok) = match on {
            Some(i) if same => {
                let s = blocks[usize::from(i)].surface(mid, midz);
                let y = to_cm(s);
                let (lo, hi) = bob_box([mx, y, mz], PLACE_CLEAR);
                let room = blocks
                    .iter()
                    .enumerate()
                    .all(|(j, b)| j == usize::from(i) || !box_hits(b, lo, hi))
                    && !hits_hero(hero, lo, hi);
                (y, feet - s <= PLACE_BELOW && room)
            }
            _ => (to_cm(feet), false),
        };
        Spot {
            at: [mx, top, mz],
            on,
            ok,
            mark: [mid, mark + cm(1), midz],
        }
    }

    /// Button actions before the hero moves: X picks the bob up or sets
    /// it down (standing only).
    pub fn before(&mut self, pad: &Pad, hero: &Hero, blocks: &[Block]) {
        self.no_room = self.no_room.saturating_sub(1);
        if pad.pressed & button::X == 0 || hero.ground.is_none() || hero.frozen > 0 {
            return;
        }
        if self.carry {
            let s = self.spot(hero, blocks);
            match s.on {
                Some(on) if s.ok => {
                    self.carry = false;
                    self.bob = s.at;
                    self.rest = on;
                }
                _ => self.no_room = NO_ROOM_STEPS,
            }
        } else if self.can_pick(hero) {
            self.carry = true;
        }
    }

    /// Whether the plank can move its right end to `next`: a hero riding
    /// it (on the plank, or on the bob resting on it) and a bob resting
    /// on it still fit.
    pub fn swing_fits(&self, hero: &Hero, blocks: &[Block], next: i16) -> bool {
        let n = blocks.len().min(MAX_LIVE);
        let mut cand = [Block::flat([0; 3], [0; 3], 0, 0, flag::HIDE); MAX_LIVE];
        cand[..n].copy_from_slice(&blocks[..n]);
        let plank = plank_rec(next);
        let bob_rides = !self.carry && self.rest == PLANK;
        let mut bob = self.bob;
        if bob_rides {
            bob[1] = to_cm(plank.surface(cm(bob[0]), cm(bob[2])));
        }
        let (pi, bi) = (usize::from(PLANK), usize::from(BOB));
        if pi < n {
            cand[pi] = plank;
        }
        if bi < n {
            cand[bi] = bob_rec(bob, self.carry);
        }
        let cand = &cand[..n];
        let on_bob = hero.ground == Some(BOB) && bob_rides;
        let rides = hero.ground == Some(PLANK) || on_bob;
        if hero.frozen == 0
            && rides
            && let Some(g) = hero.ground.and_then(|g| cand.get(usize::from(g)))
        {
            let feet = g.surface(hero.pos[0], hero.pos[2]);
            if !body_fits(cand, [hero.pos[0], feet, hero.pos[2]]) {
                return false;
            }
        }
        // Nor may the plank swing into a hero or a bob beside it (under
        // its lifted end in the middle of a swing).
        let alone = core::slice::from_ref(&plank);
        if hero.frozen == 0 && !rides && !body_fits(alone, hero.pos) {
            return false;
        }
        if !self.carry && !bob_rides {
            let (lo, hi) = bob_box(bob, 0);
            if box_hits(&plank, lo, hi) {
                return false;
            }
        }
        if bob_rides {
            let (lo, hi) = bob_box(bob, 0);
            let others = cand
                .iter()
                .enumerate()
                .all(|(j, b)| j == pi || j == bi || !box_hits(b, lo, hi));
            if !others || (!on_bob && hits_hero(hero, lo, hi)) {
                return false;
            }
        }
        true
    }

    /// After the hero moved: a respawn on a moved top, the loads, the
    /// plate and its latch, the bridge, the plank, a bob riding a mover,
    /// the clock. True when the hero landed on D.
    pub fn after(&mut self, hero: &mut Hero, ev: &HeroEvents, blocks: &[Block]) -> bool {
        self.steps = self.steps.saturating_add(1);
        if ev.respawned {
            put_back(hero, blocks);
        }
        let l = self.loads(hero);

        // The plate and its latch.
        self.plate_load = l.plate;
        let was_latched = self.latched;
        if !self.latched {
            if l.plate >= 3 {
                self.held3 = self.held3.saturating_add(1);
                self.latched = self.held3 >= LATCH;
            } else {
                self.held3 = 0;
            }
        }

        // The bridge rises from the step after the latch.
        if was_latched && self.bridge < 0 {
            let old = cm(self.bridge);
            self.bridge = (self.bridge + BRIDGE_RISE).min(0);
            let new = cm(self.bridge);
            let b = bridge_rec(self.bridge, true).bounds();
            // A hero the top rose past by at most 5 cm is lifted onto it.
            if hero.frozen == 0
                && hero.ground != Some(BRIDGE)
                && b.overlaps_square(hero.pos[0], hero.pos[2], HALF)
                && hero.pos[1] >= old
                && hero.pos[1] < new
            {
                hero.pos[1] = new;
                hero.vel[1] = hero.vel[1].max(0);
                hero.ground = Some(BRIDGE);
            }
        }

        // The plank: towards the heavier half, held if a rider would hit.
        // It tips only under at least the bob's weight on the heavier
        // half: the hero alone (1) never tips it.
        if l.left > l.right && l.left >= W_BOB {
            self.left_down = true;
        } else if l.right > l.left && l.right >= W_BOB {
            self.left_down = false;
        }
        let goal = if self.left_down { END_HIGH } else { END_LOW };
        if self.right != goal {
            let next = if goal > self.right {
                (self.right + SWING).min(goal)
            } else {
                (self.right - SWING).max(goal)
            };
            if self.swing_fits(hero, blocks, next) {
                self.right = next;
            }
        }

        // A bob resting on a mover rides it.
        if !self.carry && self.rest >= PLATE && self.rest != BOB {
            let m = self.mover(self.rest);
            self.bob[1] = to_cm(m.surface(cm(self.bob[0]), cm(self.bob[2])));
        }

        ev.landed == Some(D) && hero.frozen == 0
    }

    /// The HUD: `CARRY` (else the area's name), the plate's pips or
    /// `OPEN`, `NO ROOM`, and `GOAL` over the beacon.
    pub fn hud(&self, out: &mut Hud) {
        out.text[0].push(if self.carry { "CARRY" } else { "MACHINES" });
        if self.latched {
            out.text[1].push("OPEN");
        } else if self.plate_load > 0 {
            out.pips = 3;
            out.lit = self.plate_load.min(3);
        }
        if self.no_room > 0 {
            out.msg = Some("NO ROOM");
        }
        let b = BEACON_AT;
        out.label = Some(([cm(b[0]), cm(b[1] + 165), cm(b[2])], "GOAL"));
    }

    /// The beacon on D; while carrying, the bob in front of the hero and
    /// (standing) the set-down marker.
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
        if self.carry && h.frozen == 0 {
            let f = forward(h.yaw);
            let ahead = cm(CARRY_AHEAD);
            push(Extra {
                model: &models::BOB,
                pos: [
                    h.pos[0] + f[0] * ahead / ONE14,
                    h.pos[1] + cm(CARRY_UP),
                    h.pos[2] + f[1] * ahead / ONE14,
                ],
                yaw: h.yaw,
                scale: 256,
            });
            if h.ground.is_some() {
                let s = self.spot(h, play.blocks());
                push(Extra {
                    model: if s.ok {
                        &models::MARKER_OK
                    } else {
                        &models::MARKER_NO
                    },
                    pos: s.mark,
                    yaw: 0,
                    scale: 256,
                });
            }
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
            i64::from(self.steps),
            i64::from(self.carry),
            i64::from(self.bob[0]),
            i64::from(self.bob[1]),
            i64::from(self.bob[2]),
            i64::from(self.rest),
            i64::from(self.plate_load),
            i64::from(self.held3),
            i64::from(self.latched),
            i64::from(self.bridge),
            i64::from(self.left_down),
            i64::from(self.right),
            i64::from(self.no_room),
        ];
        v.iter().fold(f, |h, &x| fnv(h, x))
    }
}

/// After a respawn on a mover: the feet on its top as it is now, or, if
/// it is switched off (the bob, now carried), on the top under them.
fn put_back(hero: &mut Hero, blocks: &[Block]) {
    let Some(g) = hero.ground else { return };
    if g < PLATE {
        return;
    }
    let (x, z) = (hero.pos[0], hero.pos[2]);
    match blocks.get(usize::from(g)) {
        Some(b) if b.solid() => hero.pos[1] = b.surface(x, z),
        _ => match top_under(blocks, x, z, hero.pos[1]) {
            Some((t, s)) => {
                hero.pos[1] = s;
                hero.ground = Some(t);
            }
            None => hero.ground = None,
        },
    }
}
