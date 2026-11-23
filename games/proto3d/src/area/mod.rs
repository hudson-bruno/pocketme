//! The test areas (AREAS-SPEC.md, sections 1 and 2): ISLAND, the first
//! prototype's test island, and three areas that each try one idea.
//!
//! * [`Area`]: which area, its name on the menu and its records.
//! * [`Kit`]: the area's own state, one variant per area (ISLAND has
//!   none). [`crate::play::Play`] calls it at fixed points of each step;
//!   it is a plain `match`, no trait object.
//! * [`Extra`]: an extra model the area wants drawn (a sign, a weight, a
//!   sail), and [`Hud`]: its line of text on the screen.
//!
//! Each area file ([`machines`], [`sail`], [`stone`]) gives the same
//! small interface (the spec's table): `WORLD`, `MOVERS`, a state type
//! with `new`, `movers`, `body`, `before`, `after`, `hud`, `extras`,
//! `time` and `hash`. MACHINES is weight (a bob, a plate, a bridge and a
//! tipping plank), SAIL the glide against the clock, STONE a slab the
//! hero places, raises and calls back; each file's own doc has its rules.

pub mod machines;
pub mod sail;
pub mod stone;

use game_3d::Model;

use crate::draw::Text;
use crate::hero::{Body, Hero, HeroEvents};
use crate::island::{Block, ISLAND, Prop, World, flag, mat, prop};
use crate::models;
use crate::play::Play;
use crate::step::Pad;

/// The areas, in menu order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Area {
    Island = 0,
    Machines = 1,
    Sail = 2,
    Stone = 3,
}

impl Area {
    pub const ALL: [Area; 4] = [Area::Island, Area::Machines, Area::Sail, Area::Stone];

    /// The name on the menu and in the log.
    pub const fn name(self) -> &'static str {
        match self {
            Area::Island => "ISLAND",
            Area::Machines => "MACHINES",
            Area::Sail => "SAIL",
            Area::Stone => "STONE",
        }
    }

    /// The area's records (its starting blocks, props, coins, start).
    pub fn world(self) -> &'static World {
        match self {
            Area::Island => &ISLAND,
            Area::Machines => &machines::WORLD,
            Area::Sail => &sail::WORLD,
            Area::Stone => &stone::WORLD,
        }
    }

    /// Its place in [`Area::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// The most extra models an area draws in one picture.
pub const MAX_EXTRAS: usize = 6;

/// An extra model to draw: where it stands (subs, its origin), its turn
/// (1/65536 turn) and its size (256 = as made). It joins the far-to-near
/// sort with the props.
#[derive(Clone, Copy)]
pub struct Extra {
    pub model: &'static Model,
    pub pos: [i32; 3],
    pub yaw: u16,
    pub scale: u16,
}

impl Extra {
    /// An empty slot (never drawn: only the first `n` are).
    pub const NONE: Extra = Extra {
        model: &models::SIGN,
        pos: [0; 3],
        yaw: 0,
        scale: 256,
    };
}

/// An area's HUD, filled each picture by `hud`.
///
/// * `text[0]` top left, `text[1]` top right (in place of ISLAND's
///   counter).
/// * `pips`: small squares under `text[1]`, the first `lit` filled.
/// * `bar`: a bar at the top middle, 0 (none) to 255 (full).
/// * `msg`: a word in the middle near the bottom (for example `NO ROOM`).
/// * `label`: a word written over a world point (subs), when it is in
///   view (the stubs' signs use it).
/// * `best_line`: `BEST 21.3` (the area's best time, `BEST --.-` before
///   the first finish) under `text[1]` (SAIL).
/// * `low_bar`: a bar 30 px long and 2 px high at the bottom middle,
///   filled this many px (SAIL's sail bar); `None`: not shown.
/// * `icon`: an 8x8 square under `text[1]` at the right, filled
///   (`Some(true)`) or an outline (`Some(false)`) (STONE's stone: in
///   hand, placed; the area blinks it while it returns); `None`: not
///   shown.
#[derive(Default)]
pub struct Hud {
    pub text: [Text; 2],
    pub pips: u8,
    pub lit: u8,
    pub bar: u8,
    pub msg: Option<&'static str>,
    pub label: Option<([i32; 3], &'static str)>,
    pub best_line: bool,
    pub low_bar: Option<u8>,
    pub icon: Option<bool>,
}

/// The area's own state: what the rules of that area keep between steps.
/// ISLAND (and every test world) has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kit {
    None,
    Machines(machines::Machines),
    Sail(sail::Sail),
    Stone(stone::Stone),
}

/// Calls the same method on whichever area is in the kit.
macro_rules! each {
    ($kit:expr, $k:ident => $e:expr, none => $none:expr) => {
        match $kit {
            Kit::None => $none,
            Kit::Machines($k) => $e,
            Kit::Sail($k) => $e,
            Kit::Stone($k) => $e,
        }
    };
}

impl Kit {
    /// A fresh state for the area.
    pub const fn new(area: Area) -> Kit {
        match area {
            Area::Island => Kit::None,
            Area::Machines => Kit::Machines(machines::Machines::new()),
            Area::Sail => Kit::Sail(sail::Sail::new()),
            Area::Stone => Kit::Stone(stone::Stone::new()),
        }
    }

    /// How many records at the end of the world's blocks are movers.
    pub const fn movers_n(&self) -> usize {
        match self {
            Kit::None => 0,
            Kit::Machines(_) => machines::MOVERS,
            Kit::Sail(_) => sail::MOVERS,
            Kit::Stone(_) => stone::MOVERS,
        }
    }

    /// Writes the movers' records for now into `out` (`movers_n` long).
    pub fn movers(&self, out: &mut [Block]) {
        each!(self, k => k.movers(out), none => ())
    }

    /// The hero's tuning for this step.
    pub fn body(&self, hero: &Hero) -> Body {
        each!(self, k => k.body(hero), none => Body::NORMAL)
    }

    /// Button actions, before the hero moves.
    pub fn before(&mut self, pad: &Pad, hero: &Hero, blocks: &[Block]) {
        each!(self, k => k.before(pad, hero, blocks), none => ())
    }

    /// After the hero moved: the machines, rides, loads, the clock. True
    /// when the area is done.
    pub fn after(&mut self, hero: &mut Hero, ev: &HeroEvents, blocks: &[Block]) -> bool {
        each!(self, k => k.after(hero, ev, blocks), none => false)
    }

    /// The HUD for this picture.
    pub fn hud(&self, out: &mut Hud) {
        each!(self, k => k.hud(out), none => ())
    }

    /// Extra models for this picture; returns how many of `out` are set.
    pub fn extras(&self, play: &Play, out: &mut [Extra]) -> usize {
        each!(self, k => k.extras(play, out), none => 0)
    }

    /// The area's clock in steps, if it keeps one (the best time and the
    /// done panel use it).
    pub fn time(&self) -> Option<u16> {
        each!(self, k => k.time(), none => None)
    }

    /// The area's state mixed into a fingerprint.
    pub fn hash(&self, f: u64) -> u64 {
        each!(self, k => k.hash(f), none => f)
    }
}

// ---- What the stubs share ------------------------------------------------------

const W: u8 = flag::WALK | flag::RIM;

/// A stub's ground: a flat 14 m square of grass, top at 0, with a 0.6 m
/// dirt skirt (ISLAND's main block).
pub const STUB_GROUND: Block =
    Block::flat([-700, -60, -700], [700, 0, 700], mat::GRASS, mat::DIRT, W);

/// A stub's props: the rock point under the ground.
pub static STUB_PROPS: [Prop; 1] = [Prop::new(prop::CONE_MAIN, 0, 32, [0, -60, 0])];

/// A stub's start (cm): 2 m behind the middle, facing the sign.
pub const STUB_START: [i16; 3] = [0, 0, -200];

/// Where a stub's sign stands (cm): 3.5 m in front of the start and
/// 1.2 m to the side, so the hero does not stand in front of it.
pub const SIGN_AT: [i16; 3] = [-120, 0, 130];

/// A stub's sign: the post and board, and its name written over the
/// board. Returns 1 (one extra).
pub fn sign(out: &mut [Extra], hud: Option<&mut Hud>, name: &'static str) -> usize {
    let at = SIGN_AT.map(crate::maths::cm);
    if let Some(h) = hud {
        h.label = Some((
            [
                at[0],
                at[1] + crate::maths::cm(models::SIGN_BOARD_MID),
                at[2],
            ],
            name,
        ));
    }
    if let Some(e) = out.first_mut() {
        *e = Extra {
            model: &models::SIGN,
            pos: at,
            yaw: 0,
            scale: 256,
        };
        1
    } else {
        0
    }
}
