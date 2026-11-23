//! The 3D scene, far to near with no depth buffer (the spec's section
//! 3, drawing steps 2 to 5).
//!
//! **The drawing order (the rule the tests check).** The land is drawn
//! in the order of one fixed table of 16 m block offsets from the
//! camera's block, the largest squared distance first ([`crate::order`]).
//! A block within `fine_blocks` of the camera's block (counted as the
//! larger of the two offsets) is drawn as its four 8 m cells, the
//! farthest cell first (squared distance from the eye to the cell's
//! middle; equal ones in row order), and in a cell the farther of its
//! two triangles first. A block farther out is drawn as two 16 m
//! triangles. Each drawn piece of land is one *unit*: four times the
//! block's key plus the cell's rank (0 to 3 for fine cells, 3 for a whole block;
//! the key grows along the table, [`crate::order::key`]), so a bigger
//! unit is nearer.
//!
//! Everything else is an *item* with the unit after which it is drawn:
//! the largest unit of the land cells it stands on or covers (its
//! footprint), so it is drawn over the land under it and under the
//! land nearer to the eye. Items of one unit go road first, then the
//! car's shadow, then the car, then dust; items of one kind in one unit
//! go far to near. The road ribbon (4 m pieces, 8 m at LOW; 0.12 m over
//! the land)
//! counts as standing on every cell its corners touch. The car's unit
//! is also at least that of every ribbon piece within 7 m of it, so the
//! road around it is never drawn over it. Dust is drawn no earlier than
//! the car. A thing nearer to the eye than the car (for a banner: the
//! eye on one side of its line and the whole car on the other) that
//! would come before it is drawn with the car, after it and after the
//! puffs behind the thing.
//!
//! Land triangles that turn their back to the eye are left out (the eye
//! is always above the land, so they are always hidden); which way a
//! triangle turns is read from its rounded screen corners when all three
//! are on the screen, so the test agrees with the filling to the pixel. Where a block
//! of two big triangles meets a block of fine cells, the fine cells'
//! corner in the middle of the shared edge may lie off the big edge; a
//! thin upright triangle (the edge's two ends and that corner) fills
//! the gap in the big triangle's colour, so no crack shows.
//!
//! Corners are turned into the camera once per picture and kept in a
//! small store (256 slots, `ci + 37 * cj` modulo 256, see `slot`);
//! a corner whose slot holds another is worked out again, with the very
//! same result, so shared edges always meet (`SEAM_PROOF`).

use crate::camera::VIEW;
use crate::detail::Reach;
use crate::draw::{col, group, land};
use crate::land::{DIAG_BIT, LAND, Land, along_of, nearest_point, side_of, tri_corners, tri_of};
use crate::look::Look;
use crate::models::{
    BALE, BANNER, BANNER_PLAIN, CAR, CAR_PLAIN, GLOW, HUT, MARKER, MATS, POST, RAIL, ROCK, SHADES,
    SMALL_SIZE, SPRAY, TREE_SIZE, TREE4, TREE6, TREE8, TREE12,
};
use crate::order::{BLOCK, MAX_GROUP, N, NQ, STARTS, group, key};
use crate::road::{POINTS, ROAD, STEP, half_width, surface_at};
use crate::things::{COUNT, DECOR, DECOR_COUNT, THINGS, Thing, kind};
use crate::tune::{
    CELL, CELLS_X, CELLS_Z, CORNERS_X, CORNERS_Z, FORD, GRASS, GRAVEL, LOW_GRASS, M,
};
use game_3d::{
    Camera, Checked, Count, Cull, Depth, Filled, Lens, Lit, M8, Mat3, Model, Object, Planes, Pt,
    Renderer, SUB, Setup, View, cos_fine, draw_poly, faces_eye, sin_fine,
};
use game_kit::{Frame, H, W};

/// DRIVE3D's game-3d choices: checked sums (a sum that runs over stops
/// the game in a test), a near plane at 1/4 m, CAMBER's 32-bit lens,
/// back faces by the stored face directions, faces sorted by their
/// middle depth, polygons that share an edge cut at the same points,
/// sizes (trees and rocks are one model each, scaled).
pub struct D3;

impl Setup for D3 {
    type Sums = Checked;
    const NEAR: i32 = M8 / 4;
    const LENS: Lens = Lens::Fast;
    const CULL: Cull = Cull::Normal;
    const DEPTH: Depth = Depth::Middle;
    const SEAM_PROOF: bool = true;
    const SCALE: bool = true;
}

type S = Checked;

/// The planes polygons are cut by (the view's guard band).
pub const PLANES: Planes = VIEW.clip_planes_at(<D3 as Setup>::NEAR);

/// The picture's own edges, for leaving out blocks, cells and things
/// that cannot reach the picture (the guard band's planes reach twice
/// as wide and would let them through).
pub const TIGHT: Planes = View {
    area: VIEW.area,
    focal: VIEW.focal,
    guard: 1,
}
.clip_planes_at(<D3 as Setup>::NEAR);

/// Most corners of one polygon drawn here (the dust's hexagons), and
/// the room `draw_poly` needs for it after a cut (five more).
const MAX_CORNERS: usize = 6;
const P: usize = MAX_CORNERS + 6;

/// The renderer for the car and the things: 70 corners and 96
/// triangles a model at most (the car, the biggest, has 70 corners and
/// 92 faces, of which fewer than half face the eye; a face cut by the
/// view's edges can give a few more triangles; R3: was 72 and 112,
/// 16 bytes a triangle, 17 a corner; `tests/draw.rs` checks every
/// model and the sweeps check that no triangle is lost).
pub type R3 = Renderer<D3, 70, 96, P>;

/// A cell (8 m) and a block (16 m) in game-3d units, and a height step.
const CELL_U: i32 = 2048;
const HALF_BLOCK: i32 = BLOCK / 2;
const STEP_U: i32 = 64;

/// The road ribbon's lift over the land (0.12 m) and the shadow's over
/// that (5 cm), in subs.
pub const LIFT: i32 = M * 12 / 100;
const SHADOW_LIFT: i32 = M * 5 / 100;
/// The shadow is left out where the land under it bends more than this
/// (0.25 m) from flat (`Painter::shadow`).
const BENT: i32 = M / 4;

/// Pieces of the ribbon: one per 2 road points (4 m).
pub const QUADS: usize = (POINTS - 1) / 2;

/// Item numbers (the low byte of an item's key).
const ID_THING: u8 = QUADS as u8;
const ID_SHADOW: u8 = ID_THING + THINGS.len() as u8;
const ID_CAR: u8 = ID_SHADOW + 1;
const ID_PUFF: u8 = ID_CAR + 1;
/// At most 8 dust puffs (HIGH's; the rules keep `tune::PUFFS`).
const MAX_PUFFS: usize = crate::tune::PUFFS;
/// MAX's roadside things (`things::DECOR`) after the dust, so the
/// numbers before stay as they were.
const ID_DECOR: u8 = ID_PUFF + MAX_PUFFS as u8;
const MAX_ITEMS: usize = ID_DECOR as usize + DECOR_COUNT;
/// The order of items in one unit.
const SUB_ROAD: u64 = 0;
const SUB_SHADOW: u64 = 1;
const SUB_CAR: u64 = 2;
const SUB_DUST: u64 = 3;

/// The car's footprint for its unit (half its length and width, with a
/// margin), its shadow plate (half sizes), and the road it pulls in.
const FOOT_LEN: i32 = M * 22 / 10;
const FOOT_WIDE: i32 = M * 11 / 10;
const SHADOW_LEN: i32 = M * 21 / 10;
// 0.8 m: inside the wheels (0.88 m out), so no tyre bottom lies under
// the plate (at 1 m the car's tyres were drawn over a shadow that lay
// in front of them, the sweep's most common wrong-order pair once the
// dust no longer covered the car's rear: MEASURED, PC).
const SHADOW_WIDE: i32 = M * 8 / 10;
const NEAR_ROAD: i64 = 7 * M as i64;

/// What a test may record while the picture is drawn: every polygon
/// drawn with `draw_poly` (corners in camera space, game-3d units) and
/// every object drawn by the renderer, in the order they are drawn.
/// The game records nothing (`()`).
pub trait Rec {
    /// Whether `poly` also wants the polygons that cover no pixel row
    /// (their colour is then worked out for it). The game's `()` does
    /// not, so it skips their colour (the speed work).
    const ALL: bool = true;
    fn poly(&mut self, _pts: &[[i32; 3]], _colour: u8) {}
    fn object(&mut self, _o: &Object<'_>, _cam: &Camera) {}
}

impl Rec for () {
    const ALL: bool = false;
}

/// One slot of the corner store.
#[derive(Clone, Copy)]
struct Slot {
    tag: [i16; 2],
    pt: Pt,
}

const EMPTY: [i16; 2] = [i16::MIN, i16::MIN];
const NO_PT: Pt = Pt {
    c: [0; 3],
    out: 0,
    s: [0; 2],
};

/// The picture's drawing state: the renderer, the corner store and the
/// count of land pieces drawn (the line's `C`). About 10.7 KB, kept in
/// the game's state.
pub struct Painter {
    pub r: R3,
    /// Land cells and blocks drawn in the last picture.
    pub cells: u32,
    store: [Slot; 256],
}

impl Default for Painter {
    fn default() -> Self {
        Painter::new()
    }
}

/// The land as one picture draws it: fine 8 m triangles in the blocks
/// near the camera, 16 m triangles beyond. The road, its things and the
/// shadow are set on this surface, not on the fine one everywhere, so a
/// coarse block never pokes through the road lying on it.
#[derive(Clone, Copy)]
struct Ground {
    /// The camera's block.
    cb: [i32; 2],
    /// Blocks each way drawn fine.
    fine: i32,
}

impl Ground {
    /// The drawn land's height (subs) at (x, z) (subs).
    fn height(&self, x: i32, z: i32) -> i32 {
        let bs = BLOCK * 16;
        let (bi, bj) = (x.div_euclid(bs), z.div_euclid(bs));
        if (bi - self.cb[0]).abs().max((bj - self.cb[1]).abs()) <= self.fine {
            return LAND.at(x, z).height;
        }
        // A coarse block: the same planes as land.rs's cells, twice
        // the size, from the corners the picture uses.
        let (i0, j0) = (bi * 2, bj * 2);
        let diag = cell_byte(i0, j0).is_some_and(|b| b & DIAG_BIT != 0);
        let (fx, fz) = (x - bi * bs, z - bj * bs);
        let tri = tri_of(diag, fx / 2, fz / 2);
        let q = |di: i32, dj: i32| i64::from(corner_h(i0 + 2 * di, j0 + 2 * dj)) * 16;
        let (fx, fz, c) = (i64::from(fx), i64::from(fz), i64::from(bs));
        let (base, num) = match (diag, tri) {
            (false, 0) => (q(0, 0), (q(1, 0) - q(0, 0)) * fx + (q(1, 1) - q(1, 0)) * fz),
            (false, _) => (q(0, 0), (q(1, 1) - q(0, 1)) * fx + (q(0, 1) - q(0, 0)) * fz),
            (true, 0) => (q(0, 0), (q(1, 0) - q(0, 0)) * fx + (q(0, 1) - q(0, 0)) * fz),
            (true, _) => (
                q(1, 1),
                (q(0, 1) - q(1, 1)) * (c - fx) + (q(1, 0) - q(1, 1)) * (c - fz),
            ),
        };
        (base + num.div_euclid(c)) as i32
    }
}

/// What one picture needs to know, worked out once.
struct Pic {
    ground: Ground,
    cam: Camera,
    /// The eye in game-3d units.
    eye: [i32; 3],
    /// The camera's block.
    cb: [i32; 2],
    fine: i32,
    /// Where the land ends, game-3d units.
    far: i32,
    /// The first entry of the order's quarter this level draws.
    start: usize,
    /// The far edge in blocks, squared: a block is drawn when its
    /// squared offset is at most this (the entries from `start` on).
    far_b2: i32,
    lit: Lit<true>,
    /// The walk's first test of a block.
    ahead: Ahead,
}

/// A corner's height (game-3d units); corners off the land take the
/// nearest edge's.
fn corner_h(ci: i32, cj: i32) -> i32 {
    let i = ci.clamp(0, CORNERS_X as i32 - 1) as usize;
    let j = cj.clamp(0, CORNERS_Z as i32 - 1) as usize;
    i32::from(LAND.corner(i, j)) * STEP_U
}

/// Blocks on the land each way.
const BLOCKS_X: usize = CELLS_X / 2;
const BLOCKS_Z: usize = CELLS_Z / 2;

/// Blocks in the height table each way: the land's, and one ring of
/// blocks round it.
const RING_X: usize = BLOCKS_X + 2;
const RING_Z: usize = BLOCKS_Z + 2;

/// The lowest and highest of the nine corner heights (height steps) of
/// each land block and of each block of the ring round the land (its
/// corners clamped to the land's edge, as [`corner_h`] clamps them),
/// made from `LAND` while compiling (the speed work: one read instead
/// of nine `corner_h` calls, on the land and off it).
///
/// Any block off the land has the same nine clamped corners as the ring
/// block it is clamped to: for `bi <= -1` its three corner columns
/// `2 bi`, `2 bi + 1` and `2 bi + 2` are all at most 0, so all clamp to
/// 0, as for `bi = -1`; for `bi >= BLOCKS_X` they are all at least
/// `2 BLOCKS_X = CORNERS_X - 1`, so all clamp to that, as for
/// `bi = BLOCKS_X`; the same for `bj`. `tests/speed_proofs.rs` checks
/// [`block_lo_hi`] against the corners far round the land.
static BLOCK_LH: [[u8; 2]; RING_X * RING_Z] = block_table(&LAND);

/// `v` clamped to `0..=hi`, as an index (a `const fn`; `Ord::clamp`
/// is not one).
const fn clamped(v: i32, hi: usize) -> usize {
    if v < 0 {
        0
    } else if v as usize > hi {
        hi
    } else {
        v as usize
    }
}

const fn block_table(land: &Land) -> [[u8; 2]; RING_X * RING_Z] {
    let mut t = [[0u8; 2]; RING_X * RING_Z];
    let mut b = 0;
    while b < RING_X * RING_Z {
        let bi = (b % RING_X) as i32 - 1;
        let bj = (b / RING_X) as i32 - 1;
        let (mut lo, mut hi) = (u8::MAX, 0u8);
        let mut k = 0;
        while k < 9 {
            let ci = clamped(bi * 2 + k % 3, CORNERS_X - 1);
            let cj = clamped(bj * 2 + k / 3, CORNERS_Z - 1);
            let h = land.h[cj * CORNERS_X + ci];
            if h < lo {
                lo = h;
            }
            if h > hi {
                hi = h;
            }
            k += 1;
        }
        t[b] = [lo, hi];
        b += 1;
    }
    t
}

const _: () = {
    // The ring's clamp of block columns is the corners' clamp.
    assert!(CORNERS_X - 1 == 2 * BLOCKS_X && CORNERS_Z - 1 == 2 * BLOCKS_Z);
};

/// The lowest and highest corner height of block (bi, bj) (game-3d
/// units), from [`BLOCK_LH`]: the same as [`block_lo_hi_slow`] for
/// every block (see [`BLOCK_LH`]).
pub fn block_lo_hi(bi: i32, bj: i32) -> (i32, i32) {
    let i = (bi.clamp(-1, BLOCKS_X as i32) + 1) as usize;
    let j = (bj.clamp(-1, BLOCKS_Z as i32) + 1) as usize;
    let [lo, hi] = BLOCK_LH[j * RING_X + i];
    (i32::from(lo) * STEP_U, i32::from(hi) * STEP_U)
}

/// [`block_lo_hi`] from the nine corners.
pub fn block_lo_hi_slow(bi: i32, bj: i32) -> (i32, i32) {
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    for j in 0..3 {
        for i in 0..3 {
            let h = corner_h(bi * 2 + i, bj * 2 + j);
            lo = lo.min(h);
            hi = hi.max(h);
        }
    }
    (lo, hi)
}

/// The lowest `lo`, the highest `hi` and the widest `hi - lo` (height
/// steps) over every entry of [`BLOCK_LH`], so over every block on the
/// land or off it (see there), made while compiling.
static SPAN: [u8; 3] = span(&block_table(&LAND));

const fn span(t: &[[u8; 2]; RING_X * RING_Z]) -> [u8; 3] {
    let mut s = [u8::MAX, 0, 0];
    let mut b = 0;
    while b < t.len() {
        let [lo, hi] = t[b];
        if lo < s[0] {
            s[0] = lo;
        }
        if hi > s[1] {
            s[1] = hi;
        }
        if hi - lo > s[2] {
            s[2] = hi - lo;
        }
        b += 1;
    }
    s
}

/// The land walk's first test (the speed work): the camera's forward
/// row bounded over all heights a block can have, worked out once a
/// picture, so a block that is surely behind the camera or past the
/// far edge costs two multiplies instead of the height table and
/// [`seen`].
///
/// Why it never rejects a block [`seen`] would let through. For block
/// (bi, bj) = camera's block (cb) + (di, dj), `block_seen` asks
/// `seen(cam, c, r, far)` with `c = (bi BLOCK + HALF_BLOCK, (lo + hi) / 2,
/// bj BLOCK + HALF_BLOCK)` and `r = 2897 + (hi - lo) / 2 + 1`. There
/// `z = floor(S / 2^14)`, S = m20 rx + m21 ry + m22 rz exact in 64 bits,
/// with rx = di BLOCK + (cb0 BLOCK + HALF_BLOCK - p0) and rz likewise:
/// so S = di a + dj b + base + m21 ry, with a = m20 BLOCK, b = m22
/// BLOCK. `(lo + hi) / 2` lies in [Y_LO, Y_HI] (the [`SPAN`] ends; the
/// heights are not negative, so the division rounds down), and m21 ry
/// is linear in it, so S lies in [s + k_lo, s + k_hi] with s = di a +
/// dj b, k_lo = base + min(y0, y1), k_hi = base + max(y0, y1), y0 =
/// m21 (Y_LO - p1), y1 = m21 (Y_HI - p1). And 0 <= r <= R_MAX = 2897 +
/// widest / 2 + 1.
/// * Behind: if s + k_hi < (NEAR - R_MAX) 2^14, then z <= NEAR - R_MAX
///   - 1, so z + r < NEAR: `seen` says no.
/// * Past the far edge: if s + k_lo >= (far + R_MAX + 1) 2^14, then
///   z >= far + R_MAX + 1, so z - r > far: `seen` says no.
///
/// The fields hold `k_hi - (NEAR - R_MAX) 2^14` and `k_lo - (far + R_MAX
/// + 1) 2^14`, so the test is two signs. The sums in [`Ahead::out`]
/// cannot run over: |di|, |dj| <= 128 (the order table is `i8`), |a|,
/// |b| <= 2^26 (matrix entries at most 2^14, `BLOCK` 2^12), so |s| <
/// 2^34, and the fields are below 2^47 (made with checked sums).
/// `tests/speed_proofs.rs` checks it on random cameras, and
/// `tests/speed_counts.rs` on every block of its drives.
#[derive(Clone, Copy, Debug)]
pub struct Ahead {
    a: i32,
    b: i32,
    near: i64,
    far: i64,
}

impl Ahead {
    /// The bounds for camera `cam` in block `cb`, land ending at `far`.
    pub fn new(cam: &Camera, cb: [i32; 2], far: i32) -> Ahead {
        let m = &cam.rot.0;
        let p = cam.pos;
        let wide = |a: i32, b: i32| i64::from(a) * i64::from(b);
        let base = wide(m[2][0], cb[0] * BLOCK + HALF_BLOCK - p[0])
            + wide(m[2][2], cb[1] * BLOCK + HALF_BLOCK - p[2]);
        let [lo, hi, widest] = SPAN;
        let y0 = wide(m[2][1], i32::from(lo) * STEP_U - p[1]);
        let y1 = wide(m[2][1], i32::from(hi) * STEP_U - p[1]);
        let r_max = 2_897 + i32::from(widest) * STEP_U / 2 + 1;
        Ahead {
            a: m[2][0] * BLOCK,
            b: m[2][2] * BLOCK,
            near: base + y0.max(y1) - (i64::from(<D3 as Setup>::NEAR - r_max) << 14),
            far: base + y0.min(y1) - (i64::from(far + r_max + 1) << 14),
        }
    }

    /// Whether the block (di, dj) from the camera's block is surely out
    /// of the picture (behind the camera or past the far edge).
    #[inline(always)]
    pub fn out(&self, di: i32, dj: i32) -> bool {
        let s = (i64::from(di) * i64::from(self.a)).wrapping_add(i64::from(dj) * i64::from(self.b));
        s.wrapping_add(self.near) < 0 || s.wrapping_add(self.far) >= 0
    }
}

/// Whether a ball (middle `c`, radius `r`, world units) can reach the
/// picture: the same answer as game-3d's
/// `in_view::<D3>(&TIGHT, cam, c, r, far).is_some()` (the speed work;
/// `tests/speed_proofs.rs` checks it).
///
/// The camera's forward row is worked out first, so a ball behind the
/// camera or past the far edge costs one row instead of three. The rows
/// and the side sums are 64-bit as in game-3d, without overflow checks
/// on the 64-bit adds: with matrix entries below 2^24 (a camera's are at
/// most 16384) each product is below 2^55, so no sum can run over and
/// every value is game-3d's. The 32-bit steps (`c - cam.pos`, `z + r`,
/// `z - r`) keep their checks, as in game-3d.
pub fn seen(cam: &Camera, c: [i32; 3], r: i32, far: i32) -> bool {
    let rel = [c[0] - cam.pos[0], c[1] - cam.pos[1], c[2] - cam.pos[2]];
    let wide = |a: i32, b: i32| i64::from(a) * i64::from(b);
    let m = &cam.rot.0;
    let row = |k: usize| -> i32 {
        (wide(m[k][0], rel[0])
            .wrapping_add(wide(m[k][1], rel[1]))
            .wrapping_add(wide(m[k][2], rel[2]))
            >> 14) as i32
    };
    let z = row(2);
    if z + r < <D3 as Setup>::NEAR || z - r > far {
        return false;
    }
    let k = [row(0), row(1), z];
    // By reference: zipping `TIGHT.len` by value copied the whole
    // array on every call (the speed work).
    for (pl, len) in TIGHT.p.iter().zip(TIGHT.len.iter()).skip(1) {
        let side = wide(pl[0], k[0])
            .wrapping_add(wide(pl[1], k[1]))
            .wrapping_add(wide(pl[2], k[2]))
            .wrapping_add(i64::from(pl[3]));
        if side < (-i64::from(r)).wrapping_mul(*len) {
            return false;
        }
    }
    true
}

/// A corner in the world, game-3d units.
fn corner_world(ci: i32, cj: i32) -> [i32; 3] {
    [ci * CELL_U, corner_h(ci, cj), cj * CELL_U]
}

/// A cell's byte, or None off the land.
fn cell_byte(ci: i32, cj: i32) -> Option<u8> {
    if (0..CELLS_X as i32).contains(&ci) && (0..CELLS_Z as i32).contains(&cj) {
        Some(LAND.cell_byte(ci as usize, cj as usize))
    } else {
        None
    }
}

/// The colour group of a land surface.
fn land_group(surface: u8) -> u8 {
    match surface {
        GRAVEL => group::VERGE,
        FORD => group::WATER,
        LOW_GRASS => group::LOW_GRASS,
        _ => group::GRASS,
    }
}

/// The light step (0 bright, 1 middle, 2 dark) of a triangle (world,
/// game-3d units): its upward direction against the light (from above,
/// to the right and behind the start, `(40, 80, -45)`). Flat land is
/// the middle step.
///
/// In 32 bits up to the dot product (the speed work; the same numbers
/// as the 64-bit sums it replaced): every side of a drawn triangle is
/// below [`LIGHT_SIDE_MAX`] each way (land: at most 2 cells, 4096,
/// across and 255 height steps, 16320, up; road pieces: checked by
/// `tests/speed_proofs.rs`), so each product is below 2^30, each
/// difference below 2^31, the normal below 2^23 and the dot product
/// below 165 * 2^23 < 2^31. Only the squares are 64-bit (below 2^62).
/// A side outside that range stops the game (checked sums), it never
/// gives another colour. A `const fn`, so the land's tables
/// ([`LIGHT_FINE`], [`LIGHT_COARSE`]) are made by this very code.
pub const fn light_step(a: [i32; 3], b: [i32; 3], c: [i32; 3]) -> u8 {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let mut n = [
        (u[1] * v[2] - u[2] * v[1]) >> 8,
        (u[2] * v[0] - u[0] * v[2]) >> 8,
        (u[0] * v[1] - u[1] * v[0]) >> 8,
    ];
    if n[1] < 0 {
        n = [-n[0], -n[1], -n[2]];
    }
    let dot = 40 * n[0] + 80 * n[1] - 45 * n[2];
    // `as i64` widens as `i64::from` does (not a `const fn`).
    let n2 = n[0] as i64 * n[0] as i64 + n[1] as i64 * n[1] as i64 + n[2] as i64 * n[2] as i64;
    let dot2 = dot as i64 * dot as i64;
    if dot > 0 && dot2 > 6806 * n2 {
        0
    } else if dot < 0 || dot2 < 6006 * n2 {
        2
    } else {
        1
    }
}

/// The largest side (each way, game-3d units) [`light_step`] takes
/// (exclusive).
pub const LIGHT_SIDE_MAX: i32 = 1 << 15;

/// The light step of each land triangle (the speed work): it depends
/// only on the fixed land, never on the camera, so it is worked out
/// while compiling, by [`light_step`] itself on the same three world
/// corners in the same order as the picture passes them. Two bits a
/// triangle: entry `n` in byte `n / 4`, bits `2 (n % 4)`.
///
/// Fine: triangle `t` of cell (ci, cj) on the land is entry
/// `(cj CELLS_X + ci) 2 + t`, corners `tri_corners(diag, t)` from the
/// cell's corner. Coarse: triangle `t` of block (bi, bj) on the land is
/// entry `(bj BLOCKS_X + bi) 2 + t`, corners twice as far from (2 bi,
/// 2 bj), its diagonal that of cell (2 bi, 2 bj). Off the land the
/// picture still uses `light_step`. `tests/speed_proofs.rs` checks every
/// entry against `light_step` on `corner_world`'s corners.
static LIGHT_FINE: [u8; CELLS_X * CELLS_Z * 2 / 4] =
    light_table::<{ CELLS_X * CELLS_Z * 2 / 4 }>(&LAND, 1);
/// See [`LIGHT_FINE`].
static LIGHT_COARSE: [u8; BLOCKS_X * BLOCKS_Z * 2 / 4] =
    light_table::<{ BLOCKS_X * BLOCKS_Z * 2 / 4 }>(&LAND, 2);

/// [`LIGHT_FINE`] (`size` 1: cells) or [`LIGHT_COARSE`] (`size` 2:
/// blocks of 2 x 2 cells).
const fn light_table<const B: usize>(land: &Land, size: usize) -> [u8; B] {
    let (nx, nz) = (CELLS_X / size, CELLS_Z / size);
    assert!(nx * nz * 2 == B * 4);
    let mut table = [0u8; B];
    let mut n = 0;
    while n < nx * nz * 2 {
        let (q, t) = (n / 2, n % 2);
        let (i0, j0) = ((q % nx) * size, (q / nx) * size);
        let diag = land.cell[j0 * CELLS_X + i0] & DIAG_BIT != 0;
        let at = tri_corners(diag, t);
        let mut w = [[0i32; 3]; 3];
        let mut k = 0;
        while k < 3 {
            let (i, j) = (i0 + size * at[k].0, j0 + size * at[k].1);
            w[k] = [
                i as i32 * CELL_U,
                land.h[j * CORNERS_X + i] as i32 * STEP_U,
                j as i32 * CELL_U,
            ];
            k += 1;
        }
        table[n / 4] |= light_step(w[0], w[1], w[2]) << (2 * (n % 4));
        n += 1;
    }
    table
}

/// The light step of triangle `t` of land cell (ci, cj) (on the land).
pub fn fine_light(ci: usize, cj: usize, t: usize) -> u8 {
    let n = (cj * CELLS_X + ci) * 2 + t;
    (LIGHT_FINE[n / 4] >> (2 * (n % 4))) & 3
}

/// The light step of triangle `t` of land block (bi, bj) (on the land).
pub fn coarse_light(bi: usize, bj: usize, t: usize) -> u8 {
    let n = (bj * BLOCKS_X + bi) * 2 + t;
    (LIGHT_COARSE[n / 4] >> (2 * (n % 4))) & 3
}

/// Largest camera-space value (each way, exclusive) [`cam_pt`] takes:
/// the sum of a plane's three sizes times this stays below 2^31. The
/// drawn world is far smaller (the land and the order table's reach
/// span under 2^18 units; `tests/speed_proofs.rs`).
pub const CAM_MAX: i32 = {
    let mut most = 1;
    let mut k = 0;
    while k < 5 {
        let p = PLANES.p[k];
        let s = p[0].abs() + p[1].abs() + p[2].abs() + p[3].abs();
        if s > most {
            most = s;
        }
        k += 1;
    }
    i32::MAX / most
};

/// A corner in camera space with its planes and screen place: the same
/// `Pt`, and the same counts, as game-3d's `Pt::new::<D3>` with
/// [`PLANES`] (the speed work). The five plane sums are written out in
/// 32 bits (game-3d's are 64-bit, one call each): with every value
/// below [`CAM_MAX`] they are exact, so each sign, and so `out`, is the
/// same. A value past it stops the game (checked sums).
pub fn cam_pt<K: Count>(c: [i32; 3], st: &mut K) -> Pt {
    const PL: [[i32; 4]; 5] = PLANES.p;
    let side = |k: usize| PL[k][0] * c[0] + PL[k][1] * c[1] + PL[k][2] * c[2] + PL[k][3] < 0;
    let out = u8::from(side(0))
        | (u8::from(side(1)) << 1)
        | (u8::from(side(2)) << 2)
        | (u8::from(side(3)) << 3)
        | (u8::from(side(4)) << 4);
    let s = if out == 0 {
        st.projected::<S>();
        D3::project(&VIEW, c)
    } else {
        [0, 0]
    };
    st.verts::<S>(1);
    Pt { c, out, s }
}

/// Whether a polygon can paint anything: false when `cull` is set and
/// its back is to the eye, or when it lies wholly beside the picture
/// (`poly`'s first two tests, made before its colour is worked out).
fn shown(p: &[&Pt], cull: bool) -> bool {
    if cull && !faces_screen(p) {
        return false;
    }
    if off_screen(&p[..p.len().min(MAX_CORNERS)]) {
        return false;
    }
    true
}

/// Fills a polygon that is [`shown`] (corners in camera space), its
/// colour worked out only when it is needed.
///
/// A polygon wholly inside the planes that covers no pixel row
/// ([`no_rows`]) is not handed to `draw_poly` (the speed work): it is
/// counted as `draw_poly` would count it (in front, and each of its
/// triangles filled with no pixel), and the frame is not touched, as
/// the filler would not touch it.
fn fill<R: Rec, K: Count>(
    f: &mut Frame,
    st: &mut K,
    p: &[&Pt],
    colour: impl FnOnce() -> u8,
    rec: &mut R,
) {
    let p = &p[..p.len().min(MAX_CORNERS)];
    if counted_empty(st, p) {
        if R::ALL {
            record(p, colour(), rec);
        }
        return;
    }
    let colour = colour();
    if draw_poly::<D3, P>(f, &VIEW, &PLANES, p, colour, st) {
        record(p, colour, rec);
    }
}

/// When `p` covers no pixel row ([`no_rows`]), counts it as game-3d's
/// `draw_poly` would (one front polygon, one empty fill per fan
/// triangle) and says so; the caller then skips the colour and the
/// filler. Not inlined: one copy serves every caller.
#[inline(never)]
fn counted_empty<K: Count>(st: &mut K, p: &[&Pt]) -> bool {
    if !no_rows(p) {
        return false;
    }
    st.front::<S>();
    for _ in 2..p.len() {
        st.filled::<S>(Filled::default());
    }
    true
}

/// Hands a drawn polygon to the recorder.
fn record<R: Rec>(p: &[&Pt], colour: u8, rec: &mut R) {
    let mut cs = [[0i32; 3]; MAX_CORNERS];
    for (c, q) in cs.iter_mut().zip(p) {
        *c = q.c;
    }
    rec.poly(&cs[..p.len()], colour);
}

/// True when a polygon (at least three corners) lies wholly inside the
/// planes and no pixel row's middle lies within its height on the
/// screen. game-3d's filler then paints no pixel and visits no row for
/// any triangle of it: each triangle's rows (`first_row` of its top to
/// `first_row` of its bottom, within the picture) lie within the
/// polygon's, and those are none. `tests/speed_proofs.rs` checks it
/// against the filler.
pub fn no_rows(p: &[&Pt]) -> bool {
    if p.len() < 3 || p.iter().any(|q| q.out != 0) {
        return false;
    }
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    for q in p {
        lo = lo.min(q.s[1]);
        hi = hi.max(q.s[1]);
    }
    let area = VIEW.area.on_screen();
    first_row(lo).max(area.y0) >= first_row(hi).min(area.y1)
}

/// game-3d's filler's first pixel row at or below `y` (1/16 pixel):
/// the first row whose middle is at or below it.
fn first_row(y: i32) -> i32 {
    let m = y - SUB / 2;
    m.div_euclid(SUB) + i32::from(m.rem_euclid(SUB) != 0)
}

/// The store's slot of corner (ci, cj): `ci + 37 * cj` modulo 256.
///
/// Two corners of one polygon lie at most 2 apart each way, so they
/// never share a slot (`|di + 37 * dj| <= 2 + 74 < 256` and is 0 only for
/// the same corner; checked below), and a polygon's slots can be read
/// together. The skew spreads a far ring of corners over the slots,
/// so few are made twice in one picture (store_redo in
/// `tests/speed_counts.rs`: 118 to 0 at HIGH start). Which slot a
/// corner gets changes only how often it is made, never its value
/// (SEAM_PROOF), so the picture cannot change.
fn slot(ci: i32, cj: i32) -> usize {
    (ci.wrapping_add(cj.wrapping_mul(SLOT_SKEW)) & 255) as usize
}

/// The skew of [`slot`].
const SLOT_SKEW: i32 = 37;

const _: () = {
    // No two corners at most 2 apart each way share a slot.
    let mut dj = -2;
    while dj <= 2 {
        let mut di = -2;
        while di <= 2 {
            assert!((di == 0 && dj == 0) || (di + SLOT_SKEW * dj) & 255 != 0);
            di += 1;
        }
        dj += 1;
    }
};

/// True when every corner is inside all planes and the polygon lies
/// wholly beside the picture (nothing to fill).
fn off_screen(pts: &[&Pt]) -> bool {
    if pts.iter().any(|p| p.out != 0) {
        return false;
    }
    let (w, h) = (W as i32 * 16, H as i32 * 16);
    pts.iter().all(|p| p.s[0] < 0)
        || pts.iter().all(|p| p.s[0] >= w)
        || pts.iter().all(|p| p.s[1] < 0)
        || pts.iter().all(|p| p.s[1] >= h)
}

/// subs to game-3d units.
fn unit(p: [i32; 3]) -> [i32; 3] {
    Look::unit(p)
}

/// `a * b / 16384` (b a sine or cosine).
fn mul(a: i32, b: i32) -> i32 {
    (i64::from(a) * i64::from(b) / 16_384) as i32
}

/// The corners of the ribbon from piece `q` over `join` pieces (subs):
/// left and right at its first point, right and left at its last,
/// `LIFT` above the road or the land as drawn, whichever is higher.
fn ribbon_corners(q: usize, join: usize, ground: &Ground) -> [[i32; 3]; 4] {
    ribbon_xz(q, join).map(|(i, xz)| [xz[0], ribbon_y(i, xz, ground), xz[1]])
}

/// [`ribbon_corners`]' x and z (subs), each with its road point (the
/// listing needs the heights of corners 0 and 2 only).
fn ribbon_xz(q: usize, join: usize) -> [(usize, [i32; 2]); 4] {
    let edge = |i: usize, side: i32| {
        let a = ROAD.heading[i];
        let hw = half_width(i) * side;
        let x = ROAD.x[i] + mul(hw, cos_fine(a));
        let z = ROAD.z[i] - mul(hw, sin_fine(a));
        (i, [x, z])
    };
    let (i0, i1) = (2 * q, (2 * (q + join)).min(POINTS - 1));
    [edge(i0, -1), edge(i0, 1), edge(i1, 1), edge(i1, -1)]
}

/// [`ribbon_corners`]' height (subs) of the corner at (x, z) of road
/// point `i`.
fn ribbon_y(i: usize, [x, z]: [i32; 2], ground: &Ground) -> i32 {
    ROAD.y[i].max(ground.height(x, z)) + LIFT
}

/// Whether (x, z) (subs) lies on the road ribbon, and the road point
/// nearest to it.
fn on_road(x: i32, z: i32) -> (bool, usize) {
    let (k, _) = nearest_point(&ROAD, x, z);
    (side_of(&ROAD, k, x, z).abs() <= half_width(k), k)
}

/// The top (subs) of what lies at (x, z): the road ribbon where (x, z)
/// is within `reach` (subs) of the road's edge (its height across and
/// along as `ribbon_corners` sets it on the fine land, held level past
/// the edge), or the land `land` where that is higher or the road is
/// farther.
fn top_at(x: i32, z: i32, land: i32, reach: i32) -> i32 {
    let (k, _) = nearest_point(&ROAD, x, z);
    let side = side_of(&ROAD, k, x, z);
    if side.abs() > half_width(k) + reach {
        return land;
    }
    let along = along_of(&ROAD, k, x, z);
    let n = if along >= 0 {
        (k + 1).min(POINTS - 1)
    } else {
        k.saturating_sub(1)
    };
    // The ribbon's height across point i at this side.
    let across = |i: usize| {
        let a = ROAD.heading[i];
        let hw = half_width(i);
        let edge = |sign: i32| {
            let ex = ROAD.x[i] + mul(hw * sign, cos_fine(a));
            let ez = ROAD.z[i] - mul(hw * sign, sin_fine(a));
            ROAD.y[i].max(LAND.at(ex, ez).height)
        };
        let (l, r) = (edge(-1), edge(1));
        // max then min, not clamp: the same for hw >= 0, and clamp's
        // check of its bounds (a panic path with number printing) cost
        // about 540 bytes on the chip (R2).
        let t = (side + hw).max(0).min(2 * hw);
        l + (r - l) * t / (2 * hw).max(1)
    };
    let (y0, y1) = (across(k), across(n));
    let y = y0 + (y1 - y0) * along.abs().min(STEP) / STEP;
    (y + LIFT).max(land)
}

/// How far past the road's edge the shadow and the dust still keep
/// over the ribbon's height (a car half off the road).
const OVER_EDGE: i32 = 3 * M;

/// The highest drawn land (subs) under the middle (x, z) and four
/// points `r` (subs) from it, so a thing's flat base never sinks into a
/// slope.
fn base_height(ground: &Ground, x: i32, z: i32, r: i32) -> i32 {
    [(0, 0), (r, 0), (-r, 0), (0, r), (0, -r)]
        .iter()
        .map(|&(dx, dz)| ground.height(x + dx, z + dz))
        .max()
        .unwrap_or(0)
}

/// Thing `i`: one of `THINGS`, or past them one of MAX's roadside
/// things (`DECOR`).
fn thing_at(i: usize) -> &'static Thing {
    if i < COUNT {
        &THINGS[i]
    } else {
        &DECOR[i - COUNT]
    }
}

/// Thing `i` as an object, with its footprint's radius (subs: the
/// crown, the hut's walls) for its unit and for leaving it out when it
/// hides the car.
fn thing_object(i: usize, reach: &Reach, ground: &Ground) -> (Object<'static>, i32) {
    let (x, z) = thing_at(i).at();
    let (model, scale, foot, base) = thing_shape(i, reach);
    let y = base_height(ground, x, z, base);
    (
        Object {
            model,
            pos: unit([x, y, z]),
            rot: thing_rot(i),
            scale,
            swap: (0, 0),
        },
        foot,
    )
}

/// Thing `i`'s model, its scale, footprint and base radius (subs)
/// ([`thing_object`] without the base height and the turn, which the
/// listing needs only for things near enough).
fn thing_shape(i: usize, reach: &Reach) -> (&'static Model, i32, i32, i32) {
    let t = thing_at(i);
    let size = i32::from(t.size);
    match t.kind {
        kind::TREE => {
            let m = match reach.tree_sides {
                0..=4 => &TREE4,
                5 | 6 => &TREE6,
                7 | 8 => &TREE8,
                _ => &TREE12,
            };
            let s = size * 256 / TREE_SIZE;
            (m, s, 2 * M * s / 256, M * 24 / 100 * s / 256)
        }
        kind::ROCK => {
            let s = size * 256 / SMALL_SIZE;
            (&ROCK, s, M * 6 / 5 * s / 256, M * 120 / 100 * s / 256)
        }
        kind::HUT => (&HUT, 256, M * 18 / 5, M * 2),
        kind::POST => (&POST, size * 256 / SMALL_SIZE, M / 4, M / 10),
        kind::MARKER => (&MARKER, 256, M / 4, M / 10),
        kind::BALE => (&BALE, 256, M * 9 / 10, M / 2),
        kind::RAIL => (&RAIL, 256, 4 * M, 0),
        _ => {
            let m = if reach.plain_car {
                &BANNER_PLAIN
            } else {
                &BANNER
            };
            (m, 256, M * 18 / 5, M * 34 / 10)
        }
    }
}

/// Thing `i`'s turn: a rock's own, a hut's and a banner's along the
/// road, none for the rest.
fn thing_rot(i: usize) -> Mat3 {
    let t = thing_at(i);
    match t.kind {
        kind::TREE | kind::POST => Mat3::IDENTITY,
        kind::ROCK | kind::BALE => Mat3::rotation_fine((i as u16).wrapping_mul(9_973), 0, 0),
        _ => {
            let (x, z) = t.at();
            Mat3::rotation_fine(ROAD.heading[nearest_point(&ROAD, x, z).0], 0, 0)
        }
    }
}

/// Puff `k` of the dust (`Look::puffs`, kept in the world by the
/// rules): its middle (subs), half size (subs) and colour; None when
/// there is no puff there or the level shows fewer (LOW, MID and HIGH
/// show the newest 2, 4 and 8). A puff is born small (0.15 m half
/// size) with its bottom 3 cm over the road or the land, then grows to
/// 0.4 m and rises 2 cm a step, so it never covers the body above the
/// wheel hubs while it is near (review look F1).
pub fn puff(look: &Look<'_>, k: usize) -> Option<([i32; 3], i32, u8)> {
    let all = look.puffs();
    let p = all.get(k)?;
    if p.age == 0 {
        return None;
    }
    let newer = all.iter().filter(|q| q.age > 0 && q.age < p.age).count();
    if newer >= usize::from(look.reach().puffs) {
        return None;
    }
    let age = i32::from(p.age) - 1;
    let (x, z) = (p.at[0], p.at[1]);
    let half = M * 15 / 100 + age * (M / 60);
    let y = top_at(x, z, LAND.at(x, z).height, OVER_EDGE) + half + M * 3 / 100 + age * (M / 50);
    let colour = if p.spray { col::SPRAY } else { col::DUST };
    Some(([x, y, z], half, colour))
}

/// A puff at LOW: four corners (two triangles, not the hexagon's four).
const DIAMOND: [[i32; 2]; 4] = [[17_000, 0], [0, 17_000], [-17_000, 0], [0, -17_000]];

/// The hexagon's corners (1/16384).
const HEX: [[i32; 2]; 6] = [
    [16_384, 0],
    [8_192, 14_189],
    [-8_192, 14_189],
    [-16_384, 0],
    [-8_192, -14_189],
    [8_192, -14_189],
];

impl Painter {
    pub const fn new() -> Painter {
        Painter {
            r: R3::new(),
            cells: 0,
            // All zero bytes on purpose: `scene` empties every slot
            // before it looks one up, and a `Painter` of zeros can be
            // written straight into its place (see `Drive::start`).
            store: [Slot {
                tag: [0; 2],
                pt: NO_PT,
            }; 256],
        }
    }

    /// Draws the scene (land, road, things, shadow, car, dust) over
    /// the sky already in `f`.
    pub fn scene<R: Rec>(&mut self, f: &mut Frame, look: &Look<'_>, rec: &mut R) {
        self.r.begin();
        self.cells = 0;
        for s in self.store.iter_mut() {
            s.tag = EMPTY;
        }
        let reach = look.reach();
        let cam = look.camera();
        let far = i32::from(reach.far_m) * M8;
        let cb = [cam.pos[0].div_euclid(BLOCK), cam.pos[2].div_euclid(BLOCK)];
        let pic = Pic {
            ground: Ground {
                cb,
                fine: i32::from(reach.fine_blocks),
            },
            cam,
            eye: cam.pos,
            cb,
            fine: i32::from(reach.fine_blocks),
            far,
            start: usize::from(STARTS[look.detail() as usize]),
            far_b2: (i32::from(reach.far_m) / 16).pow(2),
            lit: Lit::<true> {
                mats: MATS,
                shades: SHADES,
                light: [-6_547, -13_093, 7_365],
                glow: GLOW,
                fog_from: far * 35 / 100,
                fog_to: far * 75 / 100,
                fog_shades: 2,
                clear: GLOW,
                dark_side: 3,
            },
            ahead: Ahead::new(&cam, cb, far),
        };
        let mut items = [0u64; MAX_ITEMS];
        let n = self.items(&pic, look, &mut items);
        let items = &mut items[..n];
        sort(items);
        let mut next = 0usize;
        let mut at = pic.start;
        let mut row = [[0i8; 2]; MAX_GROUP];
        while at < NQ {
            // One squared distance's blocks, in the order's order.
            let (n, after) = group(at, &mut row);
            at = after;
            for &[di, dj] in &row[..n] {
                let (di, dj) = (i32::from(di), i32::from(dj));
                let (bi, bj) = (pic.cb[0] + di, pic.cb[1] + dj);
                let unit0 = key(di, dj) * 4;
                if di.abs().max(dj.abs()) <= pic.fine {
                    if self.block_seen(&pic, bi, bj, di, dj) {
                        for (r, [ci, cj]) in cell_order(&pic, bi, bj).into_iter().enumerate() {
                            if self.cell_seen(&pic, ci, cj) {
                                self.cells += 1;
                                self.fine_cell(f, &pic, ci, cj, rec);
                            }
                            self.flush(f, &pic, look, items, &mut next, unit0 + r as u32, rec);
                        }
                    }
                } else if self.block_seen(&pic, bi, bj, di, dj) {
                    self.cells += 1;
                    self.coarse_block(f, &pic, bi, bj, rec);
                }
                self.flush(f, &pic, look, items, &mut next, unit0 + 3, rec);
            }
        }
        self.flush(f, &pic, look, items, &mut next, u32::MAX, rec);
    }

    // ---- The land -------------------------------------------------

    /// Corner (ci, cj) turned into the camera, in the store: its slot
    /// there (read it before the next corner can take the slot; the
    /// corners of one polygon never share a slot, see [`slot`]).
    fn corner(&mut self, pic: &Pic, ci: i32, cj: i32) -> usize {
        let tag = [ci as i16, cj as i16];
        let k = slot(ci, cj);
        if self.store[k].tag == tag {
            return k;
        }
        let c = pic.cam.to_cam_with::<S>(corner_world(ci, cj));
        let pt = cam_pt(c, &mut self.r.stats);
        self.store[k] = Slot { tag, pt };
        k
    }

    /// The three corners of a land triangle, from the store.
    fn tri(&mut self, pic: &Pic, at: [(i32, i32); 3]) -> [usize; 3] {
        [
            self.corner(pic, at[0].0, at[0].1),
            self.corner(pic, at[1].0, at[1].1),
            self.corner(pic, at[2].0, at[2].1),
        ]
    }

    /// Whether any of block (bi, bj), (di, dj) from the camera's block,
    /// can be in the picture.
    fn block_seen(&self, pic: &Pic, bi: i32, bj: i32, di: i32, dj: i32) -> bool {
        if pic.ahead.out(di, dj) {
            return false;
        }
        let (lo, hi) = block_lo_hi(bi, bj);
        let c = [
            bi * BLOCK + HALF_BLOCK,
            (lo + hi) / 2,
            bj * BLOCK + HALF_BLOCK,
        ];
        seen(&pic.cam, c, 2_897 + (hi - lo) / 2 + 1, pic.far)
    }

    /// Whether any of cell (ci, cj) can be in the picture.
    fn cell_seen(&self, pic: &Pic, ci: i32, cj: i32) -> bool {
        let h = [
            corner_h(ci, cj),
            corner_h(ci + 1, cj),
            corner_h(ci, cj + 1),
            corner_h(ci + 1, cj + 1),
        ];
        let lo = h.iter().copied().min().unwrap_or(0);
        let hi = h.iter().copied().max().unwrap_or(0);
        let c = [
            ci * CELL_U + CELL_U / 2,
            (lo + hi) / 2,
            cj * CELL_U + CELL_U / 2,
        ];
        seen(&pic.cam, c, 1_449 + (hi - lo) / 2 + 1, pic.far)
    }

    /// One fine cell: its two triangles, the farther first. A
    /// triangle's colour (light and haze) is worked out only when it is
    /// drawn.
    fn fine_cell<R: Rec>(&mut self, f: &mut Frame, pic: &Pic, ci: i32, cj: i32, rec: &mut R) {
        let byte = cell_byte(ci, cj);
        let diag = byte.is_some_and(|b| b & DIAG_BIT != 0);
        let mut hz = None;
        let tris = [tri_corners(diag, 0), tri_corners(diag, 1)];
        let mid = |t: &[(usize, usize); 3]| {
            let x: i32 = t.iter().map(|c| c.0 as i32).sum::<i32>() * CELL_U / 3 + ci * CELL_U;
            let z: i32 = t.iter().map(|c| c.1 as i32).sum::<i32>() * CELL_U / 3 + cj * CELL_U;
            d2(pic, x, z)
        };
        let first = if mid(&tris[1]) > mid(&tris[0]) { 1 } else { 0 };
        for t in [first, 1 - first] {
            let at = tris[t].map(|(dx, dz)| (ci + dx as i32, cj + dz as i32));
            // The listed order turns downward: (0, 2, 1) faces up.
            let k = self.tri(pic, [at[0], at[2], at[1]]);
            let p = [
                &self.store[k[0]].pt,
                &self.store[k[1]].pt,
                &self.store[k[2]].pt,
            ];
            if !shown(&p, true) {
                continue;
            }
            let colour = || {
                let surface = byte.map_or(GRASS, |b| (b >> (3 * t)) & 7);
                let h = *hz.get_or_insert_with(|| {
                    haze(pic, ci * CELL_U + CELL_U / 2, cj * CELL_U + CELL_U / 2)
                });
                // On the land (a cell byte) from the table, off it as
                // before.
                let light = match byte {
                    Some(_) => fine_light(ci as usize, cj as usize, t),
                    None => {
                        let w = at.map(|(i, j)| corner_world(i, j));
                        light_step(w[0], w[1], w[2])
                    }
                };
                land(land_group(surface), light, h)
            };
            fill(f, &mut self.r.stats, &p, colour, rec);
        }
    }

    /// One block far out: two 16 m triangles, and where it meets a
    /// fine block, the thin triangles that close the gap.
    fn coarse_block<R: Rec>(&mut self, f: &mut Frame, pic: &Pic, bi: i32, bj: i32, rec: &mut R) {
        let (i0, j0) = (bi * 2, bj * 2);
        let diag = cell_byte(i0, j0).is_some_and(|b| b & DIAG_BIT != 0);
        let corners = [0, 1]
            .map(|t| tri_corners(diag, t).map(|(dx, dz)| (i0 + 2 * dx as i32, j0 + 2 * dz as i32)));
        // Each triangle's colour, worked out the first time something
        // in it is drawn.
        let mut hz = None;
        let mut colours = [None; 2];
        let mut colour = |t: usize| {
            *colours[t].get_or_insert_with(|| {
                let w = corners[t].map(|(i, j)| corner_world(i, j));
                // The triangle's middle, subs, for its surface.
                let mx = (w[0][0] + w[1][0] + w[2][0]) / 3 * 16;
                let mz = (w[0][2] + w[1][2] + w[2][2]) / 3 * 16;
                let inside = (0..CELLS_X as i32 * CELL).contains(&mx)
                    && (0..CELLS_Z as i32 * CELL).contains(&mz);
                let surface = if inside {
                    LAND.at(mx, mz).surface
                } else {
                    GRASS
                };
                let h = *hz.get_or_insert_with(|| {
                    haze(pic, bi * BLOCK + HALF_BLOCK, bj * BLOCK + HALF_BLOCK)
                });
                let on_land =
                    (0..BLOCKS_X as i32).contains(&bi) && (0..BLOCKS_Z as i32).contains(&bj);
                let light = if on_land {
                    coarse_light(bi as usize, bj as usize, t)
                } else {
                    light_step(w[0], w[1], w[2])
                };
                land(land_group(surface), light, h)
            })
        };
        let mid = |t: usize| {
            let a = corners[t];
            let x = (a[0].0 + a[1].0 + a[2].0) * CELL_U / 3;
            let z = (a[0].1 + a[1].1 + a[2].1) * CELL_U / 3;
            d2(pic, x, z)
        };
        let first = if mid(1) > mid(0) { 1 } else { 0 };
        for t in [first, 1 - first] {
            let at = corners[t];
            let k = self.tri(pic, [at[0], at[2], at[1]]);
            let p = [
                &self.store[k[0]].pt,
                &self.store[k[1]].pt,
                &self.store[k[2]].pt,
            ];
            if shown(&p, true) {
                fill(f, &mut self.r.stats, &p, || colour(t), rec);
            }
        }
        // The edges shared with fine blocks: (end, end, middle, the
        // neighbour block). The thin triangle is drawn even when the
        // middle corner lies on the line in the world: on the screen it
        // is rounded on its own and can leave a one-pixel crack (MEASURED,
        // PC: up to 2 crack pixels a picture without it).
        let edges = [
            ((i0, j0), (i0 + 2, j0), (i0 + 1, j0), (bi, bj - 1)),
            (
                (i0, j0 + 2),
                (i0 + 2, j0 + 2),
                (i0 + 1, j0 + 2),
                (bi, bj + 1),
            ),
            ((i0, j0), (i0, j0 + 2), (i0, j0 + 1), (bi - 1, bj)),
            (
                (i0 + 2, j0),
                (i0 + 2, j0 + 2),
                (i0 + 2, j0 + 1),
                (bi + 1, bj),
            ),
        ];
        for (a, b, m, nb) in edges {
            let (ndi, ndj) = (nb.0 - pic.cb[0], nb.1 - pic.cb[1]);
            if ndi.abs().max(ndj.abs()) > pic.fine {
                continue;
            }
            let t = if corners[0].contains(&a) && corners[0].contains(&b) {
                0
            } else {
                1
            };
            let k = self.tri(pic, [a, m, b]);
            let p = [
                &self.store[k[0]].pt,
                &self.store[k[1]].pt,
                &self.store[k[2]].pt,
            ];
            if shown(&p, false) {
                fill(f, &mut self.r.stats, &p, || colour(t), rec);
            }
        }
    }

    /// Fills one polygon of camera-space corners (`cull`: only when its
    /// front, in the order given, faces the eye).
    fn poly<R: Rec>(&mut self, f: &mut Frame, p: &[Pt], colour: u8, cull: bool, rec: &mut R) {
        let mut refs = [&p[0]; MAX_CORNERS];
        let n = p.len().min(MAX_CORNERS);
        for (r, q) in refs.iter_mut().zip(p) {
            *r = q;
        }
        if shown(&refs[..n], cull) {
            fill(f, &mut self.r.stats, &refs[..n], || colour, rec);
        }
    }

    // ---- The items ------------------------------------------------

    /// Lists every item that may be in the picture, with its key:
    /// `unit << 40 | kind << 38 | nearness << 8 | number`.
    fn items(&mut self, pic: &Pic, look: &Look<'_>, out: &mut [u64; MAX_ITEMS]) -> usize {
        let mut n = 0usize;
        let mut push = |unit: u32, sub: u64, d2: i64, id: u8| {
            let near = ((1i64 << 30) - 1 - (d2 >> 8).clamp(0, (1 << 30) - 1)) as u64;
            out[n] = (u64::from(unit) << 40) | (sub << 38) | (near << 8) | u64::from(id);
            n += 1;
        };
        let car = look.car_pos();
        let mut car_unit = footprint_unit(pic, car, look.car_yaw());
        // The dust, and each puff's unit from its own cells.
        let mut puffs = [None; MAX_PUFFS];
        let mut puff_unit = [None; MAX_PUFFS];
        for k in 0..MAX_PUFFS {
            let Some((p, half, _)) = puff(look, k) else {
                continue;
            };
            puffs[k] = Some(p);
            // A cloud's second puff reaches out to 5/4 of the half size
            // sideways (MAX).
            let half = if look.reach().cloud {
                half * 5 / 4
            } else {
                half
            };
            puff_unit[k] = span_unit(pic, p[0] - half, p[2] - half, p[0] + half, p[2] + half);
        }
        // The road ribbon. The car and each puff are drawn no sooner
        // than the road pieces near them, so no piece is laid over them.
        let join = usize::from(look.reach().road_join.max(1));
        for q in (0..QUADS).step_by(join) {
            // The corners' x and z first: a piece past the level's far
            // edge needs no heights, and the sort key only corners 0
            // and 2's (the speed work; the same values).
            let c = ribbon_xz(q, join);
            let (x0, x1) = (
                c.iter().map(|p| p.1[0]).min(),
                c.iter().map(|p| p.1[0]).max(),
            );
            let (z0, z1) = (
                c.iter().map(|p| p.1[1]).min(),
                c.iter().map(|p| p.1[1]).max(),
            );
            let (Some(x0), Some(x1), Some(z0), Some(z1)) = (x0, x1, z0, z1) else {
                continue;
            };
            let Some(u) = span_unit_all(pic, x0, z0, x1, z1) else {
                continue;
            };
            let mx = (x0 + x1) / 2;
            let mz = (z0 + z1) / 2;
            let my =
                (ribbon_y(c[0].0, c[0].1, &pic.ground) + ribbon_y(c[2].0, c[2].1, &pic.ground)) / 2;
            let near = |p: [i32; 3]| {
                let (dx, dz) = (i64::from(mx - p[0]), i64::from(mz - p[2]));
                dx * dx + dz * dz < NEAR_ROAD * NEAR_ROAD
            };
            if near(car) {
                car_unit = car_unit.max(Some(u));
            }
            for (p, pu) in puffs.iter().zip(puff_unit.iter_mut()) {
                if p.is_some_and(near) {
                    *pu = (*pu).max(Some(u));
                }
            }
            let mid = unit([mx, my, mz]);
            if !seen(&pic.cam, mid, 4 * M8, pic.far) {
                continue;
            }
            push(u, SUB_ROAD, d2(pic, mid[0], mid[2]), q as u8);
        }
        // The things.
        let cd = d2(pic, car[0] / 16, car[2] / 16);
        let reach = look.reach();
        let things_u = i64::from(reach.things_m) * i64::from(M8);
        // MAX's roadside things come after the rules' things (`thing_at`).
        let decor = if reach.stage { DECOR_COUNT } else { 0 };
        for (i, t) in THINGS.iter().chain(&DECOR[..decor]).enumerate() {
            let id = if i < COUNT {
                ID_THING + i as u8
            } else {
                ID_DECOR + (i - COUNT) as u8
            };
            // The distance first (x and z only), then the shape, the
            // base height and the view (the speed work; the same values
            // as `thing_object`'s).
            let (x, z) = t.at();
            let w = unit([x, 0, z]);
            let d = d2(pic, w[0], w[2]);
            if d > things_u * things_u {
                continue;
            }
            let (model, scale, foot, base) = thing_shape(i, &reach);
            if matches!(t.kind, kind::TREE | kind::HUT | kind::BALE) && look.hides_car(x, z, foot) {
                continue;
            }
            let pos = unit([x, base_height(&pic.ground, x, z, base), z]);
            let r = model.radius * scale / 256 + 1;
            if !seen(&pic.cam, pos, r, pic.far) {
                continue;
            }
            let Some(u) = span_unit(pic, x - foot, z - foot, x + foot, z + foot) else {
                continue;
            };
            // A fence's rails stand beside the road: from the eye on the
            // road they are never in front of the car on it, so they
            // come no later than the car's unit (MAX only).
            if t.kind == kind::RAIL {
                push(car_unit.map_or(u, |cu| u.min(cu)), SUB_ROAD, d, id);
                continue;
            }
            // A thing nearer to the eye than the car but in the car's
            // unit or before it (the car's unit is raised to the road
            // pieces near it) is drawn with the car, after it: so the
            // finish gate the car has just passed under is not covered
            // by the car (final round). The same when a puff behind the
            // thing (one that goes with the car) would be drawn in the
            // thing's unit or later; the thing then comes after those
            // puffs.
            //
            // A banner is a wide plane across the road: what is behind
            // it is what lies on the other side of its line from the eye
            // (its middle's distance says little when the car is under
            // it). The car is behind it only when both its ends are.
            let banner = t.kind == kind::BANNER;
            // The banner's line (its road point's heading), for banners
            // only.
            let (s, c) = if banner {
                let a = ROAD.heading[nearest_point(&ROAD, x, z).0];
                (i64::from(sin_fine(a)), i64::from(cos_fine(a)))
            } else {
                (0, 0)
            };
            let side = |px: i32, pz: i32| i64::from(px - x) * s + i64::from(pz - z) * c < 0;
            let eye = banner && side(pic.eye[0] * 16, pic.eye[2] * 16);
            let behind_it = |px: i32, pz: i32, pd: i64| {
                if banner { side(px, pz) != eye } else { pd > d }
            };
            let nearer = if banner {
                let (ys, yc) = (sin_fine(look.car_yaw()), cos_fine(look.car_yaw()));
                let half = M * 21 / 10;
                let mul = |a: i32, b: i32| (i64::from(a) * i64::from(b) / 16_384) as i32;
                [1, -1]
                    .iter()
                    .all(|&k| behind_it(car[0] + k * mul(half, ys), car[2] + k * mul(half, yc), cd))
            } else {
                d < cd
            };
            let mut late = None;
            if let Some(cu) = car_unit.filter(|_| nearer) {
                let mut tu = u.max(cu);
                // One step of the nearness key (`push` keeps d2 >> 8) nearer.
                let mut key = d.min(cd - 256);
                let mut behind = u <= cu;
                for (p, pu) in puffs.iter().zip(puff_unit.iter()) {
                    if let Some(p) = p {
                        let pd = d2(pic, p[0] / 16, p[2] / 16);
                        if pd <= cd && behind_it(p[0], p[2], pd) {
                            let pu = pu.map_or(cu, |pu| pu.max(cu));
                            behind |= pu >= u;
                            tu = tu.max(pu);
                            key = key.min(pd - 256);
                        }
                    }
                }
                if behind {
                    late = Some((tu, key.max(0)));
                }
            }
            match late {
                Some((tu, key)) => push(tu, SUB_DUST, key, id),
                None => push(u, SUB_ROAD, d, id),
            }
        }
        // The shadow, the car and the dust.
        let Some(cu) = car_unit else {
            return n;
        };
        push(cu, SUB_SHADOW, cd, ID_SHADOW);
        push(cu, SUB_CAR, cd, ID_CAR);
        for k in 0..MAX_PUFFS {
            let Some(p) = puffs[k] else {
                continue;
            };
            let pd = d2(pic, p[0] / 16, p[2] / 16);
            if pd > cd {
                // Farther than the car: in its own unit when that comes
                // before the car's, else with the car, before its
                // shadow.
                match puff_unit[k] {
                    Some(u) if u < cu => push(u, SUB_DUST, pd, ID_PUFF + k as u8),
                    _ => push(cu, SUB_SHADOW, pd, ID_PUFF + k as u8),
                }
            } else {
                let u = puff_unit[k].map_or(cu, |u| u.max(cu));
                push(u, SUB_DUST, pd, ID_PUFF + k as u8);
            }
        }
        n
    }

    /// Draws the items whose unit is at most `unit`: the first test
    /// here, the drawing in [`Painter::flush_some`] (the speed work: most
    /// calls have nothing to draw, and so no longer open its big stack
    /// frame). The same test, the same items in the same order.
    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn flush<R: Rec>(
        &mut self,
        f: &mut Frame,
        pic: &Pic,
        look: &Look<'_>,
        items: &[u64],
        next: &mut usize,
        unit: u32,
        rec: &mut R,
    ) {
        if *next < items.len() && (items[*next] >> 40) as u32 <= unit {
            self.flush_some(f, pic, look, items, next, unit, rec);
        }
    }

    /// [`Painter::flush`]'s drawing.
    #[allow(clippy::too_many_arguments)]
    #[inline(never)]
    fn flush_some<R: Rec>(
        &mut self,
        f: &mut Frame,
        pic: &Pic,
        look: &Look<'_>,
        items: &[u64],
        next: &mut usize,
        unit: u32,
        rec: &mut R,
    ) {
        while *next < items.len() && (items[*next] >> 40) as u32 <= unit {
            let id = (items[*next] & 0xFF) as u8;
            *next += 1;
            self.item(f, pic, look, id, rec);
        }
    }

    fn item<R: Rec>(&mut self, f: &mut Frame, pic: &Pic, look: &Look<'_>, id: u8, rec: &mut R) {
        if id < ID_THING {
            let q = usize::from(id);
            let c = ribbon_corners(q, usize::from(look.reach().road_join.max(1)), &pic.ground);
            let w = c.map(unit);
            let g = if surface_at(2 * q) == FORD {
                group::FORD_ROAD
            } else {
                group::ROAD
            };
            let p = w.map(|p| self.pt(pic, p));
            let p = [&p[0], &p[1], &p[2], &p[3]];
            if shown(&p, false) {
                let colour = || {
                    let haze = haze(pic, (w[0][0] + w[2][0]) / 2, (w[0][2] + w[2][2]) / 2);
                    land(g, light_step(w[0], w[1], w[3]), haze)
                };
                fill(f, &mut self.r.stats, &p, colour, rec);
            }
        } else if !(ID_SHADOW..ID_DECOR).contains(&id) {
            let i = if id < ID_SHADOW {
                usize::from(id - ID_THING)
            } else {
                COUNT + usize::from(id - ID_DECOR)
            };
            let (o, _) = thing_object(i, &look.reach(), &pic.ground);
            self.object(f, pic, &o, rec);
        } else if id == ID_SHADOW {
            self.shadow(f, pic, look, rec);
            self.spray(f, pic, look, rec);
        } else if id == ID_CAR {
            let p = look.car_pos();
            let lift = if on_road(p[0], p[2]).0 { LIFT } else { 0 };
            let o = Object {
                model: if look.reach().plain_car {
                    &CAR_PLAIN
                } else {
                    &CAR
                },
                pos: unit([p[0], p[1] + lift, p[2]]),
                rot: look.car_rotation(),
                scale: 256,
                swap: (0, 0),
            };
            self.object(f, pic, &o, rec);
        } else if let Some((p, half, colour)) = puff(look, usize::from(id - ID_PUFF)) {
            let k = usize::from(id - ID_PUFF);
            let c = pic.cam.to_cam_with::<S>(unit(p));
            let half = half / 16;
            if c[2] < <D3 as Setup>::NEAR + half {
                return;
            }
            let shape: &[[i32; 2]] = if look.reach().plain_car {
                &DIAMOND
            } else {
                &HEX
            };
            // A cloud (MAX): first a puff of 5/8 the size beside this
            // one (to the left or the right by the puff's number, so a
            // trail is not all on one side), its middle a quarter of
            // the half size lower, so its top stays well under this
            // puff's (review look F1); then this puff over it.
            if look.reach().cloud {
                let side = if k % 2 == 0 { 1 } else { -1 };
                let lobe = half * 5 / 8;
                let at = [c[0] + side * lobe, c[1] - half / 4, c[2]];
                self.blob(f, at, lobe, shape, colour, rec);
            }
            self.blob(f, c, half, shape, colour, rec);
        }
    }

    /// A flat puff facing the camera: `shape` (1/16384) scaled by `half`
    /// round the camera point `c`.
    fn blob<R: Rec>(
        &mut self,
        f: &mut Frame,
        c: [i32; 3],
        half: i32,
        shape: &[[i32; 2]],
        colour: u8,
        rec: &mut R,
    ) {
        let mut pts = [NO_PT; 6];
        for (pt, [hx, hy]) in pts.iter_mut().zip(shape) {
            let q = [c[0] + mul(half, *hx), c[1] + mul(half, *hy), c[2]];
            *pt = cam_pt(q, &mut self.r.stats);
        }
        self.poly(f, &pts[..shape.len()], colour, false, rec);
    }

    /// A world point (game-3d units) turned into the camera.
    fn pt(&mut self, pic: &Pic, p: [i32; 3]) -> Pt {
        let c = pic.cam.to_cam_with::<S>(p);
        cam_pt(c, &mut self.r.stats)
    }

    fn object<R: Rec>(&mut self, f: &mut Frame, pic: &Pic, o: &Object<'_>, rec: &mut R) {
        if self
            .r
            .draw_in(f, &VIEW, &PLANES, &pic.cam, &pic.lit, o, pic.far)
        {
            rec.object(o, &pic.cam);
        }
    }

    /// The car's shadow: a plate 4.2 by 2 m on the land (or the road)
    /// under it, in the dark entry of what it lies on.
    fn shadow<R: Rec>(&mut self, f: &mut Frame, pic: &Pic, look: &Look<'_>, rec: &mut R) {
        let p = look.car_pos();
        let (road, k) = on_road(p[0], p[2]);
        let g = if road {
            if surface_at(k) == FORD {
                group::FORD_ROAD
            } else {
                group::ROAD
            }
        } else {
            land_group(LAND.at(p[0], p[2]).surface)
        };
        let yaw = look.car_yaw();
        let (s, c) = (sin_fine(yaw), cos_fine(yaw));
        let top = |a: i32, r: i32| {
            let x = p[0] + mul(a, s) + mul(r, c);
            let z = p[2] + mul(a, c) - mul(r, s);
            [x, top_at(x, z, LAND.at(x, z).height, OVER_EDGE), z]
        };
        // The plate is flat between its corners. Where the land under
        // it bends (the middle or an edge's middle more than `BENT`
        // off the corners' mean) a flat plate would cut into the land
        // and show through it, so there is no shadow (review look F4).
        let (l, w) = (SHADOW_LEN, SHADOW_WIDE);
        let k = [top(-l, -w), top(-l, w), top(l, w), top(l, -w)];
        let mean = |a: usize, b: usize| (k[a][1] + k[b][1]) / 2;
        let bent = [
            (top(0, 0)[1], (mean(0, 2) + mean(1, 3)) / 2),
            (top(-l, 0)[1], mean(0, 1)),
            (top(l, 0)[1], mean(2, 3)),
            (top(0, -w)[1], mean(0, 3)),
            (top(0, w)[1], mean(1, 2)),
        ];
        if bent.iter().any(|&(h, m)| (h - m).abs() > BENT) {
            return;
        }
        let w = k.map(|q| unit([q[0], q[1] + SHADOW_LIFT, q[2]]));
        let pts = w.map(|q| self.pt(pic, q));
        self.poly(f, &pts, col::SHADOW + g, false, rec);
    }
}

impl Painter {
    /// The ford's spray (R3): where the water is drawn under the car,
    /// each front wheel pushes a flat sheet of water aside (at MID and
    /// up the rear wheels a smaller one too). A sheet rises from beside
    /// the wheel and fans out sideways and back, like a bow wave, so the
    /// chase camera sees its face ([`SPRAY`]); its size grows with the
    /// speed, the sheet on the side the car slides toward grows with the
    /// slide, and the sizes flicker step by step. Drawing only, from
    /// the car's state (`Look`): the rules never see it. Drawn with the
    /// shadow, before the car, so the body covers the sheet's foot.
    #[inline(never)]
    fn spray<R: Rec>(&mut self, f: &mut Frame, pic: &Pic, look: &Look<'_>, rec: &mut R) {
        let (fwd, side) = look.car_fwd_side();
        // Bounded at 60 m/s (no car goes so fast), so the sizes below
        // cannot run over.
        let v = fwd.unsigned_abs().min(SPRAY_V) as i32;
        let p = look.car_pos();
        let (road, k) = on_road(p[0], p[2]);
        let wet = if road {
            surface_at(k) == FORD
        } else {
            look.surface() == FORD
        };
        if !wet || look.car_air() || v < SPRAY_FROM {
            return;
        }
        // Turned round when reversing, so the sheets sweep forward and
        // the wheels that lead push the bigger ones.
        let back = fwd < 0;
        let yaw = look.car_yaw().wrapping_add(if back { 0x8000 } else { 0 });
        let turn = Mat3::rotation_fine(yaw, 0, 0);
        // The slide toward the right and toward the left of the way
        // the sheets look.
        let slide = [
            side.max(0).min(SPRAY_V as i32),
            side.min(0).unsigned_abs().min(SPRAY_V) as i32,
        ];
        // Its foot `LIFT` over the car's base: on the ribbon on the
        // road, 12 cm over the water off it (about a pixel, by the tyre).
        let base = unit([p[0], p[1] + LIFT, p[2]]);
        let sheets = if look.reach().plain_car { 2 } else { 4 };
        let t = look.steps() as usize;
        for n in 0..sheets {
            // Sheets 0 and 1 at the leading wheels (1.26 m ahead), 2
            // and 3 at the others (as far behind, 3/4 the size); even
            // ones on the left, the model mirrored (a matrix entry is
            // at most 16384 each way, so its negation cannot run over).
            let left = n % 2 == 0;
            let push = v + slide[usize::from(left != back)] * 2;
            let mut rot = turn;
            if left {
                for r in &mut rot.0 {
                    r[0] = r[0].wrapping_neg();
                }
            }
            let at = turn.apply([
                if left { -SPRAY_IN } else { SPRAY_IN },
                0,
                if n < 2 { WHEEL_AT } else { -WHEEL_AT },
            ]);
            // 1/8 of the full size standing, all of it from 21 m/s of
            // push (`SPRAY` is 1.5 m high at 256), times the flicker.
            let size = (push * 5 / 64 + 32).min(256);
            let o = Object {
                model: &SPRAY,
                pos: [base[0] + at[0], base[1], base[2] + at[2]],
                rot,
                scale: size * i32::from(FLICK[((t ^ n) & 3) | (n & 2) << 1]) / 128,
                swap: (0, 0),
            };
            self.object(f, pic, &o, rec);
        }
    }
}

/// The sheets' flicker step by step, in 1/128 of their size: 13/16
/// to 16/16 at the leading wheels, 3/4 of that at the others.
const FLICK: [u8; 8] = [104, 112, 120, 128, 78, 84, 90, 96];

/// The spray (`Painter::spray`): from 1 m/s; its sheets start 0.96 m
/// out (beside the tyres' outer side, 1.04 m out, so the foot is partly
/// behind the tyre) and reach at most 1.5 m high (the model's full
/// size); the wheels' middle is 1.26 m
/// ahead of and behind the car's middle (`models`' car).
/// `SPRAY_IN` and `WHEEL_AT` are in game-3d units (1/256 m).
const SPRAY_FROM: i32 = M / 30;
const SPRAY_V: u32 = 2 * M as u32;
const SPRAY_IN: i32 = 256 * 96 / 100;
const WHEEL_AT: i32 = 256 * 126 / 100;

/// Whether a land triangle faces the eye. When all three corners are
/// on the screen this is decided from their screen places, the same
/// rounded numbers the filling uses, so a triangle at a ridge's edge is
/// kept or left out exactly as its neighbours' edges expect (deciding
/// it in camera space left a one-pixel crack now and then, MEASURED,
/// PC); otherwise from camera space.
fn faces_screen(p: &[&Pt]) -> bool {
    if p.iter().all(|q| q.out == 0) {
        let (a, b, c) = (p[0].s, p[1].s, p[2].s);
        let cross = i64::from(b[0] - a[0]) * i64::from(c[1] - a[1])
            - i64::from(b[1] - a[1]) * i64::from(c[0] - a[0]);
        if cross != 0 {
            return cross > 0;
        }
    }
    faces_eye::<S>(p[0].c, p[1].c, p[2].c)
}

/// Sorts the item keys, smallest first. An insertion sort: there are at
/// most `MAX_ITEMS` keys (a few dozen in a picture), all different, and
/// `sort_unstable` for
/// `u64` cost 9,198 bytes of chip code (MEASURED, `xtensa-esp32-elf-nm`
/// on the chip build), which put DRIVE3D past its code slot.
fn sort(keys: &mut [u64]) {
    for i in 1..keys.len() {
        let k = keys[i];
        let mut j = i;
        while j > 0 && keys[j - 1] > k {
            keys[j] = keys[j - 1];
            j -= 1;
        }
        keys[j] = k;
    }
}

/// The squared horizontal distance from the eye to (x, z) (units).
fn d2(pic: &Pic, x: i32, z: i32) -> i64 {
    let (dx, dz) = (i64::from(x - pic.eye[0]), i64::from(z - pic.eye[2]));
    dx * dx + dz * dz
}

/// The haze step (0 to 3) at (x, z) (units): from 35, 55 and 75 % of
/// the far edge.
fn haze(pic: &Pic, x: i32, z: i32) -> u8 {
    let d = d2(pic, x, z) * 400;
    let f2 = i64::from(pic.far) * i64::from(pic.far);
    if d < f2 * 49 {
        0
    } else if d < f2 * 121 {
        1
    } else if d < f2 * 225 {
        2
    } else {
        3
    }
}

/// The four cells of fine block (bi, bj), the farthest first.
///
/// The same ranks as [`cell_rank`], from the four middles' distances
/// worked out once (the speed work: 4 distances instead of 16).
fn cell_order(pic: &Pic, bi: i32, bj: i32) -> [[i32; 2]; 4] {
    let at = |k: i32| [bi * 2 + (k & 1), bj * 2 + (k >> 1)];
    let d = [0, 1, 2, 3].map(|k| {
        let [ci, cj] = at(k);
        d2(pic, ci * CELL_U + CELL_U / 2, cj * CELL_U + CELL_U / 2)
    });
    let mut out = [[0i32; 2]; 4];
    for me in 0..4 {
        let mut r = 0;
        for k in 0..4 {
            if k != me && (d[k] > d[me] || (d[k] == d[me] && k < me)) {
                r += 1;
            }
        }
        out[r] = at(me as i32);
    }
    out
}

/// A fine cell's place in its block's drawing order (0 first).
fn cell_rank(pic: &Pic, ci: i32, cj: i32) -> u32 {
    let (bi, bj) = (ci.div_euclid(2), cj.div_euclid(2));
    let mid = |i: i32, j: i32| d2(pic, i * CELL_U + CELL_U / 2, j * CELL_U + CELL_U / 2);
    let me = mid(ci, cj);
    let my_k = (cj - bj * 2) * 2 + (ci - bi * 2);
    let mut r = 0;
    for k in 0..4 {
        if k == my_k {
            continue;
        }
        let d = mid(bi * 2 + (k & 1), bj * 2 + (k >> 1));
        if d > me || (d == me && k < my_k) {
            r += 1;
        }
    }
    r
}

/// The unit of cell (ci, cj): None when its block is beyond the level's
/// far edge.
fn cell_unit(pic: &Pic, ci: i32, cj: i32) -> Option<u32> {
    let (bi, bj) = (ci.div_euclid(2), cj.div_euclid(2));
    let (di, dj) = (bi - pic.cb[0], bj - pic.cb[1]);
    // Beyond the far edge: exactly the entries before `pic.start` and
    // the offsets not in the table (it is sorted by squared offset, and
    // the far edge is at most `REACH`).
    if di * di + dj * dj > pic.far_b2 {
        return None;
    }
    let r = if di.abs().max(dj.abs()) <= pic.fine {
        cell_rank(pic, ci, cj)
    } else {
        3
    };
    Some(key(di, dj) * 4 + r)
}

/// The largest unit of the cells under the box (x0, z0)..(x1, z1)
/// (subs), or None when none of them is drawn.
fn span_unit(pic: &Pic, x0: i32, z0: i32, x1: i32, z1: i32) -> Option<u32> {
    let mut best = None;
    for cj in z0.div_euclid(CELL)..=z1.div_euclid(CELL) {
        for ci in x0.div_euclid(CELL)..=x1.div_euclid(CELL) {
            best = best.max(cell_unit(pic, ci, cj));
        }
    }
    best
}

/// Like [`span_unit`], but None unless every cell under the box is
/// drawn: a road piece reaching past the land's far edge is left out,
/// so it never hangs over the sky.
fn span_unit_all(pic: &Pic, x0: i32, z0: i32, x1: i32, z1: i32) -> Option<u32> {
    let mut best = 0;
    for cj in z0.div_euclid(CELL)..=z1.div_euclid(CELL) {
        for ci in x0.div_euclid(CELL)..=x1.div_euclid(CELL) {
            best = best.max(cell_unit(pic, ci, cj)?);
        }
    }
    Some(best)
}

/// The car's unit from its footprint (its middle and four corners).
fn footprint_unit(pic: &Pic, p: [i32; 3], yaw: u16) -> Option<u32> {
    let (s, c) = (sin_fine(yaw), cos_fine(yaw));
    let mut best = None;
    for (a, r) in [
        (0, 0),
        (FOOT_LEN, FOOT_WIDE),
        (FOOT_LEN, -FOOT_WIDE),
        (-FOOT_LEN, FOOT_WIDE),
        (-FOOT_LEN, -FOOT_WIDE),
    ] {
        let x = p[0] + mul(a, s) + mul(r, c);
        let z = p[2] + mul(a, c) - mul(r, s);
        best = best.max(cell_unit(pic, x.div_euclid(CELL), z.div_euclid(CELL)));
    }
    best
}

/// The number of entries in the order table (for tests).
pub const ORDER_LEN: usize = N;
