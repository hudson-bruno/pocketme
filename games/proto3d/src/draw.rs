//! The picture (PROTOTYPE-SPEC.md, sections 3, 5 and 6): sky, the island
//! in 3D, the counter, the line of numbers and the pause menu. Nothing
//! in the rules reads this file.
//!
//! Order, far to near (the spec's study rules):
//! 1. The sky: bands above the horizon, haze below it.
//! 2. Layer 0: the far islets, farther first.
//! 3. Layer 1: the floors (tops at height 0 or below: the main island
//!    and the islet), farther first. Each one: its rock cone, its dirt
//!    skirt, its top in checked tiles, the dark patch under the floating
//!    block, the lighter rim, then the shadow or the landing ring if
//!    they lie on it. A late floor, one that may hide the hero while he
//!    can hide nothing of it (a pad over him), waits for layer 2.
//! 4. Layer 2: the raised blocks, the late floors, trees, bushes, tufts,
//!    collectibles and the hero, sorted far to near by the depth of each
//!    middle, then the order fix: the blocks, late floors and the hero's
//!    body are boxes, and whatever lies behind another by a plane between
//!    them goes first. A thing standing on a raised block or a late floor
//!    (its parent) is drawn after it; the shadow and ring are drawn with
//!    the block they lie on.
//! 5. The counter (or the area's HUD), the line of numbers (SELECT), the
//!    done panel and the menus, on top.
//!
//! The test areas (AREAS-SPEC.md) add: blocks switched off
//! ([`flag::HIDE`]) are not drawn; a FADE block ([`flag::FADE`]) shrinks
//! toward its middle while it hides the hero ([`crate::camera::Cam::fade`]),
//! its dark patch with it; a SLAB ramp is drawn as a 12 cm plank; the
//! area's extra models join layer 2's sort.
//!
//! Blocks are drawn straight from their records, CAMBER's way: each face
//! that looks at the eye becomes up to four corners through [`Pt::new`],
//! then [`draw_poly`]. The big grass tops are cut into tiles that share
//! their corners, so no crack opens between them. Props, the hero and the
//! collectibles are fixed models ([`crate::models`]) drawn by game-3d's
//! [`Renderer`].
//!
//! The fourth detail, MAX, adds over HIGH:
//! * finer grass near the hero: within [`FINE_REACH`] of his feet each
//!   grass tile gets two quarters of the other colour over it (a checker
//!   of half tiles); the tops have [`MAX_TILES`] tiles across 14 m (14,
//!   as at HIGH, today);
//! * a rounder hero: his body, arms and legs from the HIGH model, with an
//!   eight-sided head and a domed cap ([`models::HERO_HEAD`]) and the
//!   brim, drawn as parts in the order the eye needs (not when squashed);
//! * rounder trees ([`models::TREE_MAX`]); bushes and the rock points
//!   under the islands drawn twice, the copy turned half a side, for a
//!   rounder outline (the rock copy not when the eye is over the top);
//! * bevelled edges: a strip of the top's rim colour `BEVEL` cm down the
//!   sides of the raised blocks with a rim that are not grass-topped
//!   (the floating block, the step and the ramp, the resting bob, the
//!   stone);
//! * four more grass and bush props on ISLAND (shown at MAX only);
//! * grass hanging over the flat grass tops' side edges (`Ctx::lip`), a
//!   row of teeth on each side near the eye, shorter with distance;
//! * the ground decoration that [`LOOK`] picks: none (1, this build), a
//!   few one-colour flower patches on ISLAND (2, `Ctx::patches`), or a
//!   light meadow of flat flowers, clover and pebbles (3, `Ctx::meadow`,
//!   one near band at a sixth of R6's pieces). R6's full meadow (a near
//!   and a far band) was found too busy by the owner on 2026-11-22.
//!
//! The creatures of ISLAND ([`crate::life`]) join layer 2's sort at every
//! detail, each with the face count of its detail.
//!
//! [`FINE_REACH`] and [`MAX_TILES`] are the numbers to trim MAX by.

use core::num::NonZeroU32;

use game_3d::{
    Camera, Checked, Count, Lens, Lit, M8, Mat3, Model, ONE14, Object, Planes, Pt, Rect, Renderer,
    Setup, Shade, Stats, View, cos_fine, draw_poly, in_view, isqrt, sin_fine,
};
use game_kit::{Frame, H, W, rgb565, text_width};

use crate::area::{Area, Extra, Hud, MAX_EXTRAS};
use crate::camera::FULL;
use crate::hero::Pose;
use crate::island::{Block, flag, prop, top_under};
use crate::life;
use crate::maths::clamp;
use crate::models::{self, MATS, mat, uc};
use crate::pause::{AREA_BOX, AREA_LINE0, AreaMenu, MENU_BOX, MENU_LINE0, MENU_LINES, Menu};
use crate::play::Play;
use crate::tune::{RING_IN, RING_OUT};

/// The 3D choices: checked sums (overflow checks stay on), near plane at
/// a quarter metre, CAMBER's 32-bit lens and crack-proof cuts for the
/// shared corners, KESTREL's corner culling, middle depth, sizes (props
/// shrink out of the line of sight) and the inside shortcut.
pub struct Proto3d;

impl Setup for Proto3d {
    type Sums = Checked;
    const NEAR: i32 = M8 / 4;
    const LENS: Lens = Lens::Fast;
    const SEAM_PROOF: bool = true;
}

/// The lens, in pixels (the comparison's).
pub const FOCAL: i32 = 100;
/// The whole screen.
pub const VIEW: View = View::new(Rect::new(0, 0, W as i32, H as i32), FOCAL);
const PLANES: Planes = VIEW.clip_planes_at(Proto3d::NEAR);
/// Nothing farther than 30 m is drawn (game units).
pub const FAR: i32 = 30 * M8;
/// Corners of a polygon after cutting: a quad or an 8-sided disc plus
/// five planes, with room to spare.
const MAX_POLY: usize = 16;
/// The renderer: 64 corners and 128 faces a model (the hero has 64 and
/// 96). About 2.6 KB, kept in the game's state.
pub type Rend = Renderer<Proto3d, 64, 128, MAX_POLY, Stats>;

/// Palette numbers past the lit materials (shade * 24 + material, 0 to
/// 95).
pub mod col {
    pub const SKY0: u8 = 96;
    pub const SKY1: u8 = 97;
    pub const SKY2: u8 = 98;
    pub const HAZE: u8 = 99;
    pub const TEXT: u8 = 100;
    pub const TEXT_SHADOW: u8 = 101;
    pub const MENU_BG: u8 = 102;
    pub const MENU_SEL: u8 = 103;
    pub const RING: u8 = 104;
    /// Shadow and patch colours, one per kind of top.
    pub const DARK_GRASS: u8 = 105;
    pub const DARK_STONE: u8 = 106;
    pub const DARK_SAND: u8 = 107;
    pub const DARK_BLOCK: u8 = 108;
}

/// Base colour of each material (shade 0).
const BASE: [[u8; 3]; MATS as usize] = [
    [96, 168, 72],   // 0 grass
    [82, 148, 62],   // 1 grass, second checker colour
    [132, 94, 62],   // 2 dirt
    [112, 104, 100], // 3 rock
    [90, 84, 84],    // 4 rock, darker
    [160, 158, 150], // 5 stone
    [214, 192, 130], // 6 sand
    [244, 192, 40],  // 7 gold
    [50, 118, 58],   // 8 bush leaves
    [62, 140, 66],   // 9 leaves
    [118, 80, 50],   // 10 trunk
    [178, 112, 64],  // 11 floating block sides
    [210, 154, 92],  // 12 floating block top
    [186, 22, 48],   // 13 hero top, crimson
    [242, 242, 238], // 14 cap, white
    [232, 182, 142], // 15 skin
    [58, 66, 98],    // 16 trousers
    [176, 132, 60],  // 17 brass, a warm gold-brown
    [150, 212, 112], // 18 rim on grass
    [208, 208, 198], // 19 rim on stone
    [242, 228, 178], // 20 rim on sand
    [238, 194, 134], // 21 rim on the block
    [150, 206, 196], // 22 sail cloth, a pale teal
    [104, 116, 136], // 23 slate, a blue-grey stone
];

/// Brightness of the four shades, in percent.
const SHADES: [u16; 4] = [100, 86, 72, 58];

/// The fixed colours from [`col::SKY0`] on.
const FIXED: [[u8; 3]; 13] = [
    [45, 85, 175],   // sky, top
    [95, 150, 230],  // sky
    [175, 210, 245], // sky near the horizon
    [215, 225, 240], // haze below the horizon
    [255, 255, 255], // text
    [20, 24, 36],    // text shadow
    [28, 34, 52],    // menu box
    [244, 192, 40],  // menu choice
    [250, 244, 214], // landing ring
    [44, 88, 40],    // shadow on grass
    [92, 90, 86],    // shadow on stone
    [138, 118, 78],  // shadow on sand
    [112, 72, 42],   // shadow on the block
];

/// Colours used: 96 lit and 13 fixed.
pub const COLOURS: usize = MATS as usize * SHADES.len() + FIXED.len();

const fn palette() -> [u16; COLOURS] {
    let mut out = [0u16; COLOURS];
    let mut s = 0;
    while s < SHADES.len() {
        let mut m = 0;
        while m < MATS as usize {
            let c = BASE[m];
            let k = SHADES[s];
            out[s * MATS as usize + m] = rgb565(
                (c[0] as u16 * k / 100) as u8,
                (c[1] as u16 * k / 100) as u8,
                (c[2] as u16 * k / 100) as u8,
            );
            m += 1;
        }
        s += 1;
    }
    let mut i = 0;
    while i < FIXED.len() {
        let c = FIXED[i];
        out[col::SKY0 as usize + i] = rgb565(c[0], c[1], c[2]);
        i += 1;
    }
    out
}

/// The colours, set with `ctx.set_colours(0, &PALETTE)`.
pub const PALETTE: [u16; COLOURS] = palette();

/// Light falls along (-35, -85, 40), one long (16384): from above, the
/// left and behind the start view.
pub const LIGHT: [i32; 3] = [-5_720, -13_892, 6_537];

/// Four shades, no fog; a face turned from the light gets the darkest.
pub const SHADE: Lit<false> = Lit {
    mats: MATS,
    shades: SHADES.len() as u8,
    light: LIGHT,
    glow: 0,
    fog_from: 0,
    fog_to: 0,
    fog_shades: 0,
    clear: 0,
    dark_side: 3,
};

/// Floor tiles across 14 m at LOW, MID, HIGH and MAX (grass tops only).
/// LOW has 3 so the busiest LOW view stays under its budget of 160.
pub const TILES: [i32; 4] = [3, 8, 14, MAX_TILES];
/// MAX's tiles across 14 m: one of its trim numbers (FINAL.md has
/// the table of what each value costs).
pub const MAX_TILES: i32 = 14;
/// MAX: a grass tile whose middle is at most this far (cm, along x and
/// along z) from the hero's feet is cut in four. Another trim number.
pub const FINE_REACH: i32 = 200;
/// The rim's width (cm).
const RIM: i32 = 8;
/// MAX: how far a raised block's bevel runs down its sides (cm).
const BEVEL: i32 = 6;
/// The most items in layer 2, and blocks a world may have here.
pub const MAX_ITEMS: usize = 48;
const MAX_BLOCKS: usize = crate::play::MAX_LIVE;
/// A SLAB ramp's thickness (cm).
const SLAB: i32 = 12;

const UP: [i32; 3] = [0, ONE14, 0];

/// MAX's ground decoration in this build (the three looks tried after
/// the owner found R6's meadow too busy, 2026-11-22): 1 clean (no
/// meadow), 2 a few flower patches ([`PATCHES`]), 3 a light meadow (one
/// thin near band, [`MEADOW`]).
pub const LOOK: u8 = 1;

/// MAX's meadow ([`Ctx::meadow`]) in bands, each `(cell, [inset, start,
/// end, size])`: one piece or none per `cell` cm, kept `inset` cm from
/// the tiles' edge (more than a piece's reach at its size), drawn from
/// `start` to `end` cm from the eye along the ground, growing over the
/// first [`MEADOW_RAMP`] cm and shrinking over the last (nothing pops),
/// at `size` units per cm times 256 (655 is the pieces' own size). R6
/// had a near band of 54 cm cells and a far band; the light meadow
/// (look 3) keeps the near band only, at a sixth of its pieces (cells
/// 54 x sqrt 6 = 132 cm).
const MEADOW: &[(NonZeroU32, [i32; 4])] = match LOOK {
    3 => &[(NonZeroU32::new(132).unwrap(), [36, -1_000, 850, 655])],
    _ => &[],
};

/// MAX look 2: flower patches on ISLAND, each `(x, y, z, flower)` (cm,
/// the top's height; the flower material), away from the ways the hero
/// takes and from the creatures. Each is a small cluster of the meadow's
/// pieces ([`CLUSTER`]), all its flowers one colour.
const PATCHES: &[([i16; 3], u8)] = if LOOK == 2 {
    &[
        ([480, 0, -300], mat::GOLD),
        ([-560, 0, -220], mat::CAP),
        ([260, 0, -600], mat::HERO),
        ([400, 0, 120], mat::CAP),
        ([-610, 100, 630], mat::GOLD),
        ([-330, 100, 640], mat::HERO),
        ([-140, 0, 1080], mat::GOLD),
        ([280, 0, 560], mat::HERO),
    ]
} else {
    &[]
};
/// A patch's flowers: (x, z) in cm from the patch's middle. Each is a
/// flat square on its corner, [`DOT`] cm from middle to corner.
const DOTS: [(i8, i8); 6] = [(0, 0), (24, 10), (-22, 14), (8, -24), (-14, -18), (4, 28)];
const DOT: i32 = 9;
const MEADOW_RAMP: i32 = 200;
/// The meadow's cells are counted from this far (cm) below 0, so every
/// record's place is above it.
const MEADOW_OFF: i32 = 1 << 16;
/// MAX's grass lip ([`Ctx::lip`]): the teeth's width (cm), and the reach
/// at full length and the end of the shortening (cm from the eye).
const LIP: i32 = 35;
const LIP_FULL: i32 = 900;
const LIP_END: i32 = 1200;
/// A meadow piece's colour that is the piece's flower colour.
const FLOWER: u8 = 255;
/// The meadow's pieces (one per hash value 0 to 7): flat polygons `(material, corners in cm round the
/// piece's middle)`, at most six corners, each piece within 32 cm.
type Piece = &'static [(u8, &'static [[i8; 2]])];
const PIECES: [Piece; 8] = [
    // Three flowers.
    &[
        (FLOWER, &[[0, -9], [9, 0], [0, 9], [-9, 0]]),
        (FLOWER, &[[18, -1], [27, 8], [18, 17], [9, 8]]),
        (FLOWER, &[[6, 11], [15, 20], [6, 29], [-3, 20]]),
    ],
    &[
        (FLOWER, &[[0, -9], [9, 0], [0, 9], [-9, 0]]),
        (FLOWER, &[[-14, 6], [-6, 14], [-14, 22], [-22, 14]]),
    ],
    // Clover, a flower on it.
    &[
        (
            mat::LEAF2,
            &[
                [-25, -10],
                [-5, -22],
                [20, -15],
                [28, 5],
                [8, 22],
                [-18, 15],
            ],
        ),
        (FLOWER, &[[2, -7], [9, 0], [2, 7], [-5, 0]]),
    ],
    &[(
        mat::LEAF2,
        &[[-20, -14], [6, -20], [22, -2], [10, 18], [-16, 12]],
    )],
    // Pebbles.
    &[
        (mat::ROCK2, &[[-6, -4], [4, -7], [8, 3], [-3, 7]]),
        (mat::ROCK, &[[10, 6], [19, 4], [20, 12], [12, 14]]),
    ],
    &[(mat::ROCK2, &[[-8, -5], [6, -8], [9, 4], [-5, 8]])],
    // Four flowers in a ring.
    &[
        (FLOWER, &[[-12, -21], [-3, -12], [-12, -3], [-21, -12]]),
        (FLOWER, &[[12, -19], [20, -11], [12, -3], [4, -11]]),
        (FLOWER, &[[14, 4], [22, 12], [14, 20], [6, 12]]),
        (FLOWER, &[[-10, 6], [-1, 15], [-10, 24], [-19, 15]]),
    ],
    // Clover with two flowers.
    &[
        (
            mat::LEAF2,
            &[
                [-30, -6],
                [-12, -24],
                [14, -20],
                [30, 0],
                [16, 20],
                [-16, 18],
            ],
        ),
        (FLOWER, &[[-12, -9], [-5, -2], [-12, 5], [-19, -2]]),
        (FLOWER, &[[10, -3], [17, 4], [10, 11], [3, 4]]),
    ],
];

/// One thing in layer 2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Item {
    /// Depth of its middle (camera z, game units); bigger is drawn first.
    key: i32,
    /// Its own depth before the parent rule (the test areas break equal
    /// keys with it, farther first).
    own: i32,
    kind: Kind,
    i: u8,
    /// What it must be drawn after: the raised block it stands on, or
    /// [`ON_HERO`] when it is carried over his head ([`NO_PARENT`] for
    /// none, and for blocks and the hero, which the order fix places by
    /// their boxes).
    on: u8,
}

/// [`Item::on`] when the item stands on no raised block.
const NO_PARENT: u8 = u8::MAX;
/// [`Item::on`] for a thing carried over the hero's head.
const ON_HERO: u8 = u8::MAX - 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Kind {
    #[default]
    Block,
    Prop,
    Coin,
    Hero,
    /// The area's extra model `i`.
    Extra,
    /// Creature `i` ([`crate::life`]).
    Critter,
}

/// The drawing's state: the renderer's working memory and the layer 2
/// list. Kept in the game's state, never on the stack.
pub struct Painter {
    pub r: Rend,
    items: [Item; MAX_ITEMS],
    extras: [Extra; MAX_EXTRAS],
}

/// What is shown over the scene besides the play: the menus, the line of
/// numbers, and the best time for the done panel.
pub struct Shown<'a> {
    pub menu: &'a Menu,
    pub areas: &'a AreaMenu,
    /// The line of numbers is shown, and steps run in this update.
    pub line: bool,
    pub steps: u8,
    /// The area's best time in steps (0 = none), and whether the last
    /// finish set it.
    pub best: u16,
    pub new_best: bool,
}

impl Default for Painter {
    fn default() -> Self {
        Self::new()
    }
}

/// A little text buffer for the counter and the line of numbers.
pub struct Text {
    b: [u8; 48],
    n: usize,
}

impl Text {
    pub const fn new() -> Text {
        Text { b: [0; 48], n: 0 }
    }

    pub fn push(&mut self, s: &str) -> &mut Text {
        for &c in s.as_bytes() {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
        self
    }

    pub fn num(&mut self, v: u32) -> &mut Text {
        let mut d = [0u8; 10];
        let (mut i, mut v) = (d.len(), v);
        loop {
            i -= 1;
            d[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 || i == 0 {
                break;
            }
        }
        for &c in &d[i..] {
            if self.n < self.b.len() {
                self.b[self.n] = c;
                self.n += 1;
            }
        }
        self
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

/// The line of numbers for these counts: `T` triangles filled, `O`
/// objects, `F` front faces and polygons, `P` pixels painted, `S` steps
/// run in this update.
pub fn line_text(st: &Stats, steps: u8) -> Text {
    let mut t = Text::new();
    t.push("T")
        .num(st.tris)
        .push(" O")
        .num(st.objects)
        .push(" F")
        .num(st.front)
        .push(" P")
        .num(st.pixels)
        .push(" S")
        .num(u32::from(steps));
    t
}

/// The DONE panel's top row and height: rows 22..45, between the HUD's
/// top rows (to 19) and the hero (his cap is at about row 50 from the
/// camera's usual place).
pub const DONE_BOX: (i16, u16) = (22, 24);

/// The DONE panel's top row when the hero's cap is in or just under
/// [`DONE_BOX`]'s rows: the bottom rows 94..117.
pub const DONE_LOW: i16 = H as i16 - 26;

/// The rows of the HUD's top line (the texts at 2..8 and their
/// shadows): a world label never goes into them, wherever it is across.
pub const LABEL_TOP: i32 = 11;

/// A screen box: left, top, right and bottom (the ends excluded).
pub type ScreenBox = (i32, i32, i32, i32);

/// The boxes the HUD draws in under the top line, for a label to keep
/// clear of: the pips, the card icon, the BEST line, the area's name
/// when the line of numbers pushes it down, the message row and the sail
/// bar. `line`: the line of numbers is shown. Unused entries are empty.
pub fn hud_boxes(play: &Play, h: &Hud, line: bool) -> [ScreenBox; 6] {
    let mut out = [(0, 0, 0, 0); 6];
    if play.area == Area::Island {
        return out;
    }
    let w = W as i32;
    if line {
        out[0] = (0, 12, 4 + text_width(h.text[0].as_str()) as i32, 19);
    }
    if h.pips > 0 {
        out[1] = (w - 9 - 7 * (i32::from(h.pips.min(10)) - 1), 11, w, 18);
    }
    if h.icon.is_some() {
        out[2] = (w - 11, 11, w, 21);
    }
    if h.best_line {
        // The widest BEST line: the longest clock (65535 steps).
        out[3] = (w - 4 - text_width("BEST 2184.5") as i32, 11, w, 19);
    }
    if h.msg.is_some() {
        out[4] = (0, 99, w, 107);
    }
    if h.low_bar.is_some() {
        out[5] = (63, 109, 97, 115);
    }
    out
}

/// Where the HUD's world label goes (left x, top y, word), if its point is
/// in front of the eye and at least partly in the picture: centred over
/// the point, moved fully into the picture under the top line, then down
/// (in the top half) or up (in the bottom half) off any of the
/// [`hud_boxes`] it would touch (draw review F5: never cut, never on the
/// HUD). `None` when no place is left.
pub fn label_place(play: &Play, h: &Hud, line: bool) -> Option<(i16, i16, &'static str)> {
    let (p, word) = h.label?;
    let sp = on_screen(play, su(p))?;
    let w = text_width(word) as i32;
    let (x, y) = (sp[0] / 16 - w / 2, sp[1] / 16 - 3);
    if !(x > -w && x < W as i32 && y > -8 && y < H as i32) {
        return None;
    }
    // The label's pixels with its shadow: x..x+w+1, y..y+6.
    let low = H as i32 - 7;
    let x = clamp(x, 1, W as i32 - 1 - w);
    let mut y = clamp(y, LABEL_TOP, low);
    let boxes = hud_boxes(play, h, line);
    for _ in 0..boxes.len() {
        let hit = boxes
            .iter()
            .find(|b| b.0 < x + w + 2 && x - 1 < b.2 && b.1 < y + 7 && y - 1 < b.3);
        match hit {
            None => return Some((x as i16, y as i16, word)),
            Some(b) if b.1 < H as i32 / 2 => y = b.3 + 1,
            Some(b) => y = b.1 - 8,
        }
        if !(LABEL_TOP..=low).contains(&y) {
            return None;
        }
    }
    None
}

/// Where the world point `p` (units) lands on the screen (sixteenths
/// of a pixel), if it is in front of the eye and not past [`FAR`].
#[inline(never)]
fn on_screen(play: &Play, p: [i32; 3]) -> Option<[i32; 2]> {
    let c = play.cam.camera().to_cam(p);
    if c[2] < Proto3d::NEAR || c[2] > FAR {
        return None;
    }
    Some(VIEW.project(c))
}

/// Where the line of numbers is drawn (its text's top left corner).
pub const LINE_AT: (i16, i16) = (2, 2);

/// The counter `n/total`.
pub fn counter_text(play: &Play) -> Text {
    let (n, total) = play.count();
    let mut t = Text::new();
    t.num(u32::from(n)).push("/").num(u32::from(total));
    t
}

/// The counter's left edge: 3 pixels from the right edge, its shadow
/// one pixel further right.
pub fn counter_x(t: &Text) -> i16 {
    W as i16 - 3 - text_width(t.as_str()) as i16
}

impl Painter {
    pub const fn new() -> Painter {
        Painter {
            r: Rend::new(),
            items: [Item {
                key: 0,
                own: 0,
                kind: Kind::Block,
                i: 0,
                on: NO_PARENT,
            }; MAX_ITEMS],
            extras: [Extra::NONE; MAX_EXTRAS],
        }
    }

    /// Draws the whole picture with the pause menu only. `line`: the line
    /// of numbers is shown; `steps`: steps run in this update.
    pub fn picture(&mut self, f: &mut Frame, play: &Play, menu: &Menu, line: bool, steps: u8) {
        let s = Shown {
            menu,
            areas: &AreaMenu::CLOSED,
            line,
            steps,
            best: 0,
            new_best: false,
        };
        self.show(f, play, &s);
    }

    /// Draws the whole picture: the scene, then the HUD, the done panel
    /// and the menus.
    pub fn show(&mut self, f: &mut Frame, play: &Play, s: &Shown) {
        self.scene(f, play);
        let st = self.r.stats;
        let mut h = Hud::default();
        play.kit.hud(&mut h);
        if let Some((x, y, word)) = label_place(play, &h, s.line) {
            shadowed(f, x, y, word);
        }
        hud_with(f, play, &h, s, s.line.then_some((&st, s.steps)));
    }

    /// The scene alone (sky and 3D), counted in `self.r.stats`.
    pub fn scene(&mut self, f: &mut Frame, play: &Play) {
        self.r.begin();
        sky(f, play.cam.pitch.saturating_add(play.cam.tilt));
        let cam = play.cam.camera();
        let ne = play.kit.extras(play, &mut self.extras).min(MAX_EXTRAS);
        let mut cx = Ctx {
            f,
            cam,
            r: &mut self.r,
            play,
            lvl: play.detail.level(),
            keys: [0; MAX_BLOCKS],
            extras: &self.extras[..ne],
            late: 0,
        };
        cx.far_islets();
        cx.floors();
        let n = cx.items(&mut self.items);
        for it in &self.items[..n] {
            cx.item(*it);
        }
    }
}

/// Sky bands above the horizon and haze below, from the pitch down.
pub fn sky(f: &mut Frame, pitch: u16) {
    let c = cos_fine(pitch).max(1);
    let hz = 60 - FOCAL * sin_fine(pitch) / c;
    let bands = [
        (i32::MIN, hz - 26, col::SKY0),
        (hz - 26, hz - 8, col::SKY1),
        (hz - 8, hz, col::SKY2),
        (hz, i32::MAX, col::HAZE),
    ];
    for (y0, y1, c) in bands {
        let (y0, y1) = (y0.max(0), y1.min(H as i32));
        if y1 > y0 {
            f.rect(0, y0 as i16, W as u16, (y1 - y0) as u16, c);
        }
    }
}

/// Text with a one-pixel shadow below and to the right.
fn shadowed(f: &mut Frame, x: i16, y: i16, s: &str) {
    f.text(x + 1, y + 1, s, col::TEXT_SHADOW);
    f.text(x, y, s, col::TEXT);
}

/// The counter, the line of numbers (with the counts and steps when
/// shown) and the pause menu (ISLAND's HUD, no area menu).
pub fn hud(f: &mut Frame, play: &Play, menu: &Menu, line: Option<(&Stats, u8)>) {
    let s = Shown {
        menu,
        areas: &AreaMenu::CLOSED,
        line: line.is_some(),
        steps: 0,
        best: 0,
        new_best: false,
    };
    hud_with(f, play, &Hud::default(), &s, line);
}

/// A time in steps as seconds with tenths (`12.3`).
pub fn time_text(t: &mut Text, steps: u16) {
    let tenths = u32::from(steps) / 3;
    t.num(tenths / 10).push(".").num(tenths % 10);
}

/// A menu box with its title and lines; line `sel` is marked.
fn menu_box(
    f: &mut Frame,
    b: (i16, i16, u16, u16),
    title: &str,
    line0: i16,
    lines: &[&str],
    sel: u8,
) {
    f.rect(b.0, b.1, b.2, b.3, col::MENU_BG);
    f.outline(b.0, b.1, b.2, b.3, col::TEXT_SHADOW);
    f.text(52, b.1 + 4, title, col::TEXT);
    for (i, name) in lines.iter().enumerate() {
        let y = line0 + i as i16 * 10;
        let on = usize::from(sel) == i;
        let c = if on { col::MENU_SEL } else { col::TEXT };
        if on {
            f.text(52, y, ">", c);
        }
        f.text(62, y, name, c);
    }
}

/// Text centred on the screen at height `y`.
fn centred(f: &mut Frame, y: i16, s: &str) {
    shadowed(f, (W as i16 - text_width(s) as i16) / 2, y, s);
}

/// Everything over the scene: ISLAND's counter or the area's HUD, the
/// line of numbers, the done panel, the pause menu and the area menu.
pub fn hud_with(f: &mut Frame, play: &Play, h: &Hud, s: &Shown, line: Option<(&Stats, u8)>) {
    if play.area == Area::Island {
        let t = counter_text(play);
        shadowed(f, counter_x(&t), 2, t.as_str());
    } else {
        let y0 = if line.is_some() { 12 } else { 2 };
        shadowed(f, 2, y0, h.text[0].as_str());
        let right = h.text[1].as_str();
        shadowed(f, counter_x(&h.text[1]), 2, right);
        for k in 0..h.pips.min(10) {
            let x = W as i16 - 8 - 7 * k as i16;
            if k < h.lit {
                f.rect(x, 12, 5, 5, col::MENU_SEL);
            }
            f.outline(x, 12, 5, 5, col::TEXT);
        }
        if let Some(full) = h.icon {
            let x = W as i16 - 10;
            if full {
                f.rect(x, 12, 8, 8, col::MENU_SEL);
            }
            f.outline(x, 12, 8, 8, col::TEXT);
        }
        if h.bar > 0 {
            f.outline(59, 3, 42, 5, col::TEXT_SHADOW);
            f.rect(
                60,
                4,
                (u16::from(h.bar) * 40 / 255).max(1),
                3,
                col::MENU_SEL,
            );
        }
        if h.best_line {
            let mut t = Text::new();
            t.push("BEST ");
            if s.best > 0 {
                time_text(&mut t, s.best);
            } else {
                t.push("--.-");
            }
            shadowed(f, counter_x(&t), 12, t.as_str());
        }
        if let Some(len) = h.low_bar {
            f.outline(64, 110, 32, 4, col::TEXT_SHADOW);
            if len > 0 {
                f.rect(65, 111, u16::from(len.min(30)), 2, col::MENU_SEL);
            }
        }
        if let Some(m) = h.msg {
            centred(f, 100, m);
        }
    }
    if let Some((st, steps)) = line {
        let t = line_text(st, steps);
        shadowed(f, LINE_AT.0, LINE_AT.1, t.as_str());
    }
    if play.done && !s.menu.open && !s.areas.open {
        // Under the HUD's top rows and over the hero (draw review F7: the
        // old panel at rows 36..81 hid him); at the bottom when his cap
        // is in or just under those rows (a camera looking up at him, as
        // on MACHINES' ledge D: repair round, defect 2).
        let (mut y, h) = DONE_BOX;
        let mut cap = su(play.hero_pos());
        cap[1] += uc(HERO_TALL);
        if on_screen(play, cap).is_some_and(|c| c[1] < (i32::from(y) + i32::from(h) + 2) * 16) {
            y = DONE_LOW;
        }
        f.rect(20, y, 120, h, col::MENU_BG);
        f.outline(20, y, 120, h, col::TEXT_SHADOW);
        centred(f, y + 2, "DONE");
        let mut t = Text::new();
        if let Some(steps) = play.kit.time() {
            t.push("TIME ");
            time_text(&mut t, steps);
            if s.best > 0 {
                t.push(if s.new_best { "  BEST!" } else { "  BEST " });
                if !s.new_best {
                    time_text(&mut t, s.best);
                }
            }
        }
        centred(f, y + 9, t.as_str());
        centred(f, y + 16, "A AGAIN  START MENU");
    }
    if s.menu.open {
        menu_box(f, MENU_BOX, "MENU", MENU_LINE0, &MENU_LINES, s.menu.sel);
    }
    if s.areas.open {
        let names = Area::ALL.map(Area::name);
        menu_box(f, AREA_BOX, "AREAS", AREA_LINE0, &names, s.areas.sel);
    }
}

/// The lit colour of a material on a face with direction `n`.
fn lit(m: u8, n: [i32; 3]) -> u8 {
    SHADE.paint::<Checked>(m, n, 0)
}

/// The dark colour of shadows and patches on a top of material `m`.
fn dark(m: u8) -> u8 {
    match m {
        mat::GRASS | mat::GRASS2 => col::DARK_GRASS,
        mat::SAND => col::DARK_SAND,
        mat::BLOCK_TOP | mat::BLOCK => col::DARK_BLOCK,
        _ => col::DARK_STONE,
    }
}

/// The rim material of a top of material `m`.
fn rim(m: u8) -> u8 {
    match m {
        mat::GRASS | mat::GRASS2 => mat::RIM_GRASS,
        mat::SAND => mat::RIM_SAND,
        mat::BLOCK_TOP | mat::BLOCK => mat::RIM_BLOCK,
        _ => mat::RIM_STONE,
    }
}

/// cm to subs (as `maths::cm`, for `i32`).
const fn sc(c: i32) -> i32 {
    c * 1024 / 25
}

/// A record's corner (cm) in game units.
fn wp(c: [i16; 3]) -> [i32; 3] {
    c.map(|v| uc(i32::from(v)))
}

/// Subs to game units.
fn su(s: [i32; 3]) -> [i32; 3] {
    s.map(crate::maths::unit)
}

/// How far (x, z) is from the eye `e` along the ground (cm): an
/// eight-sided measure (the longer leg and 3/8 of the shorter, within
/// 7 %).
fn ground_dist(x: i32, z: i32, e: [i32; 3]) -> i32 {
    let (dx, dz) = (
        x.wrapping_sub(e[0]).wrapping_abs(),
        z.wrapping_sub(e[2]).wrapping_abs(),
    );
    dx.max(dz).wrapping_add(dx.min(dz).wrapping_mul(3) / 8)
}

/// The point of a block's top over (x, z) in cm, in game units.
fn top_at(b: &Block, x: i32, z: i32) -> [i32; 3] {
    [uc(x), crate::maths::unit(b.surface(sc(x), sc(z))), uc(z)]
}

/// The point of a block's top over (x, z) in subs, kept on the top, in
/// game units.
fn top_at_sub(b: &Block, x: i32, z: i32) -> [i32; 3] {
    let bb = b.bounds();
    let (x, z) = (clamp(x, bb.lo[0], bb.hi[0]), clamp(z, bb.lo[2], bb.hi[2]));
    su([x, b.surface(x, z), z])
}

/// Whether a block is drawn in layer 2 (its top is above 0, or it is
/// marked [`flag::UP`]).
fn raised(b: &Block) -> bool {
    b.hi[1] > 0 || b.flags & flag::UP != 0
}

/// Unit-length (16384) direction of `v`.
fn norm(v: [i64; 3]) -> [i32; 3] {
    let l = isqrt((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as u64).max(1) as i64;
    v.map(|c| (c * i64::from(ONE14) / l) as i32)
}

/// One picture's drawing.
struct Ctx<'a> {
    f: &'a mut Frame,
    cam: Camera,
    r: &'a mut Rend,
    play: &'a Play,
    lvl: u8,
    /// Sort keys of the raised blocks (for their children).
    keys: [i32; MAX_BLOCKS],
    /// The area's extra models for this picture.
    extras: &'a [Extra],
    /// Bit `i`: floor `i` is late, drawn in layer 2 after the hero
    /// (see [`Ctx::floors`]).
    late: u16,
}

const _: () = assert!(MAX_BLOCKS <= 16);

/// A block as drawn: `None` when switched off or faded away; a FADE
/// block shrunk toward its middle by `fade / 256` (cm, rounded toward
/// the middle).
fn as_drawn(b: &Block, fade: u16) -> Option<Block> {
    if !b.solid() || fade == 0 {
        return None;
    }
    if fade >= FULL {
        return Some(*b);
    }
    let mut o = *b;
    for k in 0..3 {
        let (lo, hi) = (i32::from(b.lo[k]), i32::from(b.hi[k]));
        let mid = (lo + hi) / 2;
        let f = i32::from(fade);
        o.lo[k] = (mid - (mid - lo) * f / 256) as i16;
        o.hi[k] = (mid + (hi - mid) * f / 256) as i16;
    }
    Some(o)
}

impl<'a> Ctx<'a> {
    fn blocks(&self) -> &'a [Block] {
        self.play.blocks()
    }

    /// Block `bi` as drawn now (see [`as_drawn`]).
    fn drawn(&self, bi: usize) -> Option<Block> {
        let blocks = self.blocks();
        as_drawn(&blocks[bi], self.play.cam.fade_of(blocks, bi))
    }

    /// Camera depth of a world point (game units).
    fn depth(&self, w: [i32; 3]) -> i32 {
        self.cam.to_cam(w)[2]
    }

    #[inline(never)]
    fn pt(&mut self, w: [i32; 3]) -> Pt {
        let c = self.cam.to_cam(w);
        Pt::new::<Proto3d>(c, &VIEW, &PLANES, &mut self.r.stats)
    }

    fn poly(&mut self, pts: &[&Pt], c: u8) -> bool {
        draw_poly::<Proto3d, MAX_POLY>(self.f, &VIEW, &PLANES, pts, c, &mut self.r.stats)
    }

    /// A polygon of up to 8 world points.
    fn poly_w(&mut self, w: &[[i32; 3]], c: u8) -> bool {
        let mut p = [Pt::default(); 8];
        let n = w.len().min(8);
        for k in 0..n {
            p[k] = self.pt(w[k]);
        }
        let r = [&p[0], &p[1], &p[2], &p[3], &p[4], &p[5], &p[6], &p[7]];
        self.poly(&r[..n], c)
    }

    fn obj(&mut self, model: &Model, pos: [i32; 3], yaw: u16, scale: i32) -> bool {
        let o = Object {
            model,
            pos,
            rot: Mat3::rotation_fine(yaw, 0, 0),
            scale,
            swap: (0, 0),
        };
        self.r
            .draw_in(self.f, &VIEW, &PLANES, &self.cam, &SHADE, &o, FAR)
    }

    /// A model at size `scale` (256 its own) with its own turn `rot`.
    fn obj_rot(&mut self, model: &Model, pos: [i32; 3], rot: Mat3, scale: i32) -> bool {
        let o = Object {
            model,
            pos,
            rot,
            scale,
            swap: (0, 0),
        };
        self.r
            .draw_in(self.f, &VIEW, &PLANES, &self.cam, &SHADE, &o, FAR)
    }

    /// MAX only: the model drawn once more over itself, turned by
    /// `turn` more and at size `scale`: a rounder outline for the bushes
    /// and rock points (their corners between the first copy's).
    fn again(&mut self, model: &Model, pos: [i32; 3], yaw: u16, turn: u16, scale: i32) {
        if self.lvl >= 3 {
            self.obj(model, pos, yaw.wrapping_add(turn), scale);
        }
    }

    /// Layer 0: far islets, farther first.
    fn far_islets(&mut self) {
        let props = self.play.world.props;
        let mut list = [(0i32, 0usize, 0i32); 4];
        let (mut n, mut k) = (0, 0);
        for (i, p) in props.iter().enumerate() {
            if p.what() != prop::FAR_ISLET {
                continue;
            }
            // Sizes 1.2, 1.6 and 2.0 m half widths, in list order.
            let scale = 256 * (120 + 40 * k) / 120;
            k += 1;
            if !self.play.prop_shown(i) || n == list.len() {
                continue;
            }
            list[n] = (self.depth(wp(p.at)), i, scale);
            n += 1;
        }
        list[..n].sort_unstable_by_key(|a| core::cmp::Reverse(a.0));
        for &(_, i, scale) in &list[..n] {
            self.obj(&models::FAR_ISLET, wp(props[i].at), 0, scale);
        }
    }

    /// Layer 1: the floors, farther first.
    fn floors(&mut self) {
        let blocks = self.blocks();
        let mut list = [(0i32, 0usize); MAX_BLOCKS];
        let mut n = 0;
        for (i, b) in blocks.iter().enumerate().take(MAX_BLOCKS) {
            if !raised(b) && b.solid() {
                list[n] = (self.depth(middle(b)), i);
                n += 1;
            }
        }
        // Far to near; equal depths in record order (the keys are all
        // different, so the unstable sort gives one order only).
        list[..n].sort_unstable_by_key(|a| (core::cmp::Reverse(a.0), a.1));
        let e = self.cam.pos;
        fix_order(&mut list[..n], e, |a| Some(self.floor_box(a.1)), |_| true);
        // Late floors (repair round, defect 5): a floor that may hide the
        // hero while he can hide nothing of it (a pad over him as he
        // falls or glides under it) waits for layer 2 and is drawn after
        // him; so does every later floor that may hide a late one, to keep
        // this order between the floors.
        let hero = self.hero_box();
        let mut late = 0u16;
        for &(_, i) in &list[..n] {
            let fb = self.floor_box(i);
            let mut wait =
                hero.is_some_and(|h| cannot_hide(&h, &fb, e) && !cannot_hide(&fb, &h, e));
            for &(_, j) in &list[..n] {
                if late & (1 << j) != 0 && !cannot_hide(&fb, &self.floor_box(j), e) {
                    wait = true;
                }
            }
            if wait {
                late |= 1 << i;
            } else {
                self.floor(i);
            }
        }
        self.late = late;
    }

    /// Whether block `i` is a late floor (drawn in layer 2).
    fn is_late(&self, i: usize) -> bool {
        self.late & (1 << i) != 0
    }

    /// The hero's box for the order fix: his body (the square of
    /// [`crate::tune::HALF`] round his feet, which no block enters) up to
    /// his cap; `None` when he is not drawn.
    fn hero_box(&self) -> Option<Aabb> {
        if self.play.pose() == Pose::Gone {
            return None;
        }
        let p = su(self.play.hero_pos());
        let r = crate::maths::unit(crate::tune::HALF);
        Some((
            [p[0] - r, p[1], p[2] - r],
            [p[0] + r, p[1] + uc(HERO_TALL), p[2] + r],
        ))
    }

    /// A floor's box for the order fix: the block as drawn, with the
    /// rock cones drawn under it.
    fn floor_box(&self, bi: usize) -> Aabb {
        let b = self.blocks()[bi];
        let Some(d) = self.drawn(bi) else {
            return (wp(b.lo), wp(b.hi));
        };
        let (mut lo, mut hi) = (wp(d.lo), wp(d.hi));
        for (i, p) in self.play.world.props.iter().enumerate() {
            let k = p.what();
            if (k != prop::CONE_MAIN && k != prop::CONE_ISLET)
                || !self.play.prop_shown(i)
                || p.at[1] != d.lo[1]
                || !d
                    .bounds()
                    .over(sc(i32::from(p.at[0])), sc(i32::from(p.at[2])))
            {
                continue;
            }
            let m = if k == prop::CONE_MAIN {
                models::cone_main(self.lvl)
            } else {
                &models::CONE_ISLET
            };
            let (l, h) = model_box(m, wp(p.at), u16::from(p.yaw) << 8, 256);
            for k in 0..3 {
                lo[k] = lo[k].min(l[k]);
                hi[k] = hi[k].max(h[k]);
            }
        }
        (lo, hi)
    }

    /// A layer-2 item's box for the order fix: a raised block as drawn,
    /// a late floor with its rock cones, the hero's body, or the sail on
    /// his back; `None` for everything else (it keeps its place, and
    /// the pass after the fix keeps it after its parent, see
    /// [`Item::on`]). Boxing props, coins or other extras too was tried:
    /// it made cycles that broke MACHINES' block order (MEASURED, order
    /// sweep). The sail's box crosses the hero's, so the two keep their
    /// sorted order; without it a pad that hides the gliding hero left
    /// his wings drawn over it (repair round, defect 5).
    fn item_box(&self, it: &Item) -> Option<Aabb> {
        let i = usize::from(it.i);
        match it.kind {
            Kind::Block if self.is_late(i) => Some(self.floor_box(i)),
            Kind::Block => self.drawn(i).map(|b| (wp(b.lo), wp(b.hi))),
            Kind::Hero => self.hero_box(),
            Kind::Extra => {
                let e = self.extras[i];
                core::ptr::eq(e.model, &models::SAIL)
                    .then(|| model_box(e.model, su(e.pos), e.yaw, i32::from(e.scale)))
            }
            _ => None,
        }
    }

    /// One floor: rock cone, skirt, top and what lies on it.
    fn floor(&mut self, bi: usize) {
        let Some(b) = self.drawn(bi) else { return };
        let props = self.play.world.props;
        for (i, p) in props.iter().enumerate() {
            let k = p.what();
            if (k != prop::CONE_MAIN && k != prop::CONE_ISLET)
                || !self.play.prop_shown(i)
                || p.at[1] != b.lo[1]
                || !b
                    .bounds()
                    .over(sc(i32::from(p.at[0])), sc(i32::from(p.at[2])))
            {
                continue;
            }
            let main = k == prop::CONE_MAIN;
            let m = if main {
                models::cone_main(self.lvl)
            } else {
                &models::CONE_ISLET
            };
            let yaw = u16::from(p.yaw) << 8;
            self.obj(m, wp(p.at), yaw, 256);
            // MAX: an islet's point once more, turned 45 deg and at
            // 283 * 181 / 256 = 200 cm, corners on the islet's edges; only
            // when the eye is not over the top: from there the whole point
            // is hidden under it (and would only cost pixels). Not the
            // main island's: its square top, turned, would stick out.
            let (e, lo, hi) = (self.cam.pos, wp(b.lo), wp(b.hi));
            if !main
                && !(e[1] > hi[1]
                    && (lo[0]..=hi[0]).contains(&e[0])
                    && (lo[2]..=hi[2]).contains(&e[2]))
            {
                self.again(m, wp(p.at), yaw, 8_192, 181);
            }
        }
        self.block(bi);
    }

    /// A block from its record: the faces that look at the eye, then the
    /// top with its tiles, patch, rim, shadow and ring. A SLAB ramp's
    /// sides and bottom run [`SLAB`] cm under its top.
    fn block(&mut self, bi: usize) {
        let Some(b) = self.drawn(bi) else { return };
        let (lo, hi) = (wp(b.lo), wp(b.hi));
        let mid = middle(&b);
        let half = [
            (hi[0] - lo[0]) / 2,
            (hi[1] - lo[1]) / 2,
            (hi[2] - lo[2]) / 2,
        ];
        let r = isqrt(half.iter().map(|v| (*v as u64) * (*v as u64)).sum()) as i32 + 1;
        if in_view::<Proto3d>(&PLANES, &self.cam, mid, r, FAR).is_none() {
            return;
        }
        self.r.stats.object::<Checked>();
        let e = self.cam.pos;
        // The top's height (units) at the four corners.
        let (x0, x1, z0, z1) = (
            i32::from(b.lo[0]),
            i32::from(b.hi[0]),
            i32::from(b.lo[2]),
            i32::from(b.hi[2]),
        );
        let t00 = top_at(&b, x0, z0)[1];
        let t10 = top_at(&b, x1, z0)[1];
        let t11 = top_at(&b, x1, z1)[1];
        let t01 = top_at(&b, x0, z1)[1];
        let y = lo[1];
        let slab = b.is_ramp() && b.flags & flag::SLAB != 0;
        let th = uc(SLAB);
        // The bottom under a top height.
        let under = |t: i32| if slab { (t - th).max(y) } else { y };
        let side = b.side;
        let bevelled = raised(&b) && !slab && b.top != mat::GRASS && b.flags & flag::RIM != 0;
        // -x, +x, -z, +z: bottom corners a, b and their top heights.
        let faces = [
            (
                e[0] < lo[0],
                [lo[0], lo[2]],
                [lo[0], hi[2]],
                t00,
                t01,
                [-ONE14, 0, 0],
            ),
            (
                e[0] > hi[0],
                [hi[0], hi[2]],
                [hi[0], lo[2]],
                t11,
                t10,
                [ONE14, 0, 0],
            ),
            (
                e[2] < lo[2],
                [hi[0], lo[2]],
                [lo[0], lo[2]],
                t10,
                t00,
                [0, 0, -ONE14],
            ),
            (
                e[2] > hi[2],
                [lo[0], hi[2]],
                [hi[0], hi[2]],
                t01,
                t11,
                [0, 0, ONE14],
            ),
        ];
        for (seen, a, c, ta, tc, n) in faces {
            if !seen {
                continue;
            }
            let mut w = [[0; 3]; 4];
            let mut k = 0;
            let (ya, yc) = (under(ta), under(tc));
            w[k] = [a[0], ya, a[1]];
            k += 1;
            w[k] = [c[0], yc, c[1]];
            k += 1;
            if tc > yc {
                w[k] = [c[0], tc, c[1]];
                k += 1;
            }
            if ta > ya {
                w[k] = [a[0], ta, a[1]];
                k += 1;
            }
            if k >= 3 {
                self.poly_w(&w[..k], lit(side, n));
            }
            // MAX: a bevel on a raised block that is not grass (the
            // floating block, the stones, the resting bob, the steps):
            // the top's rim colour runs [`BEVEL`] cm down the side, lit
            // as the side, so the edge reads as cut off.
            let bv = uc(BEVEL);
            if self.lvl >= 3 && bevelled && ta - bv > ya && tc - bv > yc {
                let w = [
                    [a[0], ta - bv, a[1]],
                    [c[0], tc - bv, c[1]],
                    [c[0], tc, c[1]],
                    [a[0], ta, a[1]],
                ];
                self.poly_w(&w, lit(rim(b.top), n));
            }
            // MAX: the grass hangs over the side's top edge.
            if self.lvl >= 3 && b.top == mat::GRASS && ta == tc && ya == yc {
                self.lip(a, c, ta, ta - ya, n);
            }
        }
        // The top's plane: the eye is above it when `above`.
        let p00 = [lo[0], t00, lo[2]];
        let u = [0, i64::from(t01 - t00), i64::from(hi[2] - lo[2])];
        let v = [i64::from(hi[0] - lo[0]), i64::from(t10 - t00), 0];
        // u x v points up for a top.
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let to_eye = [0, 1, 2].map(|k| i64::from(e[k] - p00[k]));
        let above = n[0] * to_eye[0] + n[1] * to_eye[1] + n[2] * to_eye[2] > 0;
        if slab {
            // The plank's underside: the top's plane, SLAB lower.
            let up = i64::from(th) * n[1];
            if !above && n[0] * to_eye[0] + n[1] * to_eye[1] + n[2] * to_eye[2] + up < 0 {
                let w = [
                    [lo[0], under(t00), lo[2]],
                    [hi[0], under(t10), lo[2]],
                    [hi[0], under(t11), hi[2]],
                    [lo[0], under(t01), hi[2]],
                ];
                let nn = norm(n.map(|v| -v));
                self.poly_w(&w, lit(side, nn));
            }
        } else if e[1] < y {
            let w = [
                [lo[0], y, lo[2]],
                [hi[0], y, lo[2]],
                [hi[0], y, hi[2]],
                [lo[0], y, hi[2]],
            ];
            self.poly_w(&w, lit(side, [0, -ONE14, 0]));
        }
        // The top, when the eye is above its plane.
        if above {
            let n = if b.is_ramp() { norm(n) } else { UP };
            self.top(bi, &b, n);
        }
    }

    /// MAX: one band of the meadow ([`MEADOW`]) on a grass top (`edge`:
    /// the tiles' x0, x1, z0, z1 in cm): flat flowers, clover and
    /// pebbles, one piece or none in each cell, chosen, placed and turned
    /// by a fixed hash of the cell and band. Flat on the top and drawn
    /// with it, before everything that stands on it, so no draw order can
    /// go wrong. Only where this top is the highest walkable one (not
    /// under a block standing on it).
    #[inline(never)]
    fn meadow(&mut self, bi: usize, b: &Block, n: [i32; 3], edge: [i32; 4], band: usize) {
        // Small sums that cannot overflow (cm within the 16-bit
        // records, the pieces' corners within 64 cm) are written
        // wrapping: the same results, less code.
        let (cell, [m, start, end, size]) = MEADOW[band];
        let blocks = self.blocks();
        let y = uc(i32::from(b.hi[1]));
        let e = self.cam.pos.map(|u| u * 25 / 64);
        // Cells counted from [`MEADOW_OFF`] cm below the records' least
        // (whole numbers only, divided by a number known not to be 0).
        let cells = |lo: i32, hi: i32, eye: i32| {
            let a = (lo + m).max(eye - end).wrapping_add(MEADOW_OFF);
            let z = (hi - m).min(eye + end).wrapping_add(MEADOW_OFF);
            a as u32 / cell..=z as u32 / cell
        };
        for cz in cells(edge[2], edge[3], e[2]) {
            for cx in cells(edge[0], edge[1], e[0]) {
                let h = models::hash(cx, cz ^ (bi as u32) << 24 ^ start as u32);
                let x = (cx.wrapping_mul(cell.get()) + (h >> 8) % cell) as i32 - MEADOW_OFF;
                let z = (cz.wrapping_mul(cell.get()) + (h >> 16) % cell) as i32 - MEADOW_OFF;
                let d = ground_dist(x, z, e);
                let mid = [uc(x), y, uc(z)];
                if d >= end
                    || d <= start
                    || !(edge[0] + m..=edge[1] - m).contains(&x)
                    || !(edge[2] + m..=edge[3] - m).contains(&z)
                    || self.depth(mid) < -uc(m)
                    || top_under(blocks, sc(x), sc(z), i32::MAX).map(|t| usize::from(t.0))
                        != Some(bi)
                {
                    continue;
                }
                // Units per cm (times 256) at this distance.
                let k = (end - d).min(d - start).min(MEADOW_RAMP) * size / MEADOW_RAMP;
                let flower = [mat::CAP, mat::GOLD, mat::HERO][(h >> 6) as usize % 3];
                self.piece(mid, h, k, flower, n);
            }
        }
    }

    /// MAX look 2: the flower patches ([`PATCHES`]) lying on top `bi`,
    /// at full size within [`LIP_FULL`] cm of the eye, smaller up to
    /// [`LIP_END`] and none past it (nothing pops).
    #[inline(never)]
    fn patches(&mut self, bi: usize, b: &Block, n: [i32; 3]) {
        let blocks = self.blocks();
        let e = self.cam.pos.map(|u| u * 25 / 64);
        for &(at, flower) in PATCHES {
            let (x, z) = (i32::from(at[0]), i32::from(at[2]));
            let d = ground_dist(x, z, e);
            if d >= LIP_END
                || i32::from(at[1]) != i32::from(b.hi[1])
                || top_under(blocks, sc(x), sc(z), i32::MAX).map(|t| usize::from(t.0)) != Some(bi)
            {
                continue;
            }
            // The flowers' size, times 256: full, then shrinking.
            let k = (LIP_END - d).min(LIP_END - LIP_FULL) * 256 / (LIP_END - LIP_FULL);
            let r = uc(DOT).wrapping_mul(k) >> 8;
            let y = uc(i32::from(at[1]));
            let c = lit(flower, n);
            for (dx, dz) in DOTS {
                let m = [uc(x + i32::from(dx)), y, uc(z + i32::from(dz))];
                if self.depth(m) < -uc(36) {
                    continue;
                }
                let w = |u: i32, v: i32| [m[0].wrapping_add(u), y, m[2].wrapping_add(v)];
                self.poly_w(&[w(0, -r), w(r, 0), w(0, r), w(-r, 0)], c);
            }
        }
    }

    /// One meadow piece ([`PIECES`], number `h % 8`, turned or mirrored
    /// by `h`'s bits 8, 16 and 32) lying flat at `mid` (game units), at
    /// `k` units per cm times 256, its flowers `flower`.
    fn piece(&mut self, mid: [i32; 3], h: u32, k: i32, flower: u8, n: [i32; 3]) {
        for &(c, corners) in PIECES[(h % 8) as usize] {
            let mut w = [mid; 6];
            for (p, q) in w.iter_mut().zip(corners) {
                // Turned or mirrored one of eight ways.
                let (u, v) = (i32::from(q[0]), i32::from(q[1]));
                let (u, v) = if h & 8 != 0 { (v, u) } else { (u, v) };
                let u = if h & 16 != 0 { u.wrapping_neg() } else { u };
                let v = if h & 32 != 0 { v.wrapping_neg() } else { v };
                p[0] = p[0].wrapping_add(u.wrapping_mul(k) >> 8);
                p[2] = p[2].wrapping_add(v.wrapping_mul(k) >> 8);
            }
            let c = if c == FLOWER { flower } else { c };
            self.poly_w(&w[..corners.len()], lit(c, n));
        }
    }

    /// MAX: grass hanging over a side's flat top edge from `a` to `c`
    /// (x and z, game units) at height `top`, the side `high` units
    /// high: a row of [`LIP`] cm teeth pointing down, 10 to 28 cm long
    /// (a fixed hash of the tooth), in the grass colour lit as the side.
    /// Drawn right after the side and inside it (no sky between). Within
    /// [`LIP_FULL`] cm of the eye at full length, shorter up to
    /// [`LIP_END`] and none past it, so nothing pops.
    #[inline(never)]
    fn lip(&mut self, a: [i32; 2], c: [i32; 2], top: i32, high: i32, n: [i32; 3]) {
        // Sums in game units within the 16-bit records, written wrapping
        // (they cannot overflow): the same results, less code.
        let e = self.cam.pos;
        let (sx, sz) = (c[0].wrapping_sub(a[0]), c[1].wrapping_sub(a[1]));
        let k = (sx.wrapping_abs().wrapping_add(sz.wrapping_abs()) / uc(LIP)).max(1);
        let colour = lit(mat::GRASS, n);
        let at = |j: i32| {
            [
                a[0].wrapping_add(sx.wrapping_mul(j) / k / 2),
                a[1].wrapping_add(sz.wrapping_mul(j) / k / 2),
            ]
        };
        for i in 0..k {
            let (p, m, q) = (at(2 * i), at(2 * i + 1), at(2 * i + 2));
            let (dx, dz) = (
                m[0].wrapping_sub(e[0]).wrapping_abs(),
                m[1].wrapping_sub(e[2]).wrapping_abs(),
            );
            let d = dx.max(dz).wrapping_add(dx.min(dz).wrapping_mul(3) / 8);
            const FULL: i32 = uc(LIP_FULL);
            const END: i32 = uc(LIP_END);
            // 10 to 28 cm (in units, 256/100 a cm), shorter past FULL.
            let h = models::hash(m[0] as u32, m[1] as u32) % 19;
            let long = (h as i32 + 10)
                .wrapping_mul(END.wrapping_sub(d).min(END - FULL))
                .wrapping_mul(64)
                / (25 * (END - FULL));
            if d < END && long > 1 && long < high {
                self.poly_w(
                    &[
                        [p[0], top, p[1]],
                        [q[0], top, q[1]],
                        [m[0], top - long, m[1]],
                    ],
                    colour,
                );
            }
        }
    }

    /// A block's top: tiles (checked on grass), the patch under a
    /// floating block, the rim, the shadow and the ring.
    fn top(&mut self, bi: usize, b: &Block, n: [i32; 3]) {
        let blocks = self.blocks();
        let b = *b;
        let (x0, x1, z0, z1) = (
            i32::from(b.lo[0]),
            i32::from(b.hi[0]),
            i32::from(b.lo[2]),
            i32::from(b.hi[2]),
        );
        // The rim: one quad in the rim colour under the whole top, then
        // the tiles inset by [`RIM`] over it (2 triangles for the rim
        // instead of 8 for four strips).
        let rimmed = b.flags & flag::RIM != 0;
        if rimmed {
            let w = [
                top_at(&b, x0, z0),
                top_at(&b, x1, z0),
                top_at(&b, x1, z1),
                top_at(&b, x0, z1),
            ];
            self.poly_w(&w, lit(rim(b.top), n));
        }
        let inset = if rimmed { RIM } else { 0 };
        let (x0, x1, z0, z1) = (x0 + inset, x1 - inset, z0 + inset, z1 - inset);
        let grass = b.top == mat::GRASS;
        // The hero's feet (cm), for MAX's finer tiles near him.
        let hp = self.play.hero_pos();
        let (hx, hz) = (hp[0] * 25 / 1024, hp[2] * 25 / 1024);
        let per = TILES[usize::from(self.lvl).min(TILES.len() - 1)];
        let count = |len: i32| {
            if grass {
                (len * per / 1400).clamp(1, 15)
            } else {
                1
            }
        };
        let (nx, nz) = (count(x1 - x0), count(z1 - z0));
        let c0 = lit(b.top, n);
        let c1 = if grass { lit(mat::GRASS2, n) } else { c0 };
        let gx = |k: i32| x0 + (x1 - x0) * k / nx;
        let gz = |k: i32| z0 + (z1 - z0) * k / nz;
        let mut prev = [Pt::default(); 16];
        let mut cur = [Pt::default(); 16];
        for kz in 0..=nz {
            for kx in 0..=nx {
                cur[kx as usize] = self.pt(top_at(&b, gx(kx), gz(kz)));
            }
            if kz > 0 {
                for kx in 0..nx {
                    let (ta, tb, tc, td) = (gx(kx), gx(kx + 1), gz(kz - 1), gz(kz));
                    if covered(blocks, bi, ta, tb, tc, td) {
                        continue;
                    }
                    let c = if (kx + kz) & 1 == 0 { c0 } else { c1 };
                    let k = kx as usize;
                    self.poly(&[&prev[k], &prev[k + 1], &cur[k + 1], &cur[k]], c);
                    // MAX: a grass tile near the hero gets the two
                    // quarters of the other colour over it, a checker
                    // of half tiles (drawn over the whole tile in the
                    // same plane, so no crack can open between them).
                    let (mx, mz) = ((ta + tb) / 2, (tc + td) / 2);
                    if self.lvl >= 3
                        && grass
                        && (mx - hx).abs() <= FINE_REACH
                        && (mz - hz).abs() <= FINE_REACH
                    {
                        let o = if c == c0 { c1 } else { c0 };
                        self.poly_w(
                            &[
                                top_at(&b, ta, tc),
                                top_at(&b, mx, tc),
                                top_at(&b, mx, mz),
                                top_at(&b, ta, mz),
                            ],
                            o,
                        );
                        self.poly_w(
                            &[
                                top_at(&b, mx, mz),
                                top_at(&b, tb, mz),
                                top_at(&b, tb, td),
                                top_at(&b, mx, td),
                            ],
                            o,
                        );
                    }
                }
            }
            core::mem::swap(&mut prev, &mut cur);
        }
        if self.lvl >= 3 && grass && !b.is_ramp() {
            for band in 0..MEADOW.len() {
                self.meadow(bi, &b, n, [x0, x1, z0, z1], band);
            }
            if !PATCHES.is_empty() && self.play.area == Area::Island {
                self.patches(bi, &b, n);
            }
        }
        // The dark patch under a floating block.
        for (pi, p) in blocks.iter().enumerate() {
            if p.flags & flag::PATCH == 0 || pi == bi {
                continue;
            }
            // Under the block as drawn (shrunk with its fade).
            let Some(p) = as_drawn(p, self.play.cam.fade_of(blocks, pi)) else {
                continue;
            };
            let pb = p.bounds();
            let (cx, cz) = ((pb.lo[0] + pb.hi[0]) / 2, (pb.lo[2] + pb.hi[2]) / 2);
            if top_under(blocks, cx, cz, pb.lo[1]).map(|t| usize::from(t.0)) != Some(bi) {
                continue;
            }
            let (a, c, d, g) = (
                i32::from(p.lo[0]),
                i32::from(p.hi[0]),
                i32::from(p.lo[2]),
                i32::from(p.hi[2]),
            );
            let w = [
                top_at(&b, a, d),
                top_at(&b, c, d),
                top_at(&b, c, g),
                top_at(&b, a, g),
            ];
            self.poly_w(&w, dark(b.top));
        }
        // The hero's shadow, or the landing ring.
        if let Some(s) = self.play.shadow
            && usize::from(s.top) == bi
        {
            let w: [[i32; 3]; 8] = core::array::from_fn(|k| disc(&b, s.spot, s.r, k));
            self.poly_w(&w, dark(b.top));
        }
        if let Some(rg) = self.play.ring
            && usize::from(rg.top) == bi
        {
            let o: [[i32; 3]; 9] = core::array::from_fn(|k| disc(&b, rg.spot, RING_OUT, k % 8));
            let i: [[i32; 3]; 9] = core::array::from_fn(|k| disc(&b, rg.spot, RING_IN, k % 8));
            for k in 0..8 {
                self.poly_w(&[o[k], o[k + 1], i[k + 1], i[k]], col::RING);
            }
        }
    }

    /// Layer 2's list, sorted far to near with the parent rule.
    fn items(&mut self, out: &mut [Item; MAX_ITEMS]) -> usize {
        let play = self.play;
        let blocks = self.blocks();
        let mut n = 0;
        let mut push = |out: &mut [Item; MAX_ITEMS], it: Item| {
            if n < MAX_ITEMS {
                out[n] = it;
                n += 1;
            }
        };
        for (i, b) in blocks.iter().enumerate().take(MAX_BLOCKS) {
            if raised(b) {
                let own = self.depth(middle(b));
                let mut key = own;
                // In the test areas a raised block standing on an earlier
                // raised block (the weight on the plank) is drawn after it.
                if play.area != Area::Island {
                    let bb = b.bounds();
                    let (x, z) = ((bb.lo[0] + bb.hi[0]) / 2, (bb.lo[2] + bb.hi[2]) / 2);
                    if let Some((t, _)) = top_under(&blocks[..i], x, z, bb.lo[1] + sc(1))
                        && raised(&blocks[usize::from(t)])
                    {
                        key = own.min(self.keys[usize::from(t)] - 1);
                    }
                }
                self.keys[i] = key;
                if self.drawn(i).is_none() {
                    continue;
                }
                push(
                    out,
                    Item {
                        key,
                        own,
                        kind: Kind::Block,
                        i: i as u8,
                        on: NO_PARENT,
                    },
                );
            }
        }
        // The hero's key, and the late floors' just nearer (they follow
        // him; what stands on them follows them, see [`Ctx::key`]).
        let hero = (play.pose() != Pose::Gone).then(|| {
            let p = play.hero_pos();
            let own = self.depth(su([p[0], p[1] + crate::tune::HERO_H / 2, p[2]]));
            let parent = play
                .shadow
                .map(|s| s.top)
                .or(play.ring.map(|r| r.top))
                .or(play.ground());
            match parent {
                Some(t) if raised(&blocks[usize::from(t)]) => {
                    (own.min(self.keys[usize::from(t)] - 1), own)
                }
                _ => (own, own),
            }
        });
        let late_key = hero.map_or(0, |h| h.0 - 1);
        for i in 0..MAX_BLOCKS {
            if self.is_late(i) {
                self.keys[i] = late_key;
            }
        }
        for (i, p) in play.world.props.iter().enumerate() {
            let k = p.what();
            let h = match k {
                prop::TREE => 110,
                prop::BUSH => 17,
                prop::TUFTS => 11,
                _ => continue,
            };
            if !play.prop_shown(i) || play.cam.scale.get(i).copied().unwrap_or(0) == 0 {
                continue;
            }
            let at = [i32::from(p.at[0]), i32::from(p.at[1]), i32::from(p.at[2])];
            let (key, own, on) = self.key(
                [uc(at[0]), uc(at[1] + h), uc(at[2])],
                [sc(at[0]), sc(at[1]) + 1, sc(at[2])],
            );
            push(
                out,
                Item {
                    key,
                    own,
                    kind: Kind::Prop,
                    i: i as u8,
                    on,
                },
            );
        }
        let bob = play.coins.bob();
        for (i, c) in play.world.coins.iter().enumerate() {
            if !play.coin_shown(i) {
                continue;
            }
            let p = c.pos();
            let (key, own, on) = self.key(su([p[0], p[1] + bob, p[2]]), p);
            push(
                out,
                Item {
                    key,
                    own,
                    kind: Kind::Coin,
                    i: i as u8,
                    on,
                },
            );
        }
        let recs = play.world.critters;
        for i in 0..recs.len().min(life::MAX_CRITTERS) {
            if !play.life.shown(recs, i) {
                continue;
            }
            let p = play.life.pos(recs, i, play.steps);
            let (key, own, on) = self.key(su([p[0], p[1] + sc(26), p[2]]), [p[0], p[1] + 1, p[2]]);
            push(
                out,
                Item {
                    key,
                    own,
                    kind: Kind::Critter,
                    i: i as u8,
                    on,
                },
            );
        }
        let hp = play.hero_pos();
        for (i, e) in self.extras.iter().enumerate() {
            let at = e.pos;
            let (key, own, on) = self.key(su(at), [at[0], at[1] + 1, at[2]]);
            // Carried over the head (MACHINES' bob): it follows the hero.
            let over_head = play.pose() != Pose::Gone
                && (at[0] - hp[0]).abs() <= sc(HERO_REACH)
                && (at[2] - hp[2]).abs() <= sc(HERO_REACH)
                && at[1] >= hp[1] + sc(HERO_TALL - 4);
            let on = if over_head { ON_HERO } else { on };
            push(
                out,
                Item {
                    key,
                    own,
                    kind: Kind::Extra,
                    i: i as u8,
                    on,
                },
            );
        }
        if let Some((key, own)) = hero {
            push(
                out,
                Item {
                    key,
                    own,
                    kind: Kind::Hero,
                    i: 0,
                    on: NO_PARENT,
                },
            );
        }
        for i in 0..MAX_BLOCKS {
            if self.is_late(i) {
                push(
                    out,
                    Item {
                        key: late_key,
                        own: late_key,
                        kind: Kind::Block,
                        i: i as u8,
                        on: NO_PARENT,
                    },
                );
            }
        }
        // Far to near; a stable sort keeps list order for equal keys,
        // except that the test areas draw the farther own depth first
        // (two things standing on the same raised block, like the hero
        // and the weight he carries).
        let ties = play.area != Area::Island;
        let before =
            |a: &Item, b: &Item| a.key < b.key || (ties && a.key == b.key && a.own < b.own);
        let list = &mut out[..n];
        for i in 1..list.len() {
            let mut j = i;
            while j > 0 && before(&list[j - 1], &list[j]) {
                list.swap(j - 1, j);
                j -= 1;
            }
        }
        // Then the order fix between the raised blocks, the late floors
        // and the hero, by their boxes: whatever lies behind another goes
        // before it. Since the repair round (defects 1 and 5) the hero is
        // one box among the others and may go before a block too: a hero
        // behind a block or under a pad is hidden by it, no longer drawn
        // over it (review F8's old order).
        let e = self.cam.pos;
        fix_order(list, e, |it| self.item_box(it), |_| true);
        // A thing standing on a raised block (or carried by the hero)
        // that the fix moved later follows it: it goes right after it.
        let mut k = 0;
        let mut moves = 0;
        while k < list.len() {
            let on = list[k].on;
            let at = match on {
                NO_PARENT => None,
                ON_HERO => list.iter().position(|b| b.kind == Kind::Hero),
                _ => list.iter().position(|b| b.kind == Kind::Block && b.i == on),
            };
            match at {
                Some(p) if p > k && moves < MAX_ITEMS => {
                    list[k..=p].rotate_left(1);
                    moves += 1;
                }
                _ => k += 1,
            }
        }
        n
    }

    /// A sort key: the depth of `mid` (units), but nearer than the
    /// raised block under `at` (subs), if any; and the depth itself.
    fn key(&self, mid: [i32; 3], at: [i32; 3]) -> (i32, i32, u8) {
        let own = self.depth(mid);
        let blocks = self.blocks();
        match top_under(blocks, at[0], at[2], at[1]) {
            Some((t, _)) if raised(&blocks[usize::from(t)]) || self.is_late(usize::from(t)) => {
                (own.min(self.keys[usize::from(t)] - 1), own, t)
            }
            _ => (own, own, NO_PARENT),
        }
    }

    fn item(&mut self, it: Item) {
        let play = self.play;
        let i = usize::from(it.i);
        match it.kind {
            Kind::Block if self.is_late(i) => self.floor(i),
            Kind::Block => self.block(i),
            Kind::Prop => {
                let p = play.world.props[i];
                let scale = i32::from(play.cam.scale[i]);
                let (m, yaw, turn) = match p.what() {
                    // MAX has its own rounder tree (no copy).
                    prop::TREE => (models::tree(self.lvl), u16::from(p.yaw) << 8, 0),
                    // An eight-sided bush.
                    prop::BUSH => (&models::BUSH, u16::from(p.yaw) << 8, 8_192),
                    // The tufts once (MAX has more patches instead).
                    _ => (&models::TUFTS, (i as u16).wrapping_mul(13_000), 0),
                };
                self.obj(m, wp(p.at), yaw, scale);
                if turn != 0 {
                    self.again(m, wp(p.at), yaw, turn, scale);
                }
            }
            Kind::Coin => {
                let p = play.world.coins[i].pos();
                let w = su([p[0], p[1] + play.coins.bob(), p[2]]);
                let m = if self.lvl == 0 {
                    &models::NUT_LOW
                } else {
                    &models::NUT
                };
                self.obj(m, w, play.coins.spin, 256);
            }
            Kind::Hero => {
                let k = match play.pose() {
                    Pose::Stand | Pose::Gone => 0,
                    Pose::Run(0) => 1,
                    Pose::Run(_) => 2,
                    Pose::Jump => 3,
                    Pose::Fall | Pose::Glide => 4,
                    Pose::Squash => 5,
                };
                let (pos, yaw) = (su(play.hero_pos()), play.hero_yaw());
                if self.lvl >= 3 && k != 5 {
                    // MAX: the rounder head as its own object. The arm
                    // farther from the eye first, the legs and body, the
                    // head and the brim (the one nearer the eye last),
                    // the nearer arm last (in the jump the arms reach up
                    // beside the head).
                    // The eye in the hero's own frame: x across, z ahead
                    // (the turn's columns are his axes in the world).
                    let m = Mat3::rotation_fine(yaw, 0, 0);
                    let e = self.cam.pos;
                    let (mut ex, mut ez) = (0i64, 0i64);
                    for j in 0..3 {
                        let d = i64::from(e[j]) - i64::from(pos[j]);
                        ex += i64::from(m.0[j][0]) * d;
                        ez += i64::from(m.0[j][2]) * d;
                    }
                    let (px, nx) = (models::HERO_ARM_PX, models::HERO_ARM_NX);
                    let (far, near) = if ex > 0 { (nx, px) } else { (px, nx) };
                    let head = models::HERO_HEAD;
                    let brim = models::hero_part(k, models::HERO_BRIM);
                    let (h0, h1) = if ez > 0 { (head, brim) } else { (brim, head) };
                    let parts = [
                        models::hero_part(k, far),
                        models::hero_part(k, models::HERO_BODY),
                        h0,
                        h1,
                        models::hero_part(k, near),
                    ];
                    for part in &parts {
                        self.obj(part, pos, yaw, 256);
                    }
                } else {
                    // LOW: one standing shape (and the squash).
                    let model = if self.lvl == 0 {
                        &models::HERO_LOW[usize::from(k == 5)]
                    } else {
                        &models::HERO[k]
                    };
                    self.obj(model, pos, yaw, 256);
                }
            }
            Kind::Extra => {
                let e = self.extras[i];
                self.obj(e.model, su(e.pos), e.yaw, i32::from(e.scale));
            }
            Kind::Critter => self.critter(i),
        }
    }

    /// Creature `i`: its model's faces for this detail; a burr rocks
    /// from side to side (a waddle, from the step count) and is drawn a
    /// third as high while squashed.
    fn critter(&mut self, i: usize) {
        let play = self.play;
        let recs = play.world.critters;
        let lvl = usize::from(self.lvl).min(3);
        let burr = recs[i].kind == life::kind::BURR;
        let (m, faces) = if burr {
            (&models::BURR, models::BURR_FACES[lvl])
        } else {
            (&models::TUFT, models::TUFT_FACES[lvl])
        };
        let (pos, yaw) = play.life.place(recs, i, play.steps);
        let mut roll = 0u16;
        if burr {
            let w = (play.steps & 15) as i32;
            roll = w
                .min(16i32.wrapping_sub(w))
                .wrapping_sub(4)
                .wrapping_mul(300) as u16;
        }
        let mut r = Mat3::rotation_fine(yaw, 0, roll);
        if burr && play.life.c[i].t > 0 {
            for v in &mut r.0[1] {
                *v /= 3;
            }
        }
        let pos = su(pos);
        self.obj_rot(&models::first_faces(m, faces), pos, r, life::SIZE);
    }
}

/// A block's middle in game units.
fn middle(b: &Block) -> [i32; 3] {
    let (lo, hi) = (wp(b.lo), wp(b.hi));
    [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2)
}

/// A box in game units (lo, hi) for the order fix.
type Aabb = ([i32; 3], [i32; 3]);

/// How far the hero's model reaches from his middle line (cm: the arms
/// held out when falling, 32 cm, turned; it tells a thing carried over
/// his head), and how tall it is (the cap's top at 126 cm). The order fix
/// boxes him by his body instead ([`Ctx::hero_box`]): with the reach, a
/// hero within 36 cm of a wall crossed its box and the wall's middle
/// decided, so a tall wall behind him could be drawn over him (repair
/// round, defect 1). Only his arms, when falling against a wall, reach
/// out of the body box, by at most 7 cm.
const HERO_REACH: i32 = 36;
const HERO_TALL: i32 = 128;

/// The most boxes one order fix sorts: every floor, or every item of
/// layer 2. More boxes than this keep their places (never out of range).
const FIX_MAX: usize = if MAX_ITEMS > MAX_BLOCKS + 1 {
    MAX_ITEMS
} else {
    MAX_BLOCKS + 1
};
const _: () = assert!(FIX_MAX <= 64);

/// Whether `a` can never hide any part of `b` from the eye `e`: a plane
/// between the boxes (a face plane of `a` or of `b`) has the eye on
/// `b`'s side. Rays from the eye to `b` then stay on that side, and `a`
/// is wholly on the other. Boxes that share a face overlap by up to
/// [`TOUCH`] units there (the model boxes' rounding margin); that much
/// still counts as a plane between them (a sliver under 1 cm, under one
/// pixel from any eye the camera allows).
fn cannot_hide(a: &Aabb, b: &Aabb, e: [i32; 3]) -> bool {
    (0..3).any(|k| {
        (a.1[k] <= b.0[k] + TOUCH && e[k] > a.1[k]) || (b.1[k] <= a.0[k] + TOUCH && e[k] < a.0[k])
    })
}

/// The overlap (game units) two boxes that share a face may have.
const TOUCH: i32 = 2;

/// The order fix (draw review F2, F3, F4). A list sorted far to near by
/// middle depth can still have a box drawn after a box it lies behind
/// (a big block's middle nearer than the small thing in front of it).
/// The boxes in the list (those `boxed` gives one) are put in a new
/// order in the same places:
///
/// * where a plane between two boxes shows that one can hide nothing of
///   the other but the other may hide it, the first goes first, if it
///   `may_go_first` past the other; otherwise the sorted order stays;
/// * where neither can hide the other, either order is right;
/// * where no plane parts them (touching or crossing boxes), the sorted
///   order stays.
///
/// Within those rules the sorted order is kept as far as it can be (a
/// ring of rules is broken at its first box in the sorted order). With
/// every rule agreeing with the sorted order, nothing moves.
fn fix_order<T: Copy>(
    list: &mut [T],
    e: [i32; 3],
    boxed: impl Fn(&T) -> Option<Aabb>,
    may_go_first: impl Fn(&T) -> bool,
) {
    let mut at = [0usize; FIX_MAX];
    let mut bx = [([0i32; 3], [0i32; 3]); FIX_MAX];
    let mut n = 0;
    for (k, t) in list.iter().enumerate() {
        if n == FIX_MAX {
            break;
        }
        if let Some(b) = boxed(t) {
            at[n] = k;
            bx[n] = b;
            n += 1;
        }
    }
    if n < 2 {
        return;
    }
    // need[b] bit a: box a must be drawn before box b.
    let mut need = [0u64; FIX_MAX];
    for a in 0..n {
        for b in a + 1..n {
            let a_safe = cannot_hide(&bx[a], &bx[b], e);
            let b_safe = cannot_hide(&bx[b], &bx[a], e);
            if b_safe && !a_safe && may_go_first(&list[at[b]]) {
                need[a] |= 1 << b;
            } else if !(a_safe && b_safe) {
                need[b] |= 1 << a;
            }
        }
    }
    // Kahn's order, the earliest sorted box first among the free ones (a
    // ring of rules is broken at its earliest box).
    let mut order = [0usize; FIX_MAX];
    let mut placed = 0u64;
    for slot in order.iter_mut().take(n) {
        let open = |c: usize| placed & (1 << c) == 0;
        let free = (0..n).find(|&c| open(c) && need[c] & !placed == 0);
        let c = free.unwrap_or_else(|| (0..n).find(|&c| open(c)).unwrap_or(0));
        placed |= 1 << c;
        *slot = c;
    }
    let mut was = [list[at[0]]; FIX_MAX];
    for k in 0..n {
        was[k] = list[at[k]];
    }
    for k in 0..n {
        list[at[k]] = was[order[k]];
    }
}

/// The box (game units) a model turned by `yaw` at `pos` takes, over
/// both turning directions (the cones are the same either way).
fn model_box(m: &Model, pos: [i32; 3], yaw: u16, scale: i32) -> Aabb {
    let (s, c) = (sin_fine(yaw), cos_fine(yaw));
    let mut lo = pos;
    let mut hi = pos;
    for v in m.verts {
        let [x, y, z] = v.map(|a| i32::from(a) * scale / 256);
        for (px, pz) in [
            ((x * c + z * s) / ONE14, (z * c - x * s) / ONE14),
            ((x * c - z * s) / ONE14, (z * c + x * s) / ONE14),
        ] {
            let w = [pos[0] + px, pos[1] + y, pos[2] + pz];
            for k in 0..3 {
                lo[k] = lo[k].min(w[k] - 1);
                hi[k] = hi[k].max(w[k] + 1);
            }
        }
    }
    (lo, hi)
}

/// Whether the tile x0..x1, z0..z1 (cm) of block `bi`'s top lies fully
/// under another block standing on that top.
fn covered(blocks: &[Block], bi: usize, x0: i32, x1: i32, z0: i32, z1: i32) -> bool {
    let top = blocks[bi].hi[1];
    blocks.iter().enumerate().any(|(j, o)| {
        j != bi
            && o.solid()
            && o.flags & flag::FADE == 0
            && o.lo[1] == top
            && i32::from(o.lo[0]) <= x0
            && i32::from(o.hi[0]) >= x1
            && i32::from(o.lo[2]) <= z0
            && i32::from(o.hi[2]) >= z1
    })
}

/// Corner `k` of an 8-sided disc of radius `r` round `spot` (subs) on
/// block `b`'s top, kept on the top.
fn disc(b: &Block, spot: [i32; 3], r: i32, k: usize) -> [i32; 3] {
    let a = (k as u16).wrapping_mul(8192);
    let x = spot[0] + sin_fine(a) * r / ONE14;
    let z = spot[2] + cos_fine(a) * r / ONE14;
    top_at_sub(b, x, z)
}
