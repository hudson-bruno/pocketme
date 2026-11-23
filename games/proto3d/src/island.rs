//! The test island as records (PROTOTYPE-SPEC.md, section 3), and the
//! collision tests against them.
//!
//! Records are in whole centimetres (`i16`, up to 327 m), y up. The rules
//! turn them into subs (1/4096 m) when they test them; nothing is worked
//! out ahead and kept.
//!
//! * [`Block`]: a box or a ramp, 16 bytes.
//! * [`Prop`]: a tree, a bush, grass tufts, a rock cone or a far islet,
//!   8 bytes. Props are not solid.
//! * [`Coin`]: a thing to collect, 6 bytes.
//!
//! The layout is the comparison's (`compare/mock/src/scene.rs`), with one
//! change: the collectible over the gap is at 1.6 m, not 0.9 m, so that
//! only a jump across takes it, never a walk or a fall off the edge (spec
//! 4's test; tests/collect.rs).

use crate::life::{Critter, way};
use crate::maths::{clamp, cm};
use crate::tune::{HALF, HERO_H};

/// Materials (palette families for the drawer). The same names and
/// numbers as the comparison crate.
pub mod mat {
    pub const GRASS: u8 = 0;
    pub const GRASS2: u8 = 1;
    pub const DIRT: u8 = 2;
    pub const ROCK: u8 = 3;
    pub const ROCK2: u8 = 4;
    pub const STONE: u8 = 5;
    pub const SAND: u8 = 6;
    pub const LEAF: u8 = 9;
    pub const TRUNK: u8 = 10;
    pub const BLOCK: u8 = 11;
    pub const BLOCK_TOP: u8 = 12;
    /// The test areas' materials (AREAS-SPEC.md, section 2): a warm
    /// gold-brown, a pale teal cloth and a blue-grey stone.
    pub const BRASS: u8 = 17;
    pub const SAIL: u8 = 22;
    pub const SLATE: u8 = 23;
}

/// [`Block::flags`] bits.
pub mod flag {
    /// The top can be stood on.
    pub const WALK: u8 = 1 << 0;
    /// A lighter rim on the top's edges.
    pub const RIM: u8 = 1 << 1;
    /// A dark patch on what is below it (the floating block).
    pub const PATCH: u8 = 1 << 2;
    /// A raised or narrow block the camera may look through: the wall
    /// pull-in ignores it, and while it hides the hero it shrinks away
    /// in the picture (AREAS-SPEC.md, section 0). Never on a ground pad.
    pub const FADE: u8 = 1 << 3;
    /// A ramp drawn as a plank 12 cm thick (collision is still the
    /// wedge).
    pub const SLAB: u8 = 1 << 4;
    /// Switched off: not solid, not drawn, no top.
    pub const HIDE: u8 = 1 << 5;
    /// Drawn with the raised blocks (layer 2) although its top is not
    /// above 0: a step standing on a lower floor (SAIL's S1), so that it
    /// is drawn after the floor it stands on and the parent rule holds
    /// for what stands on it.
    pub const UP: u8 = 1 << 6;
}

/// [`Block::ramp`]: which way a ramp rises. 0 is a flat box.
pub mod ramp {
    pub const FLAT: u8 = 0;
    pub const UP_X: u8 = 1;
    pub const DOWN_X: u8 = 2;
    pub const UP_Z: u8 = 3;
    pub const DOWN_Z: u8 = 4;
}

/// A box or a ramp, in cm. A ramp is the box's wedge: its top goes from
/// `lo[1]` at one end to `hi[1]` at the other, along the axis in `ramp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Block {
    pub lo: [i16; 3],
    pub hi: [i16; 3],
    /// Material of the top, and of the sides.
    pub top: u8,
    pub side: u8,
    /// [`flag`] bits.
    pub flags: u8,
    /// [`ramp`]: flat or which way it rises.
    pub ramp: u8,
}

/// A block's corners in subs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub lo: [i32; 3],
    pub hi: [i32; 3],
}

impl Block {
    /// A flat box (the areas' records use it too).
    pub const fn flat(lo: [i16; 3], hi: [i16; 3], top: u8, side: u8, flags: u8) -> Block {
        Block {
            lo,
            hi,
            top,
            side,
            flags,
            ramp: ramp::FLAT,
        }
    }

    /// The corners in subs.
    pub fn bounds(&self) -> Bounds {
        Bounds {
            lo: [cm(self.lo[0]), cm(self.lo[1]), cm(self.lo[2])],
            hi: [cm(self.hi[0]), cm(self.hi[1]), cm(self.hi[2])],
        }
    }

    /// Whether it is there at all (not [`flag::HIDE`]).
    pub fn solid(&self) -> bool {
        self.flags & flag::HIDE == 0
    }

    /// Whether this is a ramp.
    pub fn is_ramp(&self) -> bool {
        self.ramp != ramp::FLAT
    }

    /// The height of the top (subs) at (x, z) in subs, the point moved
    /// onto the block first (so beside a ramp it is the height at the
    /// nearest edge). For a flat box, its top.
    pub fn surface(&self, x: i32, z: i32) -> i32 {
        let b = self.bounds();
        if !self.is_ramp() {
            return b.hi[1];
        }
        let (k, up) = match self.ramp {
            ramp::UP_X => (0, true),
            ramp::DOWN_X => (0, false),
            ramp::UP_Z => (2, true),
            _ => (2, false),
        };
        let p = if k == 0 { x } else { z };
        let (t0, t1) = (b.lo[k], b.hi[k]);
        let p = clamp(p, t0, t1);
        let along = if up { p - t0 } else { t1 - p };
        let rise = i64::from(b.hi[1] - b.lo[1]) * i64::from(along) / i64::from(t1 - t0).max(1);
        b.lo[1] + rise as i32
    }

    /// Whether the point (subs) is inside the solid (strictly).
    pub fn contains(&self, p: [i32; 3]) -> bool {
        let b = self.bounds();
        p[0] > b.lo[0]
            && p[0] < b.hi[0]
            && p[2] > b.lo[2]
            && p[2] < b.hi[2]
            && p[1] > b.lo[1]
            && p[1] < self.surface(p[0], p[2])
    }
}

impl Bounds {
    /// Whether a square of half width `half` round (x, z) overlaps the
    /// block from above (touching edges do not count).
    pub fn overlaps_square(&self, x: i32, z: i32, half: i32) -> bool {
        x - half < self.hi[0]
            && x + half > self.lo[0]
            && z - half < self.hi[2]
            && z + half > self.lo[2]
    }

    /// Whether (x, z) is over the block (inside its square or on its
    /// edge).
    pub fn over(&self, x: i32, z: i32) -> bool {
        x >= self.lo[0] && x <= self.hi[0] && z >= self.lo[2] && z <= self.hi[2]
    }
}

/// [`Prop::kind`] values (low 6 bits).
pub mod prop {
    /// Trunk and leaves (a pyramid at LOW, a cone at MID, two at HIGH).
    pub const TREE: u8 = 0;
    /// A small dark box.
    pub const BUSH: u8 = 1;
    /// A patch of 12 small three-sided pyramids (HIGH).
    pub const TUFTS: u8 = 2;
    /// The rock point under the main island (from the dirt skirt down).
    pub const CONE_MAIN: u8 = 3;
    /// The rock point under the islet.
    pub const CONE_ISLET: u8 = 4;
    /// A far islet for depth (layer 0).
    pub const FAR_ISLET: u8 = 5;
}

/// A prop: kind and the lowest detail it shows at (in one byte), a turn
/// (1/256 of a turn) and where it stands (cm).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Prop {
    /// Low 6 bits: [`prop`] kind. Top 2 bits: lowest detail (0 LOW, 1
    /// MID, 2 HIGH).
    pub kind: u8,
    pub yaw: u8,
    pub at: [i16; 3],
}

impl Prop {
    /// A prop record: [`prop`] kind, lowest detail, turn, place (cm).
    pub const fn new(kind: u8, min: u8, yaw: u8, at: [i16; 3]) -> Prop {
        Prop {
            kind: kind | (min << 6),
            yaw,
            at,
        }
    }

    /// The [`prop`] kind.
    pub fn what(&self) -> u8 {
        self.kind & 0x3F
    }

    /// The lowest detail it shows at (0 LOW, 1 MID, 2 HIGH).
    pub fn min_detail(&self) -> u8 {
        self.kind >> 6
    }

    /// The ball the camera keeps clear (middle in subs, radius in subs),
    /// for the kinds that shrink (trees, bushes); `None` for the rest.
    /// ESTIMATED from the comparison's shapes: a tree's trunk and leaves
    /// fit in a ball of 1.25 m round a point 1.15 m up; a bush in 0.36 m
    /// round 0.17 m up.
    pub fn ball(&self) -> Option<([i32; 3], i32)> {
        let p = [cm(self.at[0]), cm(self.at[1]), cm(self.at[2])];
        match self.what() {
            prop::TREE => Some(([p[0], p[1] + cm(115), p[2]], cm(125))),
            prop::BUSH => Some(([p[0], p[1] + cm(17), p[2]], cm(36))),
            _ => None,
        }
    }
}

/// A thing to collect, where its middle is (cm).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Coin {
    pub at: [i16; 3],
}

impl Coin {
    /// The middle in subs.
    pub fn pos(&self) -> [i32; 3] {
        [cm(self.at[0]), cm(self.at[1]), cm(self.at[2])]
    }
}

/// One playing field: its records and where the hero starts.
#[derive(Debug)]
pub struct World {
    pub blocks: &'static [Block],
    pub props: &'static [Prop],
    /// Coins; the first `coins_low` exist at every detail, the rest from
    /// MID up.
    pub coins: &'static [Coin],
    pub coins_low: u8,
    /// Start (cm, feet middle).
    pub start: [i16; 3],
    /// The creatures (ISLAND only).
    pub critters: &'static [Critter],
}

// ---- The test island ----------------------------------------------------------

/// Block numbers on the test island (the ground index of the rules).
pub const MAIN: u8 = 0;
pub const TERRACE: u8 = 1;
pub const RAMP: u8 = 2;
pub const STEP: u8 = 3;
pub const FLOAT: u8 = 4;
pub const ISLET: u8 = 5;

const W: u8 = flag::WALK | flag::RIM;

/// The test island's blocks (metres in the spec's table, cm here).
pub static BLOCKS: [Block; 6] = [
    // Main island: x and z from -7 to 7, top 0, a 0.6 m dirt skirt.
    Block::flat([-700, -60, -700], [700, 0, 700], mat::GRASS, mat::DIRT, W),
    // Terrace: x -7 to -1.5, z 1.5 to 7, top 1.0 m.
    Block::flat([-700, 0, 150], [-150, 100, 700], mat::GRASS, mat::STONE, W),
    // Ramp: x -4.5 to -2.5, z -1.5 to 1.5, rising 0 to 1 m along z.
    Block {
        lo: [-450, 0, -150],
        hi: [-250, 100, 150],
        top: mat::SAND,
        side: mat::STONE,
        flags: W,
        ramp: ramp::UP_Z,
    },
    // Step: x 1 to 3, z -1 to 1, top 0.5 m.
    Block::flat([100, 0, -100], [300, 50, 100], mat::STONE, mat::STONE, W),
    // Floating block: x 2 to 3.6, z 3 to 4.6, from 1.1 to 1.5 m.
    Block::flat(
        [200, 110, 300],
        [360, 150, 460],
        mat::BLOCK_TOP,
        mat::BLOCK,
        W | flag::PATCH | flag::FADE,
    ),
    // Islet past the 2.5 m gap: x -2 to 2, z 9.5 to 13.5, top 0.
    Block::flat([-200, -40, 950], [200, 0, 1350], mat::GRASS, mat::DIRT, W),
];

/// The test island's props (the comparison's, at each detail).
pub static PROPS: [Prop; 30] = [
    // Under the islands.
    Prop::new(prop::CONE_MAIN, 0, 32, [0, -60, 0]),
    Prop::new(prop::CONE_ISLET, 0, 32, [0, -40, 1150]),
    // Far islets.
    Prop::new(prop::FAR_ISLET, 1, 0, [-1300, -150, 1300]),
    Prop::new(prop::FAR_ISLET, 1, 0, [1450, 250, 1350]),
    Prop::new(prop::FAR_ISLET, 2, 0, [250, -350, 1900]),
    // Trees: five at LOW, six at MID, eight at HIGH.
    Prop::new(prop::TREE, 0, 0, [550, 0, -500]),
    Prop::new(prop::TREE, 0, 35, [-500, 0, -450]),
    Prop::new(prop::TREE, 0, 70, [550, 0, 250]),
    Prop::new(prop::TREE, 0, 105, [-150, 0, -620]),
    Prop::new(prop::TREE, 0, 141, [-300, 100, 450]),
    Prop::new(prop::TREE, 1, 176, [80, 0, 600]),
    Prop::new(prop::TREE, 2, 211, [540, 0, 590]),
    Prop::new(prop::TREE, 2, 246, [-230, 100, 260]),
    // Bushes: five at MID, eight at HIGH.
    Prop::new(prop::BUSH, 1, 0, [-350, 0, -250]),
    Prop::new(prop::BUSH, 1, 0, [320, 0, -420]),
    Prop::new(prop::BUSH, 1, 0, [650, 0, -150]),
    Prop::new(prop::BUSH, 1, 0, [-620, 0, -100]),
    Prop::new(prop::BUSH, 1, 0, [150, 0, 330]),
    Prop::new(prop::BUSH, 2, 0, [-80, 0, -500]),
    Prop::new(prop::BUSH, 2, 0, [420, 0, 620]),
    Prop::new(prop::BUSH, 2, 0, [-50, 0, 150]),
    // Grass tufts (HIGH): patch middles.
    Prop::new(prop::TUFTS, 2, 0, [300, 0, -250]),
    Prop::new(prop::TUFTS, 2, 0, [-230, 0, -330]),
    Prop::new(prop::TUFTS, 2, 0, [480, 0, -560]),
    Prop::new(prop::TUFTS, 2, 0, [-420, 0, -590]),
    Prop::new(prop::TUFTS, 2, 0, [-20, 0, -60]),
    // MAX only: more tufts (on the islet, the terrace and by the step)
    // and one more bush.
    Prop::new(prop::TUFTS, 3, 0, [-40, 0, 1170]),
    Prop::new(prop::TUFTS, 3, 0, [-470, 100, 380]),
    Prop::new(prop::TUFTS, 3, 0, [180, 0, -480]),
    Prop::new(prop::BUSH, 3, 0, [610, 0, 30]),
];

/// Coin numbers on the test island.
pub const COIN_STEP: u8 = 0;
pub const COIN_FLOAT: u8 = 1;
pub const COIN_GAP: u8 = 2;
pub const COIN_ISLET: u8 = 3;
pub const COIN_TERRACE: u8 = 4;
pub const COIN_CORNER: u8 = 5;

/// The six things to collect (four at LOW).
pub static COINS: [Coin; 6] = [
    // Over the step (teaches: jump onto the step).
    Coin { at: [200, 90, 0] },
    // Over the floating block (teaches: from the step to the block).
    Coin {
        at: [280, 185, 380],
    },
    // Over the gap (teaches: the long jump). 1.6 m up, not the
    // comparison's 0.9 m: at 0.9 m walking off the edge takes it.
    Coin { at: [0, 160, 825] },
    // On the islet (teaches: land on the far side).
    Coin { at: [0, 60, 1150] },
    // MID and HIGH: over the terrace (the ramp) and by the far corner.
    Coin {
        at: [-400, 140, 400],
    },
    Coin {
        at: [-350, 30, -500],
    },
];

/// The test island.
pub static ISLAND: World = World {
    blocks: &BLOCKS,
    props: &PROPS,
    coins: &COINS,
    coins_low: 4,
    start: [50, 0, -320],
    critters: &CRITTERS,
};

/// The test island's creatures (places ESTIMATED to teach, not tuned):
/// four burrs, none on the only way anywhere, and three tuftlings.
pub static CRITTERS: [Critter; 7] = [
    // In front of the step: across the way from the start to the step.
    Critter::burr([-60, 0, -140], way::PX, 300, 0),
    // On the terrace, away from its edges (west 1.8 m, north 1.4 m).
    Critter::burr([-520, 100, 260], way::PZ, 300, 40),
    // On the islet, behind its nut.
    Critter::burr([-90, 0, 1250], way::PX, 180, 90),
    // By the far corner's nut.
    Critter::burr([-560, 0, -590], way::PX, 180, 20),
    // Near the start, by the ramp.
    Critter::tuft([-180, 0, -300], 64),
    // East of the floating block.
    Critter::tuft([440, 0, 380], 192),
    // Beside the way to the gap.
    Critter::tuft([-70, 0, 540], 128),
];
const _: () = assert!(CRITTERS.len() <= crate::life::MAX_CRITTERS);

// ---- Collision helpers ------------------------------------------------------------

/// Whether a hero standing with the feet at `p` (subs) would be inside
/// any block.
pub fn body_fits(blocks: &[Block], p: [i32; 3]) -> bool {
    blocks.iter().all(|b| {
        let bb = b.bounds();
        !(b.solid()
            && bb.overlaps_square(p[0], p[2], HALF)
            && p[1] < b.surface(p[0], p[2])
            && p[1] + HERO_H > bb.lo[1])
    })
}

/// Whether the point (subs) is inside any block.
pub fn inside_any(blocks: &[Block], p: [i32; 3]) -> Option<u8> {
    blocks
        .iter()
        .position(|b| b.solid() && b.contains(p))
        .map(|i| i as u8)
}

/// The highest walkable top (subs) under the point (x, z) at or below
/// `y`, with its block number.
pub fn top_under(blocks: &[Block], x: i32, z: i32, y: i32) -> Option<(u8, i32)> {
    let mut best: Option<(u8, i32)> = None;
    for (i, b) in blocks.iter().enumerate() {
        if b.flags & flag::WALK == 0 || !b.solid() || !b.bounds().over(x, z) {
            continue;
        }
        let s = b.surface(x, z);
        if s <= y && best.is_none_or(|(_, t)| s > t) {
            best = Some((i as u8, s));
        }
    }
    best
}

/// Where the segment from `a` to `b` (subs) first enters a block: the
/// fraction of the way (1/65536, above 0) and the block number. A block
/// the segment starts inside is passed over. Ramps are tested as their
/// wedge. Switched-off blocks ([`flag::HIDE`]) are not there.
pub fn first_hit(blocks: &[Block], a: [i32; 3], b: [i32; 3]) -> Option<(u32, u8)> {
    first_hit_mask(blocks, a, b, flag::HIDE)
}

/// [`first_hit`] passing over every block with any of the `skip` flags
/// (the camera's wall ray skips [`flag::FADE`] and [`flag::HIDE`]).
pub fn first_hit_mask(blocks: &[Block], a: [i32; 3], b: [i32; 3], skip: u8) -> Option<(u32, u8)> {
    let mut best: Option<(u32, u8)> = None;
    for (i, blk) in blocks.iter().enumerate() {
        if blk.flags & skip != 0 {
            continue;
        }
        if let Some(s) = enter(blk, a, b)
            && best.is_none_or(|(t, _)| s < t)
        {
            best = Some((s, i as u8));
        }
    }
    best
}

/// Whether the segment from `a` to `b` (subs) enters this block (see
/// [`first_hit`]).
pub fn crosses(blk: &Block, a: [i32; 3], b: [i32; 3]) -> bool {
    enter(blk, a, b).is_some()
}

/// The squared distance (subs squared) from `p` to the block's box.
pub fn box_dist2(blk: &Block, p: [i32; 3]) -> i64 {
    let bb = blk.bounds();
    (0..3)
        .map(|k| {
            let d = i64::from((bb.lo[k] - p[k]).max(p[k] - bb.hi[k]).max(0));
            d * d
        })
        .sum()
}

/// One block for [`first_hit`].
fn enter(blk: &Block, a: [i32; 3], b: [i32; 3]) -> Option<u32> {
    const ONE: i64 = 65_536;
    let bb = blk.bounds();
    let (mut lo, mut hi) = (0i64, ONE);
    // One rule f(s) <= 0 along the segment, with f(a) = fa and f(b) = fb.
    let mut cut = |fa: i64, fb: i64| -> bool {
        if fa > 0 && fb > 0 {
            return false;
        }
        if fa <= 0 && fb <= 0 {
            return true;
        }
        let s = fa * ONE / (fa - fb);
        if fa > 0 {
            lo = lo.max(s);
        } else {
            hi = hi.min(s);
        }
        true
    };
    for k in 0..3 {
        let (pa, pb) = (i64::from(a[k]), i64::from(b[k]));
        let (l, h) = (i64::from(bb.lo[k]), i64::from(bb.hi[k]));
        if !cut(l - pa, l - pb) || !cut(pa - h, pb - h) {
            return None;
        }
    }
    if blk.is_ramp() {
        // Below the slope: (y - y0) * span - rise * along <= 0.
        let (k, up) = match blk.ramp {
            ramp::UP_X => (0, true),
            ramp::DOWN_X => (0, false),
            ramp::UP_Z => (2, true),
            _ => (2, false),
        };
        let span = i64::from(bb.hi[k] - bb.lo[k]);
        let rise = i64::from(bb.hi[1] - bb.lo[1]);
        let f = |p: [i32; 3]| {
            let along = if up {
                i64::from(p[k] - bb.lo[k])
            } else {
                i64::from(bb.hi[k] - p[k])
            };
            i64::from(p[1] - bb.lo[1]) * span - rise * along
        };
        if !cut(f(a), f(b)) {
            return None;
        }
    }
    if lo < hi && lo > 0 {
        Some(lo as u32)
    } else {
        None
    }
}

/// The squared distance (game units squared) from the point `c` to the
/// segment from `a` to `b` (all in game units).
pub fn dist2_to_segment(c: [i32; 3], a: [i32; 3], b: [i32; 3]) -> i64 {
    let v: [i64; 3] = core::array::from_fn(|k| i64::from(b[k] - a[k]));
    let w: [i64; 3] = core::array::from_fn(|k| i64::from(c[k] - a[k]));
    let vv: i64 = v.iter().map(|x| x * x).sum();
    let wv: i64 = (0..3).map(|k| w[k] * v[k]).sum();
    let near: [i64; 3] = if vv == 0 || wv <= 0 {
        [0; 3]
    } else if wv >= vv {
        v
    } else {
        core::array::from_fn(|k| v[k] * wv / vv)
    };
    (0..3).map(|k| (w[k] - near[k]) * (w[k] - near[k])).sum()
}
