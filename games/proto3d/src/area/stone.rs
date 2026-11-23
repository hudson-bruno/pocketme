//! STONE (AREAS-SPEC.md, section 5): build your own step. The hero
//! carries one slate slab (1.0 x 1.0 m, 25 cm thick) and can:
//!
//! * **place** it with B: standing, 1 m ahead on a flat top (not on the
//!   cliff K: `NO ROOM`); in the air,
//!   right under the feet (once per take-off from real ground);
//! * **raise** it with B held while standing on it (after a fresh press
//!   of B on it): 4 cm a step, up to 2.5 m over where it was placed;
//! * **call it back** with X: it vanishes at once and is back in hand
//!   after 15 steps (0.5 s).
//!
//! The course: a 3 m cliff K over the start pad S (a raise climbs it),
//! a 5 m gap of open sky to the pad T (an air place bridges it), and the
//! goal G 4 m over T and 3 m past it (an air place and a raise reach
//! it). Standing on G is "done".
//!
//! The stone is a mover record (the last block), rebuilt each step from
//! the small state in [`Stone`]. Button presses are noted in `before`
//! and acted on in `after`, on the hero as he stands after his move:
//! the live blocks are rebuilt only after `after`, so a stone placed in
//! `before` would not be there in the same step's move anyway.
//!
//! Units: records in cm, rules in subs (1/4096 m), time in steps (1/30 s).

use game_3d::ONE14;
use game_kit::button;

use crate::area::machines::{forward, to_cm};
use crate::area::{Extra, Hud};
use crate::draw::time_text;
use crate::hero::{Body, Hero, HeroEvents};
use crate::island::{Block, Prop, World, body_fits, flag, mat, prop, top_under};
use crate::maths::{cm, fnv};
use crate::models;
use crate::play::{MAX_LIVE, Play};
use crate::step::Pad;
use crate::tune::{HALF, HERO_H, JUMP, M, RESPAWN_STEPS, RUN, STEP_UP};

// ---- Block numbers ------------------------------------------------------------

/// The start pad.
pub const S: u8 = 0;
/// The cliff, 3 m over S, its face at z 2 m.
pub const K: u8 = 1;
/// The pad past the gap.
pub const T: u8 = 2;
/// The goal.
pub const G: u8 = 3;
/// The stone (the one mover).
pub const STONE: u8 = 4;

// ---- Numbers (cm unless said) -------------------------------------------------

/// The hero's start (feet), facing +z.
pub const START: [i16; 3] = [0, 0, -300];
/// Half the stone's side, and its thickness.
pub const HALF_SIDE: i16 = 50;
pub const THICK: i16 = 25;
/// How far ahead of the hero's middle a ground place puts the stone's
/// middle (subs, 1.0 m).
pub const AHEAD: i32 = M;
/// The most the tops under a ground-placed stone may differ (2 cm).
pub const FLAT: i16 = 2;
/// Steps `NO ROOM` stays on the screen.
pub const NO_ROOM_STEPS: u8 = 20;
/// The raise: 4 cm a step, at most 250 cm over where it was placed.
pub const RAISE: i16 = 4;
pub const RAISE_MAX: i16 = 250;
/// Steps from a call back until the stone is in hand again.
pub const RETURN: u8 = 15;
/// Falling this far under the safe spot is falling off (subs, 6 m).
pub const FALL_ROOM: i32 = 6 * M;
/// Steps the button hint shows at the start.
pub const HINT_STEPS: u16 = 150;
/// The goal's beacon: on G's middle.
pub const BEACON_AT: [i16; 3] = [0, 700, 1725];
/// The outline drawn under the feet in the air: this far under them.
pub const OUTLINE_DOWN: i16 = 25;

// ---- The records ----------------------------------------------------------------

const WR: u8 = flag::WALK | flag::RIM;
/// The stone's flags while placed: FADE, so that it never pulls the
/// camera in and shrinks away when it hides the hero.
const STONE_FLAGS: u8 = WR | flag::FADE;

/// The area's blocks: S, K, T, G, then the stone's starting record
/// (switched off, under the start).
static BLOCKS: [Block; 5] = [
    // S: the start pad. It runs on under K (to z 4 m), so that K's
    // sides start on S's top and never reach down into it (a raised
    // block's buried faces would be painted over the floor's top).
    Block::flat([-300, -60, -400], [300, 0, 400], mat::GRASS, mat::DIRT, WR),
    // K: the cliff, 3 m over S, its face at z 2 m towards the start.
    Block::flat([-300, 0, 200], [300, 300, 400], mat::GRASS, mat::ROCK, WR),
    // T: 5 m of open sky past K, the same height, floating.
    Block::flat(
        [-250, 240, 900],
        [250, 300, 1300],
        mat::GRASS,
        mat::DIRT,
        WR,
    ),
    // G: the goal, 4 m over T and 3 m past it; FADE.
    Block::flat(
        [-125, 640, 1600],
        [125, 700, 1850],
        mat::STONE,
        mat::ROCK,
        WR | flag::FADE,
    ),
    // The stone, in hand: switched off.
    stone_record([0, 0, -300], false),
];

/// Rock points under S (with K) and under T (G is too small for one).
static PROPS: [Prop; 2] = [
    Prop::new(prop::CONE_ISLET, 0, 32, [0, -60, -100]),
    Prop::new(prop::CONE_ISLET, 0, 32, [0, 240, 1100]),
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
pub const MOVERS: usize = 1;

const _: () = assert!(BLOCKS.len() <= MAX_LIVE);
const _: () = assert!(core::mem::size_of::<Stone>() <= 32);

/// The stone's record with its top middle at `at` (cm: x, top, z).
pub const fn stone_record(at: [i16; 3], placed: bool) -> Block {
    let [x, y, z] = at;
    Block::flat(
        [x - HALF_SIDE, y - THICK, z - HALF_SIDE],
        [x + HALF_SIDE, y, z + HALF_SIDE],
        mat::SLATE,
        mat::BRASS,
        if placed {
            STONE_FLAGS
        } else {
            STONE_FLAGS | flag::HIDE
        },
    )
}

/// The stone's box (subs) with its top middle at `at` (cm).
fn stone_box(at: [i16; 3]) -> ([i32; 3], [i32; 3]) {
    let b = stone_record(at, true).bounds();
    (b.lo, b.hi)
}

/// Whether a box (subs) overlaps a solid flat block (strictly; STONE's
/// blocks are all flat).
fn box_hits(b: &Block, lo: [i32; 3], hi: [i32; 3]) -> bool {
    let bb = b.bounds();
    b.solid() && (0..3).all(|k| lo[k] < bb.hi[k] && hi[k] > bb.lo[k])
}

/// Whether a box (subs) overlaps the hero's body.
fn hits_hero(h: &Hero, lo: [i32; 3], hi: [i32; 3]) -> bool {
    h.frozen == 0
        && h.pos[0] - HALF < hi[0]
        && h.pos[0] + HALF > lo[0]
        && h.pos[2] - HALF < hi[2]
        && h.pos[2] + HALF > lo[2]
        && h.pos[1] < hi[1]
        && h.pos[1] + HERO_H > lo[1]
}

/// The course's blocks (all but the stone).
fn course(blocks: &[Block]) -> &[Block] {
    &blocks[..usize::from(STONE).min(blocks.len())]
}

/// Whether the stone with its top middle at `at` (cm) overlaps none of
/// the course's blocks.
pub fn clear(blocks: &[Block], at: [i16; 3]) -> bool {
    let (lo, hi) = stone_box(at);
    !course(blocks).iter().any(|b| box_hits(b, lo, hi))
}

/// Whether the stone fits with its top middle at `at` (cm): it overlaps
/// none of the course's blocks and not the hero.
pub fn fits(blocks: &[Block], hero: &Hero, at: [i16; 3]) -> bool {
    let (lo, hi) = stone_box(at);
    clear(blocks, at) && !hits_hero(hero, lo, hi)
}

/// A ground place: the stone's top middle (cm) 1 m ahead of the hero,
/// if the tops under its middle and four corners are walkable and at
/// most [`FLAT`] apart and the stone fits there.
pub fn ground_spot(blocks: &[Block], hero: &Hero) -> Option<[i16; 3]> {
    let f = forward(hero.yaw);
    let x = to_cm(hero.pos[0] + f[0] * AHEAD / ONE14);
    let z = to_cm(hero.pos[2] + f[1] * AHEAD / ONE14);
    let course = course(blocks);
    let reach = hero.pos[1] + STEP_UP;
    let (under, mid) = top_under(course, cm(x), cm(z), reach)?;
    // Not on the cliff K: a ground place there and a raise would cross
    // the gap to T without the air place the course is built round.
    if under == K {
        return None;
    }
    let h = HALF_SIDE;
    for (dx, dz) in [(-h, -h), (h, -h), (-h, h), (h, h)] {
        let (_, t) = top_under(course, cm(x + dx), cm(z + dz), reach)?;
        if (t - mid).abs() > cm(FLAT) {
            return None;
        }
    }
    let at = [x, to_cm(mid) + THICK, z];
    fits(blocks, hero, at).then_some(at)
}

/// An air place: the stone's top middle (cm) at the feet (rounded down
/// to whole cm, so the feet are never under its top) and the hero's
/// middle, if it fits there.
pub fn air_spot(blocks: &[Block], hero: &Hero) -> Option<[i16; 3]> {
    let feet = hero.pos[1];
    let mut top = (i64::from(feet) * 25).div_euclid(1024) as i16;
    while cm(top) > feet {
        top -= 1;
    }
    let at = [to_cm(hero.pos[0]), top, to_cm(hero.pos[2])];
    fits(blocks, hero, at).then_some(at)
}

// ---- The state ---------------------------------------------------------------------

/// Where the stone is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// In the hero's hand (not there in the world).
    InHand,
    /// Placed; `base` is its top when placed (cm): the raise stops
    /// [`RAISE_MAX`] over it.
    Placed { base: i16 },
    /// Called back: in hand after this many more steps.
    Returning(u8),
}

/// Buttons noted in `before` for `after`.
mod key {
    /// B pressed: place, or arm the raise.
    pub const PLACE: u8 = 1 << 0;
    /// B held: raise.
    pub const HOLD: u8 = 1 << 1;
    /// X pressed: call back.
    pub const CALL: u8 = 1 << 2;
}

/// The area's state (at most 32 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stone {
    pub held: Held,
    /// The stone's top middle (cm: x, top, z), where it was last placed.
    pub at: [i16; 3],
    /// An air place is allowed (set while standing on any top but the
    /// stone, cleared by an air place).
    pub air_ok: bool,
    /// B was pressed while standing on the stone (since the last
    /// landing on it): holding B raises it.
    pub armed: bool,
    /// Steps `NO ROOM` still shows.
    pub no_room: u8,
    /// The buttons of this step ([`key`] bits).
    pub keys: u8,
    /// The clock (steps since the start).
    pub steps: u16,
    /// The last safe spot off the stone (the hero's own safe spot is
    /// put back to it whenever it lands on the stone, which moves and
    /// vanishes).
    pub safe: [i32; 3],
    pub safe_top: u8,
}

impl Default for Stone {
    fn default() -> Stone {
        Stone::new()
    }
}

impl Stone {
    pub const fn new() -> Stone {
        Stone {
            held: Held::InHand,
            at: [0, 0, -300],
            air_ok: true,
            armed: false,
            no_room: 0,
            keys: 0,
            steps: 0,
            safe: [cm(START[0]), cm(START[1]), cm(START[2])],
            safe_top: S,
        }
    }

    /// Whether the stone is in the world.
    pub fn placed(&self) -> bool {
        matches!(self.held, Held::Placed { .. })
    }

    /// Writes the stone's record.
    pub fn movers(&self, out: &mut [Block]) {
        if let Some(o) = out.first_mut() {
            *o = stone_record(self.at, self.placed());
        }
    }

    /// The hero's tuning: the normal run and jump, no glide, and a fall
    /// of 6 m under the safe spot puts him back after 15 steps.
    pub fn body(&self, hero: &Hero) -> Body {
        Body {
            run: RUN,
            jump: JUMP,
            glide: false,
            kill_y: hero.safe[1] - FALL_ROOM,
            respawn: RESPAWN_STEPS,
        }
    }

    /// Notes this step's buttons (acted on in `after`).
    pub fn before(&mut self, pad: &Pad, _hero: &Hero, _blocks: &[Block]) {
        let on = |bits: u32, b: u32, k: u8| if bits & b != 0 { k } else { 0 };
        self.keys = on(pad.pressed, button::B, key::PLACE)
            | on(pad.held, button::B, key::HOLD)
            | on(pad.pressed, button::X, key::CALL);
    }

    /// After the hero moved: the stone's rules (in this order: respawn,
    /// safe spot, air place allowed, return, call back, place, raise),
    /// then the clock. True when the hero stands on G.
    pub fn after(&mut self, hero: &mut Hero, ev: &HeroEvents, blocks: &[Block]) -> bool {
        let keys = core::mem::take(&mut self.keys);
        self.steps = self.steps.saturating_add(1);
        self.no_room = self.no_room.saturating_sub(1);
        if ev.respawned {
            self.held = Held::InHand;
            self.air_ok = true;
        }
        // The safe spot is never on the stone.
        if hero.safe_top == STONE {
            hero.safe = self.safe;
            hero.safe_top = self.safe_top;
        } else {
            self.safe = hero.safe;
            self.safe_top = hero.safe_top;
        }
        let playing = hero.frozen == 0;
        let on_stone = playing && hero.ground == Some(STONE);
        if playing && hero.ground.is_some() && !on_stone {
            self.air_ok = true;
        }
        if !on_stone || ev.landed == Some(STONE) {
            self.armed = false;
        }
        if let Held::Returning(n) = self.held {
            self.held = if n <= 1 {
                Held::InHand
            } else {
                Held::Returning(n - 1)
            };
        }
        if keys & key::CALL != 0 && self.placed() {
            self.held = Held::Returning(RETURN);
        }
        if keys & key::PLACE != 0 && playing {
            if self.held == Held::InHand {
                let spot = if hero.ground.is_some() {
                    ground_spot(blocks, hero)
                } else if self.air_ok {
                    air_spot(blocks, hero)
                } else {
                    None
                };
                match spot {
                    Some(at) => {
                        self.at = at;
                        self.held = Held::Placed { base: at[1] };
                        if hero.ground.is_none() {
                            self.air_ok = false;
                        }
                    }
                    None if hero.ground.is_some() || self.air_ok => {
                        self.no_room = NO_ROOM_STEPS;
                    }
                    None => {}
                }
            } else if on_stone {
                self.armed = true;
            }
        }
        if let Held::Placed { base } = self.held
            && on_stone
            && self.armed
            && keys & key::HOLD != 0
        {
            let top = (self.at[1] + RAISE).min(base + RAISE_MAX);
            let at = [self.at[0], top, self.at[2]];
            if top > self.at[1]
                && clear(blocks, at)
                && body_fits(course(blocks), [hero.pos[0], cm(top), hero.pos[2]])
            {
                self.at = at;
            }
        }
        playing && hero.ground == Some(G)
    }

    /// The HUD: the name, the clock, the stone icon (filled in hand,
    /// outline placed, blinking while returning), `NO ROOM` or the
    /// button hint, and `GOAL` over the beacon.
    pub fn hud(&self, out: &mut Hud) {
        out.text[0].push("STONE");
        time_text(&mut out.text[1], self.steps);
        out.icon = Some(match self.held {
            Held::InHand => true,
            Held::Placed { .. } => false,
            Held::Returning(n) => n & 4 != 0,
        });
        out.msg = if self.no_room > 0 {
            Some("NO ROOM")
        } else if self.steps < HINT_STEPS {
            Some("B STONE  X CALL")
        } else {
            None
        };
        let b = BEACON_AT;
        out.label = Some(([cm(b[0]), cm(b[1] + 165), cm(b[2])], "GOAL"));
    }

    /// The beacon on G, and the stone's outline under the feet while an
    /// air place would work.
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
        if self.held == Held::InHand
            && self.air_ok
            && h.ground.is_none()
            && h.frozen == 0
            && air_spot(play.blocks(), h).is_some()
        {
            push(Extra {
                model: &models::STONE_OUTLINE,
                pos: [h.pos[0], h.pos[1] - cm(OUTLINE_DOWN), h.pos[2]],
                yaw: 0,
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
        let held = match self.held {
            Held::InHand => 0,
            Held::Placed { base } => 0x1_0000 + i64::from(base),
            Held::Returning(n) => 0x2_0000 + i64::from(n),
        };
        let v = [
            held,
            i64::from(self.at[0]),
            i64::from(self.at[1]),
            i64::from(self.at[2]),
            i64::from(self.air_ok),
            i64::from(self.armed),
            i64::from(self.no_room),
            i64::from(self.steps),
            i64::from(self.safe[0]),
            i64::from(self.safe[1]),
            i64::from(self.safe[2]),
            i64::from(self.safe_top),
        ];
        v.iter().fold(f, |h, &x| fnv(h, x))
    }
}
