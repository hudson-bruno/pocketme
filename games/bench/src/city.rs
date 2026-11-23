//! CITY: a street through a small city that the eye drives along for
//! ever, drawn with the bench's renderer ([`crate::poly`]): the showcase
//! scene, "what can the console draw".
//!
//! The world: a main road along +z (four lanes, a yellow middle line,
//! dashed white lane lines, a zebra crossing before each junction), a
//! cross street every 32 m, and on each side four rows of buildings
//! (low shops and houses at the road, towers at the back). A building
//! is a box, some with a darker window band or three, some with a
//! smaller box on top. Cars (a body and a glass cabin) drive both ways
//! in four lanes and cross at the junctions. The sun comes from the
//! upper right behind the eye: one side of the street is in shadow.
//!
//! The eye flies 3 m above the middle line at 10 m a second, looking a
//! little down and swaying 10 degrees left and right every 8.5 s. Time
//! is game time in ticks of 1/60 s (the same speed at 30 and at 60
//! calls a second; 60 shows twice the pictures). The city is 8 blocks
//! (256 m) long and repeats: one cycle is [`CYCLE`] ticks (25.6 s) and
//! every car is back where it started, so the picture of tick
//! `t + CYCLE` is the picture of tick `t`. Every position is written so
//! that tick `2n` gives exactly call `n` of the earlier rounds at 30.
//!
//! **The load** is the number of buildings: the `load` buildings
//! nearest ahead (by their far end), all four rows, both sides. So the
//! load sets how far the city reaches (about 1.6 m of street per
//! building: 8 is the corner, 128 is about 200 m). The cars on about
//! that stretch come with it, out to a fixed distance per load
//! ([`car_reach`], at least 48 m), so no car blinks when the nearest
//! buildings change. The road itself is always drawn to [`ROAD_RANGE`]
//! (the fixed part of every load).
//!
//! **What is counted**: the city's paint has [`Paint::ONLY_PAINTED`], so
//! a triangle that holds no pixel middle of the view is not handed to
//! the filler, and TRIS counts only triangles that painted a pixel.
//!
//! The drawing order (there is no depth buffer): the sky and the ground
//! as flat bands, then the road pieces of each block (flat, in layers),
//! then buildings and cars sorted far to near by the distance from the
//! eye to the nearest point of their ground plan. Cars whose whole plan
//! is on the main road are drawn after every building: nothing at the
//! side of the road can be in front of them, seen from the middle line.
//! Inside a model the faces keep their order ([`Paint::KEEP_ORDER`]):
//! the eye is always above the cars' bodies and below every roof.
//!
//! Units: positions in 1/128 m ([`CM`], half the balls' scale, so that
//! 200 m ahead and 64 m to the side still fit the renderer's 2^15).
//! Everything depends only on the load and the tick.

use game_kit::Frame;

use crate::art::{CITY_SHADES, CITY0, PAVE, SKY_HI, SKY_LOW, SKY_MID};
use crate::fixed::{Mat3, ONE14, V3, hash, isqrt, sin};
use crate::load::{Counts, MAX_OBJ, VIEW, WORK};
use crate::poly::{
    Face, MAX_VERTS, Model, Object, Paint, Rect, Renderer, first_row, in_view, radius_of, rect_fill,
};

/// One metre in the city.
pub const CM: i32 = 128;
/// A block: a junction (8 m) and 24 m of buildings.
pub const BLOCK: i32 = 32 * CM;
/// Blocks before the city repeats.
pub const BLOCKS: i32 = 8;
/// The city's length (256 m).
pub const LENGTH: i32 = BLOCK * BLOCKS;
/// Ticks of 1/60 s in one cycle (25.6 s at 30 and at 60): the eye
/// moves [`LENGTH`] and every car is back in its place.
pub const CYCLE: u32 = 1536;
/// In the ramp's 30 half, CITY moves this many ticks of the cycle per
/// call, not by game time (the eye goes 80 m a second), so that one ramp
/// step of [`RAMP_CALLS`] calls sees every 16th tick of the whole cycle
/// once, however late the calls come: the step's FIT and count are the
/// whole cycle's, not those of the stretch that happened to be on screen
/// (the check's defect C). These are the same 96 pictures as in the
/// earlier rounds. The 60 half does not use it (see [`ramp60_tick`]).
pub const RAMP_STRIDE: u32 = 16;
/// Calls measured per CITY ramp step: one whole cycle at
/// [`RAMP_STRIDE`] (3.2 s at 30, longer when over), and at 60
/// [`SEGMENTS`] x [`SEG_MEASURED`].
pub const RAMP_CALLS: u32 = CYCLE / RAMP_STRIDE;

// ---- The ramp's 60 half: short runs at real speed --------------------------
//
// At 60 the stride above made nearly every picture change across the whole
// width of the screen (the eye 16 times faster than real), and such a
// picture is sent in step with the panel, which refreshes 58.7 times a
// second: every call waited for one sweep and every load was judged over
// (board, round 3). So at 60 a step is [`SEGMENTS`] short runs at real speed
// (one tick a call, the pictures of the fixed test at 60), spread evenly
// over the cycle: run `s` starts at tick `s * SEG_GAP`. Each run's first
// [`SEG_WARM`] calls are not counted: its first picture jumps to a new
// place in the street (a full-width change), and the calls after it can
// be late and then early (the console catches up on its frame grid) while
// the second core sends that picture; then [`SEG_MEASURED`] calls are
// counted. The step starts with [`SEG_LEAD`] more calls not counted (the
// ramp's usual warm-up), showing the four ticks before tick 0.

/// Runs per step at 60.
pub const SEGMENTS: u32 = 16;
/// Ticks between the runs' starts (1.6 s of the street).
pub const SEG_GAP: u32 = CYCLE / SEGMENTS;
/// Calls of a run not counted.
pub const SEG_WARM: u32 = 6;
/// Calls of a run counted.
pub const SEG_MEASURED: u32 = 6;
/// Calls of a run.
pub const SEG_CALLS: u32 = SEG_WARM + SEG_MEASURED;
/// Calls before the first run (the ramp's warm-up, `ramp::WARM`).
pub const SEG_LEAD: u32 = 4;
/// Calls of one step at 60 (when the counted calls add up to at least
/// half a second, which 96 calls at 60 always do on the console).
pub const STEP60_CALLS: u32 = SEG_LEAD + SEGMENTS * SEG_CALLS;

const _: () = {
    assert!(SEGMENTS * SEG_MEASURED == RAMP_CALLS);
    assert!(SEGMENTS * SEG_GAP == CYCLE);
    assert!(SEG_CALLS <= SEG_GAP);
    assert!(SEG_LEAD == crate::ramp::WARM);
    // A 60 step takes a whole number of cycles' calls more than the old
    // one (4 + 96 calls), so the 30 half that follows sees the same
    // pictures as before: its pictures are `RAMP_STRIDE` times the calls
    // since the ramp started, which repeat every `RAMP_CALLS` calls.
    assert!((STEP60_CALLS - (SEG_LEAD + RAMP_CALLS)).is_multiple_of(RAMP_CALLS));
};

/// Call `j` of a CITY ramp step at 60 (0 = the step's first call): the
/// tick it draws and whether its time is counted. A step longer than
/// [`STEP60_CALLS`] (only when its counted calls are shorter than the
/// frame) goes on with the last run, counted.
pub const fn ramp60_tick(j: u32) -> (u32, bool) {
    if j < SEG_LEAD {
        return (CYCLE - SEG_LEAD + j, false);
    }
    let e = j - SEG_LEAD;
    let mut s = e / SEG_CALLS;
    if s > SEGMENTS - 1 {
        s = SEGMENTS - 1;
    }
    let c = e - s * SEG_CALLS;
    ((s * SEG_GAP + c) % CYCLE, c >= SEG_WARM)
}
/// The eye's height (3 m).
pub const EYE_Y: i32 = 3 * CM;
/// The road is drawn this far ahead at every load.
pub const ROAD_RANGE: i32 = 190 * CM;
/// The largest load (buildings): it stays below the ~200 m every
/// position must fit in.
pub const MAX_LOAD: u32 = 128;
/// Nothing farther than this is drawn (a guard: the loads stay inside).
pub const FAR: i32 = 240 * CM;
/// Half the road's width (6.5 m: four lanes of 3.25 m).
pub const ROAD: i32 = 832;
/// Half the cross streets' length (64 m).
pub const CROSS_HALF: i32 = 64 * CM;

// ---- Materials (palette: `CITY0 + material * CITY_SHADES + shade`) --------

pub const ASPHALT: u8 = 0;
pub const LINE_Y: u8 = 1;
pub const LINE_W: u8 = 2;
/// Eight wall colours.
pub const WALL0: u8 = 3;
pub const WALLS: u32 = 8;
/// Two glass colours.
pub const GLASS0: u8 = 11;
/// Six car colours.
pub const CAR0: u8 = 13;
pub const CAR_COLOURS: u32 = 6;
/// Roofs: concrete.
const ROOF: u8 = WALL0 + 2;

/// The sun: falls from the upper right behind the eye (world, 16384
/// long). Faces get these shades: roofs and ground 1, faces looking
/// left (+x, the left side's street fronts) 2, faces looking at the eye
/// 3, faces looking right (the right side's fronts) and away 6.
pub const SUN: V3 = [-9830, -11469, 6390];

/// The city's colours: four materials per object, faces drawn in order.
#[derive(Clone, Copy, Debug)]
pub struct CityPaint {
    pub mats: [u8; 4],
}

impl Paint for CityPaint {
    const KEEP_ORDER: bool = true;
    const ONLY_PAINTED: bool = true;

    #[inline(always)]
    fn dir(&self) -> V3 {
        SUN
    }

    #[inline(always)]
    fn colour(&self, mat: u8, shade: u8) -> u8 {
        let m = self.mats[usize::from(mat & 3)];
        CITY0
            .wrapping_add(m.wrapping_mul(CITY_SHADES))
            .wrapping_add(shade.min(CITY_SHADES - 1))
    }
}

// ---- Model tables (made while compiling) ----------------------------------

const UP: [i16; 3] = [0, ONE14 as i16, 0];
/// The four sides of a box, in corner order (see [`box_sides`]): -z,
/// +x, +z, -x; two triangles each.
const SIDES: [[i16; 3]; 8] = [
    [0, 0, -ONE14 as i16],
    [0, 0, -ONE14 as i16],
    [ONE14 as i16, 0, 0],
    [ONE14 as i16, 0, 0],
    [0, 0, ONE14 as i16],
    [0, 0, ONE14 as i16],
    [-ONE14 as i16, 0, 0],
    [-ONE14 as i16, 0, 0],
];

const NO_FACE: Face = Face { v: [0; 3], mat: 0 };

/// Four corners of a level: `(-hx, -hz), (hx, -hz), (hx, hz), (-hx, hz)`.
const fn level(v: &mut [[i16; 3]; MAX_VERTS], at: usize, hx: i32, hz: i32, y: i32) {
    v[at] = [-hx as i16, y as i16, -hz as i16];
    v[at + 1] = [hx as i16, y as i16, -hz as i16];
    v[at + 2] = [hx as i16, y as i16, hz as i16];
    v[at + 3] = [-hx as i16, y as i16, hz as i16];
}

/// The four sides between the level at `lo` and the one at `hi` (two
/// triangles each, in the order of [`SIDES`]).
const fn box_sides(f: &mut [Face], n: usize, lo: usize, hi: usize, mat: u8) -> usize {
    let mut k = 0;
    while k < 4 {
        let k1 = (k + 1) % 4;
        f[n + 2 * k] = Face {
            v: [(lo + k) as u8, (lo + k1) as u8, (hi + k1) as u8],
            mat,
        };
        f[n + 2 * k + 1] = Face {
            v: [(lo + k) as u8, (hi + k1) as u8, (hi + k) as u8],
            mat,
        };
        k += 1;
    }
    n + 8
}

/// A flat top over the level at `at`.
const fn lid(f: &mut [Face], n: usize, at: usize, mat: u8) -> usize {
    f[n] = Face {
        v: [at as u8, (at + 1) as u8, (at + 2) as u8],
        mat,
    };
    f[n + 1] = Face {
        v: [at as u8, (at + 2) as u8, (at + 3) as u8],
        mat,
    };
    n + 2
}

// Buildings. Corners: the main box's levels first (4 each, bottom to
// top: `2 * bands + 2` levels), then the top box's two levels. Faces:
// the top box (its sides, its roof), then the main roof, then the main
// box's strips bottom to top (wall, glass, wall, ...). The top box comes
// first: the eye is below every roof, so the main box hides the top
// box's lower part, never the other way round.

/// Most window bands.
pub const MAX_BANDS: usize = 3;
/// Most faces of a building (three bands and a top box).
pub const BLD_FACES: usize = 10 + 2 + 8 * (2 * MAX_BANDS + 1);

/// Corners of a building.
pub const fn bld_verts(bands: usize, top: bool) -> usize {
    4 * (2 * bands + 2) + if top { 8 } else { 0 }
}

/// Faces of a building.
pub const fn bld_faces(bands: usize, top: bool) -> usize {
    (if top { 10 } else { 0 }) + 2 + 8 * (2 * bands + 1)
}

const fn bld_face_table(bands: usize, top: bool) -> [Face; BLD_FACES] {
    let mut f = [NO_FACE; BLD_FACES];
    let levels = 2 * bands + 2;
    let mut n = 0;
    if top {
        let b = 4 * levels;
        n = box_sides(&mut f, n, b, b + 4, 3);
        n = lid(&mut f, n, b + 4, 3);
    }
    n = lid(&mut f, n, 4 * (levels - 1), 2);
    let mut s = 0;
    while s + 1 < levels {
        n = box_sides(&mut f, n, 4 * s, 4 * (s + 1), (s % 2) as u8);
        s += 1;
    }
    let _ = n;
    f
}

const fn all_bld_faces() -> [[Face; BLD_FACES]; 2 * (MAX_BANDS + 1)] {
    let mut t = [[NO_FACE; BLD_FACES]; 2 * (MAX_BANDS + 1)];
    let mut b = 0;
    while b <= MAX_BANDS {
        t[2 * b] = bld_face_table(b, false);
        t[2 * b + 1] = bld_face_table(b, true);
        b += 1;
    }
    t
}

/// Face tables by `bands * 2 + top`.
pub static BLD_FACE_TABLES: [[Face; BLD_FACES]; 2 * (MAX_BANDS + 1)] = all_bld_faces();

const fn bld_normal_table() -> [[i16; 3]; BLD_FACES] {
    let mut t = [UP; BLD_FACES];
    let mut i = 0;
    while i < 8 {
        t[i] = SIDES[i];
        i += 1;
    }
    // 8..12: the top box's roof and the main roof (up).
    let mut i = 12;
    while i < BLD_FACES {
        t[i] = SIDES[(i - 12) % 8];
        i += 1;
    }
    t
}

/// Normals of every building face table: with a top box from 0, without
/// from 10 (the same order of directions).
pub static BLD_NORMALS: [[i16; 3]; BLD_FACES] = bld_normal_table();

/// The faces and normals of a building.
pub fn bld_model_tables(bands: usize, top: bool) -> (&'static [Face], &'static [[i16; 3]]) {
    let b = bands.min(MAX_BANDS);
    let nf = bld_faces(b, top);
    let faces = &BLD_FACE_TABLES[2 * b + usize::from(top)][..nf];
    let from = if top { 0 } else { 10 };
    (faces, &BLD_NORMALS[from..from + nf])
}

// Cars: a body (4.2 x 1.8 x 0.75 m) and a glass cabin on it, the middle
// 0.85 m above the road, the front at +z. The body first: the eye is
// always above it.

/// The car's middle above the road.
pub const CAR_Y: i32 = 109;
/// Half the car's length and width.
pub const CAR_HL: i32 = 269;
pub const CAR_HW: i32 = 115;

const fn car_verts() -> [[i16; 3]; MAX_VERTS] {
    let mut v = [[0i16; 3]; MAX_VERTS];
    level(&mut v, 0, CAR_HW, CAR_HL, -77);
    level(&mut v, 4, CAR_HW, CAR_HL, 19);
    // The cabin: 1.56 m wide, 2 m long, a little behind the middle.
    let mut k = 0;
    while k < 8 {
        let y = if k < 4 { 19 } else { 92 };
        let c = k % 4;
        let x: i16 = if c == 1 || c == 2 { 100 } else { -100 };
        let z: i16 = if c >= 2 { 109 } else { -147 };
        v[8 + k] = [x, y, z];
        k += 1;
    }
    v
}

const fn car_faces() -> [Face; 20] {
    let mut f = [NO_FACE; 20];
    let mut n = box_sides(&mut f, 0, 0, 4, 0);
    n = lid(&mut f, n, 4, 0);
    n = box_sides(&mut f, n, 8, 12, 1);
    n = lid(&mut f, n, 12, 0);
    let _ = n;
    f
}

const fn car_normals() -> [[i16; 3]; 20] {
    let mut t = [UP; 20];
    let mut i = 0;
    while i < 8 {
        t[i] = SIDES[i];
        t[10 + i] = SIDES[i];
        i += 1;
    }
    t
}

const CAR_V: [[i16; 3]; MAX_VERTS] = car_verts();
static CAR_VERTS: [[i16; 3]; 16] = {
    let mut v = [[0i16; 3]; 16];
    let mut i = 0;
    while i < 16 {
        v[i] = CAR_V[i];
        i += 1;
    }
    v
};
static CAR_FACES: [Face; 20] = car_faces();
static CAR_NORMALS: [[i16; 3]; 20] = car_normals();
/// The car.
pub static CAR: Model<'static> = Model {
    verts: &CAR_VERTS,
    faces: &CAR_FACES,
    normals: &CAR_NORMALS,
    radius: radius_of(&CAR_V),
};

// The road of one block: the middle of the model is the middle of the
// block on the ground; the junction is its first 8 m (-16 m to -8 m).
// Layers: the main road, the cross street (left and right), the yellow
// middle line, the dashed lane lines, the zebra crossing before the
// next junction.

const fn quad(v: &mut [[i16; 3]; MAX_VERTS], f: &mut [Face; 32], n: usize, r: [i32; 4], mat: u8) {
    let at = 4 * n;
    v[at] = [r[0] as i16, 0, r[2] as i16];
    v[at + 1] = [r[1] as i16, 0, r[2] as i16];
    v[at + 2] = [r[1] as i16, 0, r[3] as i16];
    v[at + 3] = [r[0] as i16, 0, r[3] as i16];
    lid(f, 2 * n, at, mat);
}

/// In 1/10 m.
const fn dm(x: i32) -> i32 {
    x * CM / 10
}

const fn road_tables() -> ([[i16; 3]; MAX_VERTS], [Face; 32]) {
    let mut v = [[0i16; 3]; MAX_VERTS];
    let mut f = [NO_FACE; 32];
    let m = CM;
    quad(&mut v, &mut f, 0, [-ROAD, ROAD, -16 * m, 16 * m], 0);
    quad(&mut v, &mut f, 1, [-CROSS_HALF, -ROAD, -16 * m, -8 * m], 0);
    quad(&mut v, &mut f, 2, [ROAD, CROSS_HALF, -16 * m, -8 * m], 0);
    quad(&mut v, &mut f, 3, [-dm(2), dm(2), -8 * m, dm(130)], 1);
    // Lane lines at +-3.25 m: three dashes of 3 m.
    let mut d = 0;
    while d < 6 {
        let x = if d % 2 == 0 { -416 } else { 416 };
        let z0 = [-5, 1, 7][d / 2] * m;
        quad(
            &mut v,
            &mut f,
            4 + d,
            [x - dm(1), x + dm(1), z0, z0 + 3 * m],
            2,
        );
        d += 1;
    }
    // The zebra: six stripes of 0.9 m across the road.
    let mut s = 0;
    while s < 6 {
        let x = (2 * s as i32 - 5) * m;
        quad(
            &mut v,
            &mut f,
            10 + s,
            [x - 58, x + 58, dm(135), dm(155)],
            2,
        );
        s += 1;
    }
    (v, f)
}

const ROAD_T: ([[i16; 3]; MAX_VERTS], [Face; 32]) = road_tables();
static ROAD_VERTS: [[i16; 3]; MAX_VERTS] = ROAD_T.0;
static ROAD_FACES: [Face; 32] = ROAD_T.1;
static ROAD_NORMALS: [[i16; 3]; 32] = [UP; 32];
/// One block's road.
pub static ROAD_MODEL: Model<'static> = Model {
    verts: &ROAD_VERTS,
    faces: &ROAD_FACES,
    normals: &ROAD_NORMALS,
    radius: radius_of(&ROAD_T.0),
};

/// Model to world turnings: facing +z, -z, +x, -x.
const TURN: [Mat3; 4] = [
    Mat3::IDENTITY,
    Mat3([[-ONE14, 0, 0], [0, ONE14, 0], [0, 0, -ONE14]]),
    Mat3([[0, 0, ONE14], [0, ONE14, 0], [-ONE14, 0, 0]]),
    Mat3([[0, 0, -ONE14], [0, ONE14, 0], [ONE14, 0, 0]]),
];

// ---- The buildings (made while compiling) --------------------------------

/// One building: its ground plan (middle and half sizes; `z` within the
/// city's length), heights, window bands and colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lot {
    pub x: i16,
    pub z: i16,
    pub hx: i16,
    pub hz: i16,
    /// The main box's height.
    pub h: i16,
    /// The box on top's height (0: none).
    pub top: i16,
    pub bands: u8,
    pub wall: u8,
    pub glass: u8,
    pub top_wall: u8,
    /// A ball round the middle (height `(h + top) / 2`) holding it all.
    pub radius: i16,
}

impl Lot {
    /// Its far end.
    pub const fn far(&self) -> i32 {
        self.z as i32 + self.hz as i32
    }

    /// The height of its middle.
    pub const fn mid_y(&self) -> i32 {
        (self.h as i32 + self.top as i32) / 2
    }
}

/// The rows of buildings beside the road: where each starts (m from the
/// middle line), how many buildings share a block's 22 m, the lowest
/// height and the spread (m).
const ROW_X: [i32; 4] = [9, 23, 37, 51];
const ROW_N: [u32; 4] = [3, 3, 2, 2];
const ROW_H: [(u32, u32); 4] = [(5, 12), (10, 16), (16, 22), (22, 26)];
/// Buildings in the city.
pub const LOTS_N: usize = 2 * (3 + 3 + 2 + 2) * BLOCKS as usize;

const NO_LOT: Lot = Lot {
    x: 0,
    z: 0,
    hx: 0,
    hz: 0,
    h: 0,
    top: 0,
    bands: 0,
    wall: 0,
    glass: 0,
    top_wall: 0,
    radius: 0,
};

/// Where cut `k` of `n` falls in a row of a block (1/128 m from the
/// city's start).
const fn cut(b: u32, side: u32, row: u32, k: u32, n: u32) -> i32 {
    let z0 = (b as i32) * BLOCK + 9 * CM;
    let span = 22 * CM;
    if k == 0 {
        return z0;
    }
    if k == n {
        return z0 + span;
    }
    let j = (hash(b * 64 + side * 32 + row * 8 + k, 31) % 384) as i32 - 192;
    z0 + span * k as i32 / n as i32 + j
}

const fn make_lots() -> [Lot; LOTS_N] {
    let mut out = [NO_LOT; LOTS_N];
    let mut n = 0;
    let mut b = 0u32;
    while b < BLOCKS as u32 {
        let mut side = 0u32;
        while side < 2 {
            let mut row = 0u32;
            while row < 4 {
                let nr = ROW_N[row as usize];
                let mut k = 0u32;
                while k < nr {
                    let h = hash(b * 64 + side * 32 + row * 8 + k, 77);
                    // A 0.5 m gap between neighbours.
                    let z0 = cut(b, side, row, k, nr) + 32;
                    let z1 = cut(b, side, row, k + 1, nr) - 32;
                    let depth = (8 + (h % 4) as i32) * CM;
                    let x0 = ROW_X[row as usize] * CM;
                    let xm = x0 + depth / 2;
                    let (lo, spread) = ROW_H[row as usize];
                    let hm = lo + (h >> 8) % spread;
                    let bands = if row == 0 {
                        (hm >= 8) as u32 + ((h >> 12) & 1)
                    } else {
                        let nb = hm / 8;
                        if nb > 3 { 3 } else { nb }
                    };
                    let top = if hm >= 12 && (h >> 14).is_multiple_of(3) {
                        3 + (h >> 16) % 4
                    } else {
                        0
                    };
                    let wall = (h >> 18) % WALLS;
                    let top_wall = (wall + 1 + (h >> 22) % (WALLS - 1)) % WALLS;
                    let hx = depth / 2;
                    let hz = (z1 - z0) / 2;
                    let hh = (hm + top) as i32 * CM;
                    let r2 = (hx as i64 * hx as i64
                        + hz as i64 * hz as i64
                        + (hh / 2) as i64 * (hh / 2) as i64) as u64;
                    out[n] = Lot {
                        x: (if side == 0 { -xm } else { xm }) as i16,
                        z: ((z0 + z1) / 2) as i16,
                        hx: hx as i16,
                        hz: hz as i16,
                        h: (hm as i32 * CM) as i16,
                        top: (top as i32 * CM) as i16,
                        bands: bands as u8,
                        wall: WALL0 + wall as u8,
                        glass: GLASS0 + ((h >> 21) & 1) as u8,
                        top_wall: WALL0 + top_wall as u8,
                        radius: (isqrt(r2) + 2) as i16,
                    };
                    n += 1;
                    k += 1;
                }
                row += 1;
            }
            side += 1;
        }
        b += 1;
    }
    // Sorted by their far end.
    let mut i = 1;
    while i < LOTS_N {
        let l = out[i];
        let mut j = i;
        while j > 0 && out[j - 1].far() > l.far() {
            out[j] = out[j - 1];
            j -= 1;
        }
        out[j] = l;
        i += 1;
    }
    out
}

/// Every building, by its far end.
pub static LOTS: [Lot; LOTS_N] = make_lots();

/// A building's corners (middle at `mid_y` above the ground); returns
/// how many.
pub fn bld_corners(l: &Lot, v: &mut [[i16; 3]; MAX_VERTS]) -> usize {
    let bands = usize::from(l.bands).min(MAX_BANDS);
    let (hx, hz, h) = (i32::from(l.hx), i32::from(l.hz), i32::from(l.h));
    let mid = l.mid_y();
    let levels = 2 * bands + 2;
    let strips = (levels - 1) as i32;
    for i in 0..levels {
        // Heights below 2^13: no overflow.
        let y = h.wrapping_mul(i as i32) / strips - mid;
        level_rt(v, 4 * i, hx, hz, y);
    }
    let mut n = 4 * levels;
    if l.top > 0 {
        let (tx, tz) = (hx * 3 / 5, hz * 3 / 5);
        level_rt(v, n, tx, tz, h - mid);
        level_rt(v, n + 4, tx, tz, h + i32::from(l.top) - mid);
        n += 8;
    }
    n
}

fn level_rt(v: &mut [[i16; 3]; MAX_VERTS], at: usize, hx: i32, hz: i32, y: i32) {
    let (x, z, y) = (hx as i16, hz as i16, y as i16);
    v[at] = [x.wrapping_neg(), y, z.wrapping_neg()];
    v[at + 1] = [x, y, z.wrapping_neg()];
    v[at + 2] = [x, y, z];
    v[at + 3] = [x.wrapping_neg(), y, z];
}

// ---- Traffic -------------------------------------------------------------

/// The main road's lanes: x, speed in half city lengths (128 m) a cycle
/// (+: along +z), and where car 0 starts. Eight cars a lane, one a
/// block: every junction sees the same traffic. Speeds: 15 and 5 m/s
/// along the eye, 15 and 10 m/s against it (the eye: 10).
pub const LANES: [(i32, i32, i32); 4] = [
    (205, 3, 1907),
    (614, 1, 940),
    (-205, -3, 1065),
    (-614, -2, 1003),
];
/// The cross streets' lanes: z in the block (2 m: driving to +x, 6 m:
/// to -x), direction, where the car starts on a 128 m loop. One car a
/// lane per junction, 10 m/s; odd junctions half a loop later. The
/// start places are chosen so no crossing car ever meets a car on the
/// main road (`tests/city.rs` checks every tick of the cycle).
pub const CROSS: [(i32, i32, i32); 2] = [(256, 1, 8735), (768, -1, 15018)];
/// Half a city length.
const HALF: u32 = (LENGTH / 2) as u32;
/// A cross street's loop (128 m).
pub const LOOP: i32 = 2 * CROSS_HALF;

/// How far the eye has come in the city (0 to [`LENGTH`]).
pub fn eye_z(tick: u32) -> i32 {
    ((tick % CYCLE) * LENGTH as u32 / CYCLE) as i32
}

/// Where car `k` of main lane `lane` is (z in the city, 0 to LENGTH).
pub fn lane_car_z(lane: usize, k: u32, tick: u32) -> i32 {
    let (_, half_laps, start) = LANES[lane % 4];
    // Below 1536 * 3 * 16384: no overflow.
    let off = ((tick % CYCLE) * half_laps.unsigned_abs() * HALF / CYCLE) as i32;
    let off = if half_laps > 0 { off } else { -off };
    (start + (k % 8) as i32 * BLOCK + off).rem_euclid(LENGTH)
}

/// Where the car of cross lane `c` at the junction of block `b` is (x).
pub fn cross_car_x(c: usize, b: i32, tick: u32) -> i32 {
    let (_, dir, start) = CROSS[c % 2];
    let off = ((tick % CYCLE) * 2 * HALF / CYCLE) as i32;
    let odd = b.rem_euclid(2) * (LOOP / 2);
    (start + odd + dir * off).rem_euclid(LOOP) - CROSS_HALF
}

/// How far ahead cars are drawn at a load: a fixed distance for each
/// load, so a car comes into sight or leaves it only by crossing it (it
/// never blinks when the buildings' far end moves back). About the
/// stretch the `load` buildings cover on average (1.6 m of street per
/// building) plus 16 m, at least 48 m (a car there is about 4 pixels
/// wide), at most the road's [`ROAD_RANGE`]. It grows with the load.
pub const fn car_reach(load: u32) -> i32 {
    let l = if load > MAX_LOAD { MAX_LOAD } else { load };
    // 1.6 m = 205 / 128 m; at most 128 * 205 + 2048: no overflow.
    let r = l as i32 * 205 + 16 * CM;
    if r < 48 * CM {
        48 * CM
    } else if r > ROAD_RANGE {
        ROAD_RANGE
    } else {
        r
    }
}

/// A car's colour.
fn car_mat(seed: u32) -> u8 {
    CAR0 + (hash(seed, 5) % CAR_COLOURS) as u8
}

// ---- Drawing -------------------------------------------------------------

/// The order key: the distance (1/16 m) from the eye to the nearest
/// point of a ground plan (middle `x`, `z` from the eye, half sizes).
/// Keys of things drawn last (cars on the main road) are moved below
/// every other key.
fn order_key(x: i32, z: i32, hx: i32, hz: i32, last: bool) -> i16 {
    // Each below 2^13 after the shift: the sum fits.
    let dx = (x.wrapping_abs().wrapping_sub(hx)).max(0) >> 3;
    let dz = (z.wrapping_abs().wrapping_sub(hz)).max(0) >> 3;
    let d = isqrt((dx.wrapping_mul(dx).wrapping_add(dz.wrapping_mul(dz))) as u64) as i32;
    (if last { d.wrapping_sub(8192) } else { d }) as i16
}

/// A car on the road: its id, x, z from the eye, turning.
fn car_at(id: u16, ze: i32, tick: u32) -> (i32, i32, usize, u8) {
    let lane = usize::from((id >> 8) & 7);
    let k = u32::from(id & 0xFF);
    if lane < 4 {
        let (x, hl, _) = LANES[lane];
        let mut z = (lane_car_z(lane, k, tick) - ze).rem_euclid(LENGTH);
        if z >= LENGTH - 8 * CM {
            z -= LENGTH;
        }
        let colour = car_mat((lane as u32) * 8 + k % 4);
        (x, z, if hl > 0 { 0 } else { 1 }, colour)
    } else {
        let c = lane - 4;
        let b = k as i32 - 1;
        let (zc, dir, _) = CROSS[c];
        let z = b * BLOCK - ze + zc;
        let colour = car_mat(32 + (c as u32) * 8 + b.rem_euclid(BLOCKS) as u32);
        (
            cross_car_x(c, b, tick),
            z,
            if dir > 0 { 2 } else { 3 },
            colour,
        )
    }
}

/// The camera at a tick: world to camera, and the turn (for the sky).
pub fn camera(tick: u32) -> (Mat3, u16) {
    let t = tick % CYCLE;
    // 10 degrees each way, one sway in 512 ticks (8.5 s).
    let yaw = (sin((t as u16).wrapping_mul(128)).wrapping_mul(1820) >> 14) as u16;
    // 4 degrees down.
    let pitch = 0u16.wrapping_sub(728);
    (Mat3::rotation(yaw, pitch, 0).transposed(), yaw)
}

/// The first row of the ground (the horizon), within the work area.
pub fn horizon(cam: &Mat3, yaw: u16) -> i32 {
    let ahead = [sin(yaw), 0, sin(yaw.wrapping_add(16_384))];
    let c = cam.apply(ahead);
    let p = VIEW.project(c);
    first_row(p[1]).clamp(WORK.y0, WORK.y1)
}

/// One object of the city (one copy of the renderer's object stage for
/// the road, the buildings and the cars: smaller code).
#[inline(never)]
fn draw_one(r: &mut Renderer, f: &mut Frame, cam: &Mat3, paint: &CityPaint, o: &Object<'_>) {
    r.draw_with(f, &VIEW, cam, paint, o);
}

/// The city at tick `tick`: `load` buildings (at most [`MAX_LOAD`]).
pub fn draw(
    r: &mut Renderer,
    keys: &mut [i16; MAX_OBJ],
    list: &mut [u16; MAX_OBJ],
    f: &mut Frame,
    load: u32,
    tick: u32,
) -> Counts {
    r.begin();
    let ze = eye_z(tick);
    let (cam, yaw) = camera(tick);
    // The sky in three bands, the ground.
    let hz = horizon(&cam, yaw);
    let (y0, y1) = (WORK.y0, WORK.y1);
    let b1 = (hz - 14).clamp(y0, hz);
    let b2 = (hz - 5).clamp(y0, hz);
    rect_fill(f, &Rect::new(0, y0, 160, b1 - y0), SKY_HI);
    rect_fill(f, &Rect::new(0, b1, 160, b2 - b1), SKY_MID);
    rect_fill(f, &Rect::new(0, b2, 160, hz - b2), SKY_LOW);
    rect_fill(f, &Rect::new(0, hz, 160, y1 - hz), PAVE);

    // The road, block by block, far to near.
    let road = CityPaint {
        mats: [ASPHALT, LINE_Y, LINE_W, ASPHALT],
    };
    let first = ze / BLOCK;
    let last = (ze + ROAD_RANGE) / BLOCK;
    let mut b = last;
    while b >= first {
        let pos = [0, -EYE_Y, b * BLOCK + 16 * CM - ze];
        if let Some(c) = in_view(&VIEW, &cam, pos, ROAD_MODEL.radius, FAR) {
            let o = Object {
                model: &ROAD_MODEL,
                rot: Mat3::IDENTITY,
                centre: c,
            };
            draw_one(r, f, &cam, &road, &o);
        }
        b -= 1;
    }

    // The buildings: the `load` nearest ahead.
    let n = LOTS.len();
    let s0 = LOTS.partition_point(|l| l.far() <= ze);
    let take = load.min(MAX_LOAD) as usize;
    let mut count = 0usize;
    for s in 0..take {
        let i = s0 + s;
        let (idx, wrap) = if i >= n { (i - n, LENGTH) } else { (i, 0) };
        let l = &LOTS[idx];
        let z = i32::from(l.z) + wrap - ze;
        let x = i32::from(l.x);
        let pos = [x, l.mid_y() - EYE_Y, z];
        if in_view(&VIEW, &cam, pos, i32::from(l.radius), FAR).is_some() && count < MAX_OBJ {
            keys[count] = order_key(x, z, i32::from(l.hx), i32::from(l.hz), false);
            list[count] = idx as u16 | if wrap > 0 { 0x100 } else { 0 };
            count += 1;
        }
    }

    // The cars on about that stretch, to a fixed distance per load.
    let reach = car_reach(load);
    let near = -8 * CM;
    let car_r = CAR.radius;
    for lane in 0..4 {
        for k in 0..8u16 {
            let id = 0x8000 | (lane as u16) << 8 | k;
            let (x, z, ..) = car_at(id, ze, tick);
            if z > reach || count >= MAX_OBJ {
                continue;
            }
            if in_view(&VIEW, &cam, [x, CAR_Y - EYE_Y, z], car_r, FAR).is_some() {
                keys[count] = order_key(x, z, CAR_HW, CAR_HL, true);
                list[count] = id;
                count += 1;
            }
        }
    }
    let mut b = (ze + near).div_euclid(BLOCK);
    while b * BLOCK - ze <= reach {
        for c in 0..2u16 {
            // b is -1 to about 15.
            let id = 0x8000 | (4 + c) << 8 | (b + 1) as u16;
            let (x, z, ..) = car_at(id, ze, tick);
            if count < MAX_OBJ && in_view(&VIEW, &cam, [x, CAR_Y - EYE_Y, z], car_r, FAR).is_some()
            {
                // On the main road (its whole length): drawn last.
                let last = x.wrapping_abs() + CAR_HL <= ROAD + 64;
                keys[count] = order_key(x, z, CAR_HL, CAR_HW, last);
                list[count] = id;
                count += 1;
            }
        }
        b += 1;
    }

    // Far to near (insertion sort; the buildings come nearly sorted the
    // other way, so this is the slow case: at most 128 + 64 objects).
    for a in 1..count {
        let (key, id) = (keys[a], list[a]);
        let mut j = a;
        while j > 0 && keys[j - 1] < key {
            keys[j] = keys[j - 1];
            list[j] = list[j - 1];
            j -= 1;
        }
        keys[j] = key;
        list[j] = id;
    }

    let mut v = [[0i16; 3]; MAX_VERTS];
    for &id in &list[..count] {
        if id & 0x8000 == 0 {
            let l = &LOTS[usize::from(id & 0xFF)];
            let wrap = if id & 0x100 != 0 { LENGTH } else { 0 };
            let nv = bld_corners(l, &mut v);
            let (faces, normals) = bld_model_tables(usize::from(l.bands), l.top > 0);
            let m = Model {
                verts: &v[..nv],
                faces,
                normals,
                radius: i32::from(l.radius),
            };
            let pos = [
                i32::from(l.x),
                l.mid_y() - EYE_Y,
                i32::from(l.z) + wrap - ze,
            ];
            let o = Object {
                model: &m,
                rot: Mat3::IDENTITY,
                centre: cam.apply(pos),
            };
            let paint = CityPaint {
                mats: [l.wall, l.glass, ROOF, l.top_wall],
            };
            draw_one(r, f, &cam, &paint, &o);
        } else {
            let (x, z, turn, colour) = car_at(id, ze, tick);
            let o = Object {
                model: &CAR,
                rot: TURN[turn],
                centre: cam.apply([x, CAR_Y - EYE_Y, z]),
            };
            let paint = CityPaint {
                mats: [colour, GLASS0, ASPHALT, colour],
            };
            draw_one(r, f, &cam, &paint, &o);
        }
    }

    let s = r.stats;
    Counts {
        tris: s.tris,
        px: s.pixels,
        objs: s.objects,
        verts: s.verts,
        faces: s.faces,
        front: s.front,
        clipped: s.clipped,
        dropped: s.dropped,
        check: 0,
    }
}
