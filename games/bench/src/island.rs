//! ISLAND: the showcase study's floating island (study kept outside git),
//! drawn with the bench's renderer ([`crate::poly`]): an original box
//! figure runs a loop round a small island in the sky, jumps onto two
//! floating platforms and over the gap between them, runs through a
//! stone gate, climbs two terraces and runs down two ramps, followed by
//! a third-person camera. All by game time: one loop is [`LOOP`] ticks
//! of 1/60 s (12.8 s) at 30 and at 60 calls a second, and the picture
//! of tick `t + LOOP` is the picture of tick `t`.
//!
//! **The load** is a detail level, fixed per level (no ramp): LOW, MID
//! and HIGH are the study's three levels (tiles, tree shapes, tower
//! sides, bushes, fences, grass tufts, far islets); XL and XXL are HIGH
//! with a finer floor (28 x 28 and 56 x 56 tiles, made while drawing,
//! only the strips in view), finer trees (three tiers of ten sides) and
//! grass (16 blades), more trees, bushes and grass on the island and
//! more islets round it (11 more at XL, 36 more at XXL, placed while
//! compiling): on the board-fitted cost estimate XL is about at 60's
//! limit and XXL over 30's, so both limits fall inside the range. The
//! figure is one model at LOW and MID and two (legs, body)
//! from HIGH on, posed every tick (legs and arms swing; a jump pose).
//!
//! **Draw order** (no depth buffer; the study's rules): the far islets
//! first (far to near), then the island's underside and its floor tiles
//! in list order (everything else stands on or above them) and the
//! figure's shadow when it is on the floor; then every other object far
//! to near by the depth of its middle, where an object that stands on
//! another is always drawn after it (its key is kept below its
//! support's: the "parent" rule). The figure stands on its shadow,
//! which stands on what the figure is over.
//!
//! **Near the eye** (the study's near-bush fault: a bush or tree
//! between the camera and the figure was cut by the near plane and
//! showed as a big dark slab at the picture's edge): small things
//! (trees, bushes, fences, grass tufts, gems) are not drawn while their
//! ball reaches nearer than the near plane. The ground pieces, ramps,
//! platforms, gate and tower are never hidden; the route and the camera
//! are laid out so that the eye stays clear of them (a test checks it
//! for every tick at every level).
//!
//! **What is counted**: the paint has [`Paint::ONLY_PAINTED`], like the
//! city: TRIS counts triangles that painted a pixel.
//!
//! Units: the layout is written in centimetres ([`u`] turns them into
//! the renderer's 1/256 m). The models are made while compiling by
//! [`B`], a whole-number builder (`const fn`); only the figure's corners
//! and face directions are worked out at run time (in [`Scratch`]).

use game_kit::Frame;

use crate::art::{CITY_SHADES, CITY0, HAZE, ISLE_SHADES, ISLE0, SHADOW, SKY_HI, SKY_LOW};
use crate::fixed::{Mat3, ONE14, SHIFT14, V3, ccos, cos, csin, hash, isqrt, sin};
use crate::load::{Counts, FAR, MAX_OBJ, VIEW, WORK};
use crate::poly::{
    Face, M8, MAX_TRIS, MAX_VERTS, Model, NEAR, Object, Paint, Rect, Renderer, SUB, in_view,
    radius_of, rect_fill,
};

/// One loop of the figure's run, in ticks of 1/60 s (12.8 s).
pub const LOOP: u32 = 768;
/// The levels' names (load 1 to 5).
pub const LEVEL_NAMES: [&str; 5] = ["LOW", "MID", "HIGH", "XL", "XXL"];
/// Levels.
pub const LEVELS: usize = 5;

/// Centimetres to model units (1/256 m).
pub const fn u(cm: i32) -> i32 {
    cm * M8 / 100
}

// ---- Colours ------------------------------------------------------------------

/// The island's materials. 0 to 9 are its own colours (four shades,
/// [`crate::art::ISLE0`]); 10 to 18 are city colours (seven shades);
/// [`SHADE`] is the shadow's one flat colour.
pub const GRASS: u8 = 0;
pub const GRASS2: u8 = 1;
pub const DIRT: u8 = 2;
pub const TRUNK: u8 = 3;
pub const LEAF: u8 = 4;
pub const BLOCK: u8 = 5;
pub const BLOCK_TOP: u8 = 6;
pub const GEM: u8 = 7;
pub const SKIN: u8 = 8;
pub const BODY: u8 = 9;
pub const ROCK: u8 = 10;
pub const ROCK2: u8 = 11;
pub const STONE: u8 = 12;
pub const SAND: u8 = 13;
pub const TOWER: u8 = 14;
pub const ROOF: u8 = 15;
pub const PANTS: u8 = 16;
pub const HAIR: u8 = 17;
pub const PACK: u8 = 18;
pub const SHADE: u8 = 19;
const MATS: usize = 20;
/// The city colour each of materials 10 to 18 uses (slate, asphalt,
/// cream, sand, white, brick, dark glass, black, yellow).
const FROM_CITY: [u8; 9] = [10, 0, 3, 6, 9, 4, 11, 18, 15];

const fn colour_table() -> [[u8; 7]; MATS] {
    let mut t = [[0u8; 7]; MATS];
    let mut m = 0;
    while m < MATS {
        let mut s = 0;
        while s < 7 {
            t[m][s] = if m < 10 {
                ISLE0 + m as u8 * ISLE_SHADES + (s as u8 >> 1)
            } else if m < 19 {
                let c = FROM_CITY[m - 10];
                let k = if s as u8 >= CITY_SHADES {
                    CITY_SHADES - 1
                } else {
                    s as u8
                };
                CITY0 + c * CITY_SHADES + k
            } else {
                SHADOW
            };
            s += 1;
        }
        m += 1;
    }
    t
}

/// Palette number of a material at a shade.
static COLOUR: [[u8; 7]; MATS] = colour_table();

/// The light: from above, a little from the left and from behind the
/// figure's start (world, 16384 long, from the light to the scene).
pub const LIGHT: V3 = light();

const fn light() -> V3 {
    let d = [-35i64, -85, 40];
    let len = isqrt((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as u64) as i64;
    [
        (d[0] * ONE14 as i64 / len) as i32,
        (d[1] * ONE14 as i64 / len) as i32,
        (d[2] * ONE14 as i64 / len) as i32,
    ]
}

/// The island's paint: one light, a colour table, faces sorted far to
/// near inside an object, only painted triangles counted.
#[derive(Clone, Copy, Debug)]
pub struct IslandPaint;

impl Paint for IslandPaint {
    const ONLY_PAINTED: bool = true;

    #[inline(always)]
    fn dir(&self) -> V3 {
        LIGHT
    }

    #[inline(always)]
    fn colour(&self, mat: u8, shade: u8) -> u8 {
        COLOUR[usize::from(mat) % MATS][usize::from(shade).min(6)]
    }
}

// ---- The model builder (runs while compiling) -----------------------------------

/// Convex parts a model may have.
const MAX_PARTS: usize = 40;

/// Where a model's origin goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// The middle of its corners' box.
    Middle,
    /// x and z 0, y the middle of the box (models placed and turned
    /// round their own upright axis).
    Upright,
    /// Given (units).
    At([i32; 3]),
}

/// A model being made: corners (units), faces (three corners, material,
/// part), each convex part's middle (faces are turned to look away from
/// it), and for the figure each corner's limb.
#[derive(Clone, Copy)]
pub struct B {
    v: [[i32; 3]; MAX_VERTS],
    pub nv: usize,
    f: [[u8; 5]; MAX_TRIS],
    pub nf: usize,
    mids: [[i32; 3]; MAX_PARTS],
    np: usize,
    g: [u8; MAX_VERTS],
    grp: u8,
    origin: Origin,
}

impl B {
    const fn with(origin: Origin) -> B {
        B {
            v: [[0; 3]; MAX_VERTS],
            nv: 0,
            f: [[0; 5]; MAX_TRIS],
            nf: 0,
            mids: [[0; 3]; MAX_PARTS],
            np: 0,
            g: [0; MAX_VERTS],
            grp: 0,
            origin,
        }
    }

    /// A model whose origin is the middle of its box (written in world
    /// places).
    pub const fn world() -> B {
        B::with(Origin::Middle)
    }

    /// A model made round its own upright axis (placed and turned).
    pub const fn upright() -> B {
        B::with(Origin::Upright)
    }

    /// A model with its origin at `cm`.
    pub const fn at(cm: [i32; 3]) -> B {
        B::with(Origin::At([u(cm[0]), u(cm[1]), u(cm[2])]))
    }

    const fn vert(&mut self, p: [i32; 3]) -> u8 {
        assert!(self.nv < MAX_VERTS, "island model: more than 64 corners");
        self.v[self.nv] = p;
        self.g[self.nv] = self.grp;
        self.nv += 1;
        (self.nv - 1) as u8
    }

    const fn part(&mut self, mid: [i32; 3]) -> u8 {
        assert!(self.np < MAX_PARTS, "island model: too many parts");
        self.mids[self.np] = mid;
        self.np += 1;
        (self.np - 1) as u8
    }

    const fn tri(&mut self, a: u8, b: u8, c: u8, mat: u8, part: u8) {
        assert!(self.nf < MAX_TRIS, "island model: more than 128 faces");
        self.f[self.nf] = [a, b, c, mat, part];
        self.nf += 1;
    }

    const fn quad(&mut self, q: [u8; 4], mat: u8, part: u8) {
        self.tri(q[0], q[1], q[2], mat, part);
        self.tri(q[0], q[2], q[3], mat, part);
    }

    /// A box from `lo` to `hi` (cm). Materials: top, sides, bottom
    /// (`None`: no such face).
    pub const fn cube(
        &mut self,
        lo: [i32; 3],
        hi: [i32; 3],
        top: Option<u8>,
        side: u8,
        bottom: Option<u8>,
    ) {
        let lo = [u(lo[0]), u(lo[1]), u(lo[2])];
        let hi = [u(hi[0]), u(hi[1]), u(hi[2])];
        let mut c = [0u8; 8];
        let mut i = 0;
        while i < 8 {
            c[i] = self.vert([
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]);
            i += 1;
        }
        let part = self.part([
            (lo[0] + hi[0]) / 2,
            (lo[1] + hi[1]) / 2,
            (lo[2] + hi[2]) / 2,
        ]);
        if let Some(t) = top {
            self.quad([c[2], c[3], c[7], c[6]], t, part);
        }
        if let Some(b) = bottom {
            self.quad([c[0], c[1], c[5], c[4]], b, part);
        }
        self.quad([c[0], c[1], c[3], c[2]], side, part);
        self.quad([c[4], c[5], c[7], c[6]], side, part);
        self.quad([c[0], c[4], c[6], c[2]], side, part);
        self.quad([c[1], c[5], c[7], c[3]], side, part);
    }

    /// Corner `k` of `n` round `c` (units) at radius `r` (units).
    const fn ring(c: [i32; 3], r: i32, k: usize, n: usize, turn: u16) -> [i32; 3] {
        let a = ((k * 65_536 / n) as u32 as u16).wrapping_add(turn);
        [
            c[0] + ((csin(a) * r) >> SHIFT14),
            c[1],
            c[2] + ((ccos(a) * r) >> SHIFT14),
        ]
    }

    /// An upright prism of `n` sides round `c` (cm, the base's middle),
    /// radius `r`, height `h`, no top or bottom.
    pub const fn prism(&mut self, n: usize, c: [i32; 3], r: i32, h: i32, turn: u16, side: u8) {
        let (cu, ru, hu) = ([u(c[0]), u(c[1]), u(c[2])], u(r), u(h));
        let first = self.nv as u8;
        let mut k = 0;
        while k < n {
            let p = B::ring(cu, ru, k, n, turn);
            self.vert(p);
            self.vert([p[0], p[1] + hu, p[2]]);
            k += 1;
        }
        let part = self.part([cu[0], cu[1] + hu / 2, cu[2]]);
        let mut k = 0;
        while k < n {
            let j = (k + 1) % n;
            let (lo_k, hi_k) = (first + 2 * k as u8, first + 2 * k as u8 + 1);
            let (lo_j, hi_j) = (first + 2 * j as u8, first + 2 * j as u8 + 1);
            self.quad([lo_k, lo_j, hi_j, hi_k], side, part);
            k += 1;
        }
    }

    /// A cone of `n` sides: base round `c` (cm) at its height, radius
    /// `r`, tip `h` above (below when `h` < 0); `base` closes the base.
    #[allow(clippy::too_many_arguments)]
    pub const fn cone(
        &mut self,
        n: usize,
        c: [i32; 3],
        r: i32,
        h: i32,
        turn: u16,
        mat: u8,
        base: Option<u8>,
    ) {
        let (cu, ru, hu) = ([u(c[0]), u(c[1]), u(c[2])], u(r), u(h));
        let tip = self.vert([cu[0], cu[1] + hu, cu[2]]);
        let first = self.nv as u8;
        let mut k = 0;
        while k < n {
            self.vert(B::ring(cu, ru, k, n, turn));
            k += 1;
        }
        let part = self.part([cu[0], cu[1] + hu / 4, cu[2]]);
        let mut k = 0;
        while k < n {
            let j = (k + 1) % n;
            self.tri(tip, first + k as u8, first + j as u8, mat, part);
            k += 1;
        }
        if let Some(b) = base {
            let mut k = 1;
            while k + 1 < n {
                self.tri(first, first + k as u8, first + k as u8 + 1, b, part);
                k += 1;
            }
        }
    }

    /// A ramp over x0..x1, rising along +z from (z0, y0) to (z1, y1).
    #[allow(clippy::too_many_arguments)]
    pub const fn ramp(
        &mut self,
        x0: i32,
        x1: i32,
        z0: i32,
        z1: i32,
        y0: i32,
        y1: i32,
        top: u8,
        side: u8,
    ) {
        let a = self.vert([u(x0), u(y0), u(z0)]);
        let b = self.vert([u(x1), u(y0), u(z0)]);
        let c = self.vert([u(x1), u(y0), u(z1)]);
        let d = self.vert([u(x0), u(y0), u(z1)]);
        let e = self.vert([u(x1), u(y1), u(z1)]);
        let g = self.vert([u(x0), u(y1), u(z1)]);
        let part = self.part([
            u((x0 + x1) / 2),
            u(y0 + (y1 - y0) / 3),
            u(z0 + 2 * (z1 - z0) / 3),
        ]);
        self.quad([a, b, e, g], top, part);
        self.quad([d, c, e, g], side, part);
        self.tri(a, d, g, side, part);
        self.tri(b, c, e, side, part);
    }

    /// A gem: an eight-sided double pyramid round `c` (cm), half width
    /// `r`, half height `h`.
    pub const fn gem(&mut self, c: [i32; 3], r: i32, h: i32, mat: u8) {
        let cu = [u(c[0]), u(c[1]), u(c[2])];
        let top = self.vert([cu[0], cu[1] + u(h), cu[2]]);
        let bot = self.vert([cu[0], cu[1] - u(h), cu[2]]);
        let first = self.nv as u8;
        let mut k = 0;
        while k < 4 {
            self.vert(B::ring(cu, u(r), k, 4, 0));
            k += 1;
        }
        let part = self.part(cu);
        let mut k = 0;
        while k < 4 {
            let j = (k + 1) % 4;
            self.tri(top, first + k as u8, first + j as u8, mat, part);
            self.tri(bot, first + k as u8, first + j as u8, mat, part);
            k += 1;
        }
    }

    /// A flat floor at height `y` over x0..x1, z0..z1 (cm) in nx by nz
    /// tiles of two materials in a checker (row numbers start at
    /// `row0`); tiles wholly inside `skip` (x0, z0, x1, z1) are left out.
    /// Faces look up.
    #[allow(clippy::too_many_arguments)]
    pub const fn floor(
        &mut self,
        x: [i32; 2],
        z: [i32; 2],
        y: i32,
        nx: i32,
        nz: i32,
        row0: i32,
        mats: [u8; 2],
        skip: Option<[i32; 4]>,
    ) {
        assert!((nx + 1) * (nz + 1) <= 256, "island floor: too many tiles");
        let part = self.part([u((x[0] + x[1]) / 2), u(y - 100), u((z[0] + z[1]) / 2)]);
        let mut grid = [255u8; 256];
        let mut k = 0;
        while k < nz {
            let mut i = 0;
            while i < nx {
                let xa = x[0] + (x[1] - x[0]) * i / nx;
                let xb = x[0] + (x[1] - x[0]) * (i + 1) / nx;
                let za = z[0] + (z[1] - z[0]) * k / nz;
                let zb = z[0] + (z[1] - z[0]) * (k + 1) / nz;
                let skipped = match skip {
                    Some(s) => xa >= s[0] && xb <= s[2] && za >= s[1] && zb <= s[3],
                    None => false,
                };
                if !skipped {
                    let mut q = [0u8; 4];
                    let corners = [(0, 0), (1, 0), (1, 1), (0, 1)];
                    let mut n = 0;
                    while n < 4 {
                        let (di, dk) = corners[n];
                        let g = ((k + dk) * (nx + 1) + i + di) as usize;
                        if grid[g] == 255 {
                            let px = x[0] + (x[1] - x[0]) * (i + di) / nx;
                            let pz = z[0] + (z[1] - z[0]) * (k + dk) / nz;
                            grid[g] = self.vert([u(px), u(y), u(pz)]);
                        }
                        q[n] = grid[g];
                        n += 1;
                    }
                    let mat = mats[((i + k + row0) % 2) as usize];
                    self.quad(q, mat, part);
                }
                i += 1;
            }
            k += 1;
        }
    }

    /// A flat disc of `n` corners at the origin's height round it,
    /// radius `r` (cm; the shadow). Faces look up.
    pub const fn disc(&mut self, n: usize, r: i32, mat: u8) {
        let part = self.part([0, u(-100), 0]);
        let first = self.nv as u8;
        let mut k = 0;
        while k < n {
            self.vert(B::ring([0, 0, 0], u(r), k, n, 0));
            k += 1;
        }
        let mut k = 1;
        while k + 1 < n {
            self.tri(first, first + k as u8, first + k as u8 + 1, mat, part);
            k += 1;
        }
    }

    /// The model's origin (units).
    pub const fn origin(&self) -> [i32; 3] {
        let mut lo = [i32::MAX; 3];
        let mut hi = [i32::MIN; 3];
        let mut i = 0;
        while i < self.nv {
            let mut k = 0;
            while k < 3 {
                let p = self.v[i][k];
                if p < lo[k] {
                    lo[k] = p;
                }
                if p > hi[k] {
                    hi[k] = p;
                }
                k += 1;
            }
            i += 1;
        }
        let mid = [
            (lo[0] + hi[0]) / 2,
            (lo[1] + hi[1]) / 2,
            (lo[2] + hi[2]) / 2,
        ];
        match self.origin {
            Origin::Middle => mid,
            Origin::Upright => [0, mid[1], 0],
            Origin::At(o) => o,
        }
    }
}

/// A finished model: corners round the origin, faces turned outwards,
/// face directions, radius.
pub struct Built<const V: usize, const F: usize> {
    pub verts: [[i16; 3]; V],
    pub faces: [Face; F],
    pub normals: [[i16; 3]; F],
    pub radius: i32,
}

/// Finishes `b` (while compiling).
pub const fn trim<const V: usize, const F: usize>(b: &B) -> Built<V, F> {
    assert!(V == b.nv && F == b.nf);
    let o = b.origin();
    let mut verts = [[0i16; 3]; V];
    let mut i = 0;
    while i < V {
        let mut k = 0;
        while k < 3 {
            let d = b.v[i][k] - o[k];
            assert!(d > -32_768 && d < 32_768, "island model: corner too far");
            verts[i][k] = d as i16;
            k += 1;
        }
        i += 1;
    }
    let mut faces = [Face { v: [0; 3], mat: 0 }; F];
    let mut normals = [[0i16; 3]; F];
    let mut i = 0;
    while i < F {
        let fr = b.f[i];
        let m = b.mids[fr[4] as usize];
        let (a, bb, c) = (
            b.v[fr[0] as usize],
            b.v[fr[1] as usize],
            b.v[fr[2] as usize],
        );
        let e1 = [
            (bb[0] - a[0]) as i64,
            (bb[1] - a[1]) as i64,
            (bb[2] - a[2]) as i64,
        ];
        let e2 = [
            (c[0] - a[0]) as i64,
            (c[1] - a[1]) as i64,
            (c[2] - a[2]) as i64,
        ];
        let mut n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let out = [
            (a[0] - m[0]) as i64,
            (a[1] - m[1]) as i64,
            (a[2] - m[2]) as i64,
        ];
        let dot = n[0] * out[0] + n[1] * out[1] + n[2] * out[2];
        let v = if dot < 0 {
            n = [-n[0], -n[1], -n[2]];
            [fr[0], fr[2], fr[1]]
        } else {
            [fr[0], fr[1], fr[2]]
        };
        let len = isqrt((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) as u64) as i64;
        assert!(len > 0, "island model: a face with no area");
        normals[i] = [
            (n[0] * ONE14 as i64 / len) as i16,
            (n[1] * ONE14 as i64 / len) as i16,
            (n[2] * ONE14 as i64 / len) as i16,
        ];
        faces[i] = Face { v, mat: fr[3] };
        i += 1;
    }
    let radius = radius_of(&verts);
    Built {
        verts,
        faces,
        normals,
        radius,
    }
}

/// A finished floor: like [`Built`], every face looking straight up
/// (checked), so the faces share [`UPS`].
pub struct Flat<const V: usize, const F: usize> {
    pub verts: [[i16; 3]; V],
    pub faces: [Face; F],
    pub radius: i32,
}

pub const fn trim_flat<const V: usize, const F: usize>(b: &B) -> Flat<V, F> {
    let m: Built<V, F> = trim(b);
    let mut i = 0;
    while i < F {
        let n = m.normals[i];
        assert!(
            n[0] == 0 && n[1] == ONE14 as i16 && n[2] == 0,
            "a floor face not flat"
        );
        i += 1;
    }
    Flat {
        verts: m.verts,
        faces: m.faces,
        radius: m.radius,
    }
}

/// Straight up, for every flat floor face.
static UPS: [[i16; 3]; FINE_FACES] = [[0, ONE14 as i16, 0]; FINE_FACES];

/// Makes a `static` [`Model`] from a builder expression.
macro_rules! model {
    ($name:ident, $b:expr) => {
        static $name: Model<'static> = {
            const BB: B = $b;
            const TT: Built<{ BB.nv }, { BB.nf }> = trim(&BB);
            static T: Built<{ BB.nv }, { BB.nf }> = TT;
            Model {
                verts: &T.verts,
                faces: &T.faces,
                normals: &T.normals,
                radius: TT.radius,
            }
        };
    };
    ($name:ident, $b:expr, flat) => {
        static $name: Model<'static> = {
            const BB: B = $b;
            const TT: Flat<{ BB.nv }, { BB.nf }> = trim_flat(&BB);
            const _: () = assert!(BB.nf <= FINE_FACES, "a floor with more faces than UPS");
            static T: Flat<{ BB.nv }, { BB.nf }> = TT;
            Model {
                verts: &T.verts,
                faces: &T.faces,
                normals: &UPS,
                radius: TT.radius,
            }
        };
    };
}

// ---- The layout (cm) -----------------------------------------------------------

/// The island: x and z from -700 to 700, its floor at 0.
const ISLE: i32 = 700;
/// The first terrace (x0, z0, x1, z1, top) and the second, on it.
const T1: [i32; 5] = [-700, 150, -150, 700, 100];
const T2: [i32; 5] = [-700, 450, -400, 700, 200];
/// The ramps: x0, x1, z0, z1, y0, y1 (rising along +z).
pub const RAMP1: [i32; 6] = [-450, -250, -150, 150, 0, 100];
pub const RAMP2: [i32; 6] = [-540, -410, 250, 450, 100, 200];
/// The tower on the second terrace (middle of its base) and its radius.
const TOWER_AT: [i32; 3] = [-590, 0, -610];
const TOWER_R: i32 = 70;
const TOWER_H: i32 = 240;
/// Platform middles and half sizes (x and z, y).
const PLATS: [([i32; 3], i32); 4] = [
    ([220, 90, 20], 70),
    ([400, 170, 170], 70),
    ([580, 250, -120], 60),
    ([-20, 200, 320], 60),
];
const PLAT_HY: i32 = 20;
/// The gate's pillars' height (the beam's bottom), cm.
const GATE_Y: i32 = 300;

// ---- The models ------------------------------------------------------------------

/// Tiles across the floor, terrace tiles, second terrace tiles,
/// underside sides, tower sides, shadow corners: LOW, MID, HIGH.
const TILES: [i32; 3] = [4, 8, 14];
const T1N: [i32; 3] = [1, 4, 6];
const T2N: [i32; 3] = [1, 2, 3];
const UNDER: [usize; 3] = [4, 8, 12];
const TOWER_N: [usize; 3] = [4, 8, 10];
const DISC_N: [usize; 3] = [6, 8, 10];

const fn islet(k: i32) -> B {
    let s = 120 + 40 * k;
    let mut b = B::upright();
    b.cube([-s, -30, -s], [s, 0, s], Some(GRASS), DIRT, None);
    b.cone(4, [0, -30, 0], s * 1414 / 1000, -2 * s, 8192, ROCK, None);
    b.cube([-8, 0, -8], [8, 40, 8], None, TRUNK, None);
    b.cone(6, [0, 35, 0], 50, 110, 0, LEAF, None);
    b
}

const fn under(d: usize) -> B {
    let mut b = B::world();
    b.cube([-ISLE, -60, -ISLE], [ISLE, 0, ISLE], None, DIRT, None);
    let n = UNDER[d];
    if n == 4 {
        b.cone(4, [0, -60, 0], ISLE * 1414 / 1000, -540, 8192, ROCK, None);
    } else {
        b.cone(n, [0, -60, 0], 760, -200, 0, ROCK, None);
        b.cone(n, [0, -260, 0], 560, -360, 2048, ROCK2, None);
    }
    b
}

/// Rows of floor tiles per strip (at most 64 corners a model).
const fn rows_per(d: usize) -> i32 {
    let r = 64 / (TILES[d] + 1) - 1;
    if r < 1 { 1 } else { r }
}

/// Floor strip `s` of detail `d` (all strips share the origin 0, so
/// shared edges land on the same pixels).
const fn strip(d: usize, s: i32) -> B {
    let tiles = TILES[d];
    let step = 2 * ISLE / tiles;
    let r0 = s * rows_per(d);
    let mut r1 = r0 + rows_per(d);
    if r1 > tiles {
        r1 = tiles;
    }
    let mut b = B::at([0, 0, 0]);
    b.floor(
        [-ISLE, ISLE],
        [-ISLE + r0 * step, -ISLE + r1 * step],
        0,
        tiles,
        r1 - r0,
        r0,
        [GRASS, GRASS2],
        Some([T1[0], T1[1], T1[2], T1[3]]),
    );
    b
}

/// The fine floors of XL and XXL: tiles across, rows of tiles per
/// strip, the radius of a strip's ball (cm). Each half of the island
/// (x below or above 0) is cut into strips of whole rows; the strips
/// are made while drawing (only the ones in view), straight into the
/// figure's scratch, so they take no room in the game's data.
const FINE: [(i32, i32, i32); 2] = [(28, 3, 360), (56, 1, 352)];

/// Fine floor strips at XL (`k` 0) and XXL (1), both halves.
const fn fine_strips(k: usize) -> usize {
    let (n, rows, _) = FINE[k];
    (2 * ((n + rows - 1) / rows)) as usize
}

/// Most faces of a fine strip (two a tile) and of a flat floor: the
/// length of [`UPS`] and of the scratch's faces.
const FINE_FACES: usize = 92;
const _: () = {
    let mut k = 0;
    while k < 2 {
        let (n, rows, _) = FINE[k];
        assert!((n / 2) * rows * 2 <= FINE_FACES as i32);
        assert!((n / 2 + 1) * (rows + 1) <= MAX_VERTS as i32);
        k += 1;
    }
};

/// One quad over the whole island: its radius is the fine strips'
/// model radius (it is never drawn).
const fn fine_box() -> B {
    let mut b = B::at([0, 0, 0]);
    b.floor(
        [-ISLE, ISLE],
        [-ISLE, ISLE],
        0,
        1,
        1,
        0,
        [GRASS, GRASS],
        None,
    );
    b
}

const fn terrace(d: usize) -> B {
    let mut b = B::world();
    b.cube([T1[0], -2, T1[1]], [T1[2], T1[4], T1[3]], None, STONE, None);
    b.floor(
        [T1[0], T1[2]],
        [T1[1], T1[3]],
        T1[4],
        T1N[d],
        T1N[d],
        0,
        [GRASS, GRASS2],
        None,
    );
    b
}

const fn level2(d: usize) -> B {
    let mut b = B::world();
    b.cube(
        [T2[0], T2[4] - 100, T2[1]],
        [T2[2], T2[4], T2[3]],
        None,
        STONE,
        None,
    );
    b.floor(
        [T2[0], T2[2]],
        [T2[1], T2[3]],
        T2[4],
        T2N[d],
        T2N[d],
        0,
        [GRASS, GRASS2],
        None,
    );
    b
}

const fn ramp(r: [i32; 6]) -> B {
    let mut b = B::world();
    b.ramp(r[0], r[1], r[2], r[3], r[4], r[5], SAND, STONE);
    b
}

const fn tower(d: usize) -> B {
    let mut b = B::world();
    b.prism(TOWER_N[d], TOWER_AT, TOWER_R, TOWER_H, 4096, TOWER);
    b
}

/// The roof: the tower's top corners and no floor (an overhang seen
/// from below was drawn over the tower in the study).
const fn roof(d: usize) -> B {
    let mut b = B::world();
    b.cone(
        TOWER_N[d],
        [TOWER_AT[0], TOWER_AT[1] + TOWER_H, TOWER_AT[2]],
        TOWER_R,
        130,
        4096,
        ROOF,
        None,
    );
    b
}

/// The gate's beam on its two pillars (the pillars have no tops).
const fn beam() -> B {
    let mut b = B::world();
    b.cube(
        [170, GATE_Y, 420],
        [590, GATE_Y + 50, 480],
        Some(STONE),
        STONE,
        Some(STONE),
    );
    b
}

const fn pillars() -> B {
    let mut b = B::upright();
    b.cube([-30, 0, -30], [30, GATE_Y, 30], None, STONE, None);
    b
}

const fn plat(h: i32) -> B {
    let mut b = B::upright();
    b.cube(
        [-h, -PLAT_HY, -h],
        [h, PLAT_HY, h],
        Some(BLOCK_TOP),
        BLOCK,
        Some(BLOCK),
    );
    b
}

/// A tree: trunk and leaves (a pyramid at LOW, a cone at MID, two
/// tiers at HIGH, three finer tiers from XL).
const fn tree(d: usize) -> B {
    let mut b = B::upright();
    b.cube([-12, 0, -12], [12, 60, 12], None, TRUNK, None);
    match d {
        0 => b.cone(4, [0, 50, 0], 75, 170, 0, LEAF, None),
        1 => b.cone(7, [0, 50, 0], 75, 170, 0, LEAF, Some(LEAF)),
        2 => {
            b.cone(8, [0, 45, 0], 85, 120, 0, LEAF, Some(LEAF));
            b.cone(8, [0, 125, 0], 62, 115, 4096, LEAF, Some(LEAF));
        }
        _ => {
            b.cone(10, [0, 45, 0], 85, 95, 0, LEAF, Some(LEAF));
            b.cone(10, [0, 105, 0], 68, 85, 3277, LEAF, Some(LEAF));
            b.cone(10, [0, 160, 0], 48, 80, 6554, LEAF, Some(LEAF));
        }
    }
    b
}

const fn bush() -> B {
    let mut b = B::upright();
    b.cube([-22, 0, -22], [22, 34, 22], Some(LEAF), LEAF, None);
    b
}

/// A short fence along z: four posts and a rail.
const fn fence() -> B {
    let mut b = B::upright();
    let mut k = 0;
    while k < 4 {
        let t = -210 + k * 140;
        b.cube([-6, 0, t - 6], [6, 55, t + 6], Some(TRUNK), TRUNK, None);
        k += 1;
    }
    b.cube([-4, 38, -216], [4, 46, 216], Some(TRUNK), TRUNK, None);
    b
}

/// A patch of grass: `n` small three-sided pyramids (12 at HIGH, 16
/// from XL).
const fn tufts(n: u32) -> B {
    let mut b = B::upright();
    let mut k = 0;
    while k < n {
        let h = hash(7, k);
        let x = (h % 160) as i32 - 80;
        let z = ((h >> 12) % 160) as i32 - 80;
        b.cone(3, [x, 0, z], 12, 22, (h >> 20) as u16, GRASS2, None);
        k += 1;
    }
    b
}

const fn gem() -> B {
    let mut b = B::upright();
    b.gem([0, 0, 0], 20, 32, GEM);
    b
}

const fn disc(d: usize, r: i32) -> B {
    let mut b = B::at([0, 0, 0]);
    b.disc(DISC_N[d], r, SHADE);
    b
}

model!(ISLET0, islet(0));
model!(ISLET1, islet(1));
model!(ISLET2, islet(2));
model!(UNDER0, under(0));
model!(UNDER1, under(1));
model!(UNDER2, under(2));
model!(FLOOR0_0, strip(0, 0), flat);
model!(FLOOR1_0, strip(1, 0), flat);
model!(FLOOR1_1, strip(1, 1), flat);
model!(FLOOR2_0, strip(2, 0), flat);
model!(FLOOR2_1, strip(2, 1), flat);
model!(FLOOR2_2, strip(2, 2), flat);
model!(FLOOR2_3, strip(2, 3), flat);
model!(FLOOR2_4, strip(2, 4), flat);
model!(TERRACE0, terrace(0));
model!(TERRACE1, terrace(1));
model!(TERRACE2, terrace(2));
model!(LEVEL2_0, level2(0));
model!(LEVEL2_1, level2(1));
model!(LEVEL2_2, level2(2));
model!(RAMP1_M, ramp(RAMP1));
model!(RAMP2_M, ramp(RAMP2));
model!(TOWER0, tower(0));
model!(TOWER1, tower(1));
model!(TOWER2, tower(2));
model!(ROOF0, roof(0));
model!(ROOF1, roof(1));
model!(ROOF2, roof(2));
model!(BEAM_M, beam());
model!(PILLARS_M, pillars());
model!(PLAT70, plat(70));
model!(PLAT60, plat(60));
model!(TREE0, tree(0));
model!(TREE1, tree(1));
model!(TREE2, tree(2));
model!(BUSH_M, bush());
model!(FENCE_M, fence());
model!(TUFTS_M, tufts(12));
model!(TREE3, tree(3));
model!(TUFTS_X, tufts(16));
model!(FINE_M, fine_box(), flat);
model!(GEM_M, gem());
model!(DISC0, disc(0, 28));
model!(DISC1, disc(1, 28));
model!(DISC2, disc(2, 28));
model!(DISC0S, disc(0, 20));
model!(DISC1S, disc(1, 20));
model!(DISC2S, disc(2, 20));

/// Every placed model, by number (the [`Thing`] table names them so).
static MODELS: [&Model<'static>; 42] = [
    &ISLET0, &ISLET1, &ISLET2, &UNDER0, &UNDER1, &UNDER2, &FLOOR0_0, &FLOOR1_0, &FLOOR1_1,
    &FLOOR2_0, &FLOOR2_1, &FLOOR2_2, &FLOOR2_3, &FLOOR2_4, &TERRACE0, &TERRACE1, &TERRACE2,
    &LEVEL2_0, &LEVEL2_1, &LEVEL2_2, &RAMP1_M, &RAMP2_M, &TOWER0, &TOWER1, &TOWER2, &ROOF0, &ROOF1,
    &ROOF2, &BEAM_M, &PILLARS_M, &PLAT70, &PLAT60, &TREE0, &TREE1, &TREE2, &BUSH_M, &FENCE_M,
    &TUFTS_M, &GEM_M, &TREE3, &TUFTS_X, &FINE_M,
];

/// Each model's origin (units), in the same order, kept as 16 bits
/// (checked) in a `static`: a `const` table read with a run-time index
/// is copied into the data slot as 32-bit numbers.
static ORIGINS: [[i16; 3]; 42] = short(ORIGINS32);

const fn short<const N: usize>(a: [[i32; 3]; N]) -> [[i16; 3]; N] {
    let mut out = [[0; 3]; N];
    let mut i = 0;
    while i < N {
        let mut k = 0;
        while k < 3 {
            assert!(a[i][k] >= i16::MIN as i32 && a[i][k] <= i16::MAX as i32);
            out[i][k] = a[i][k] as i16;
            k += 1;
        }
        i += 1;
    }
    out
}

const ORIGINS32: [[i32; 3]; 42] = [
    islet(0).origin(),
    islet(1).origin(),
    islet(2).origin(),
    under(0).origin(),
    under(1).origin(),
    under(2).origin(),
    [0; 3],
    [0; 3],
    [0; 3],
    [0; 3],
    [0; 3],
    [0; 3],
    [0; 3],
    [0; 3],
    terrace(0).origin(),
    terrace(1).origin(),
    terrace(2).origin(),
    level2(0).origin(),
    level2(1).origin(),
    level2(2).origin(),
    ramp(RAMP1).origin(),
    ramp(RAMP2).origin(),
    tower(0).origin(),
    tower(1).origin(),
    tower(2).origin(),
    roof(0).origin(),
    roof(1).origin(),
    roof(2).origin(),
    beam().origin(),
    pillars().origin(),
    plat(70).origin(),
    plat(60).origin(),
    tree(0).origin(),
    tree(1).origin(),
    tree(2).origin(),
    bush().origin(),
    fence().origin(),
    tufts(12).origin(),
    gem().origin(),
    tree(3).origin(),
    tufts(16).origin(),
    fine_box().origin(),
];

// Model numbers.
const M_ISLET: u8 = 0;
const M_UNDER: u8 = 3;
const M_TERRACE: u8 = 14;
const M_LEVEL2: u8 = 17;
const M_RAMP1: u8 = 20;
const M_RAMP2: u8 = 21;
const M_TOWER: u8 = 22;
const M_ROOF: u8 = 25;
const M_BEAM: u8 = 28;
const M_PILLARS: u8 = 29;
const M_PLAT70: u8 = 30;
const M_PLAT60: u8 = 31;
const M_TREE: u8 = 32;
const M_BUSH: u8 = 35;
const M_FENCE: u8 = 36;
const M_TUFTS: u8 = 37;
const M_GEM: u8 = 38;
const M_TREE3: u8 = 39;
const M_TUFTS_X: u8 = 40;
/// The floor strips of each detail: first model number, count.
const FLOORS: [(u8, u8); 3] = [(6, 1), (7, 2), (9, 5)];

static DISCS: [[&Model<'static>; 3]; 2] = [[&DISC0, &DISC1, &DISC2], [&DISC0S, &DISC1S, &DISC2S]];

// ---- The things of the island ----------------------------------------------------

/// Roles: what a thing is to the things that stand on it (the parent
/// rule) and to the figure (what it stands on).
pub const NONE: u8 = 0;
pub const ON_T1: u8 = 1;
pub const ON_T2: u8 = 2;
pub const ON_TOWER: u8 = 3;
pub const ON_BEAM: u8 = 4;
pub const ON_RAMP1: u8 = 5;
pub const ON_RAMP2: u8 = 6;
/// Platforms 1 to 4: `ON_P1 + k`.
pub const ON_P1: u8 = 7;
pub const ON_SHADOW: u8 = 11;
pub const ON_LEGS: u8 = 12;
/// The gate's pillars (the beam rests on them).
pub const ON_GATE: u8 = 13;
/// The nearer of the two pillars' keys (kept apart from [`ON_GATE`],
/// which keeps the farther).
const GATE_NEAR: u8 = 14;
const ROLES: usize = 15;

/// Level masks (bit `l`: drawn at level `l`, LOW = 0).
const LOW: u8 = 0b00001;
const MID: u8 = 0b00010;
const HIGH_UP: u8 = 0b11100;
const HIGH: u8 = 0b00100;
const XL_UP: u8 = 0b11000;

/// The levels from `from` on.
const fn from(l: u8) -> u8 {
    (0b11111u8 << l) & 0b11111
}

/// One thing of the island.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thing {
    /// Model number (in [`MODELS`]).
    pub model: u8,
    /// Where its origin's place is (cm): for models made in world
    /// places 0, for the rest the foot of their upright axis.
    pub at: [i16; 3],
    /// Turn round the upright axis, 1/256 of a turn.
    pub turn: u8,
    /// Its role, and the role of what it stands on.
    pub role: u8,
    pub on: u8,
    /// 0: far islets, 1: underside and floor (list order), 2: the rest.
    pub layer: u8,
    /// Levels it is drawn at.
    pub levels: u8,
    /// A small thing: not drawn while it reaches nearer than the near
    /// plane.
    pub hide: bool,
}

const NO_THING: Thing = Thing {
    model: 0,
    at: [0; 3],
    turn: 0,
    role: NONE,
    on: NONE,
    layer: 2,
    levels: 0,
    hide: false,
};

/// A place for a placed thing: x, y, z (cm), first level, turn.
type Place = (i16, i16, i16, u8, u8);

/// Far islets: place, first level, size.
const ISLETS: [(Place, u8); 15] = [
    ((-1300, -150, 1300, 1, 0), 0),
    ((1450, 250, 1350, 1, 40), 1),
    ((250, -350, 1900, 2, 90), 2),
    ((-1800, 100, -300, 3, 20), 1),
    ((1700, -200, -500, 3, 60), 0),
    ((-700, 350, 2100, 3, 110), 2),
    ((1900, 150, 500, 4, 10), 2),
    ((-1600, -300, -1300, 4, 70), 0),
    ((900, 400, -1700, 4, 30), 1),
    ((-2100, 200, 900, 4, 100), 0),
    ((2000, -300, 1800, 4, 50), 1),
    ((700, -250, 2300, 4, 80), 2),
    ((-1200, 450, -2000, 4, 120), 2),
    ((-2200, -100, -800, 4, 15), 1),
    ((1300, 500, 1900, 4, 45), 0),
];

/// Trees: place (y 100 stands on the first terrace), first level.
const TREES: [Place; 15] = [
    (550, 0, -500, 0, 0),
    (-500, 0, -450, 0, 35),
    (570, 0, 280, 0, 70),
    (-150, 0, -620, 0, 105),
    (-250, 100, 420, 0, 140),
    (-60, 0, 690, 1, 175),
    (540, 0, 590, 2, 210),
    (620, 0, 60, 3, 20),
    (-600, 0, -250, 3, 55),
    (230, 0, 220, 3, 90),
    (-640, 100, 220, 3, 125),
    (330, 0, -560, 4, 160),
    (-380, 0, -640, 4, 195),
    (-40, 0, -430, 4, 230),
    (-650, 0, 50, 4, 10),
];

/// Bushes (on the floor).
const BUSHES: [Place; 16] = [
    (-350, 0, -250, 1, 0),
    (320, 0, -420, 1, 0),
    (650, 0, -150, 1, 0),
    (-620, 0, -100, 1, 0),
    (150, 0, 330, 1, 0),
    (-80, 0, -500, 2, 0),
    (420, 0, 620, 2, 0),
    (-50, 0, 150, 2, 0),
    (100, 0, -450, 3, 0),
    (-250, 0, -600, 3, 0),
    (600, 0, -300, 3, 0),
    (-520, 0, -20, 3, 0),
    (600, 0, 380, 4, 0),
    (-420, 0, -330, 4, 0),
    (250, 0, -50, 4, 0),
    (-150, 0, -310, 4, 0),
];

/// Fences (along z; turn 64 is along x).
const FENCES: [Place; 5] = [
    (680, 0, 60, 1, 0),
    (680, 0, 480, 1, 0),
    (250, 0, 680, 2, 64),
    (680, 0, -400, 3, 0),
    (300, 0, -680, 4, 64),
];

/// Grass patches.
const TUFTS: [Place; 13] = [
    (300, 0, -250, 2, 0),
    (-230, 0, -360, 2, 50),
    (480, 0, -560, 2, 100),
    (-380, 0, -560, 2, 150),
    (40, 0, -330, 2, 200),
    (550, 0, -380, 3, 30),
    (-560, 0, -400, 3, 80),
    (160, 0, -560, 3, 130),
    (610, 0, 320, 3, 180),
    (430, 0, -180, 4, 10),
    (480, 0, 80, 4, 60),
    (0, 0, 330, 4, 110),
    (180, 0, -230, 4, 160),
];

/// Gems (their middles; the last field is what they float over: they
/// are drawn after it).
const GEMS: [Place; 8] = [
    (300, 150, -60, 0, NONE),
    (-40, 45, -200, 0, NONE),
    (-410, 100, 0, 0, ON_RAMP1),
    (320, 60, 330, 0, NONE),
    (480, 240, 30, 1, NONE),
    (-200, 145, 630, 1, ON_T1),
    (580, 305, -120, 2, ON_P1 + 2),
    (-20, 255, 320, 2, ON_P1 + 3),
];

/// Most things in the table.
const MAX_THINGS: usize = 320;

struct Table {
    t: [Thing; MAX_THINGS],
    n: usize,
}

impl Table {
    #[allow(clippy::too_many_arguments)]
    const fn put(
        &mut self,
        model: u8,
        at: [i32; 3],
        turn: u8,
        role: u8,
        on: u8,
        layer: u8,
        levels: u8,
        hide: bool,
    ) {
        assert!(self.n < MAX_THINGS);
        self.t[self.n] = Thing {
            model,
            at: [at[0] as i16, at[1] as i16, at[2] as i16],
            turn,
            role,
            on,
            layer,
            levels,
            hide,
        };
        self.n += 1;
    }

    /// A thing in three details (first model number `m`): LOW, MID,
    /// HIGH and up, from level `first`.
    #[allow(clippy::too_many_arguments)]
    const fn put3(
        &mut self,
        m: u8,
        at: [i32; 3],
        turn: u8,
        role: u8,
        on: u8,
        layer: u8,
        first: u8,
        hide: bool,
    ) {
        let masks = [LOW, MID, HIGH_UP];
        let mut d = 0;
        while d < 3 {
            let lv = masks[d] & from(first);
            if lv != 0 {
                self.put(m + d as u8, at, turn, role, on, layer, lv, hide);
            }
            d += 1;
        }
    }
}

impl Table {
    /// A tree at its four details: LOW, MID, HIGH, XL and up.
    const fn tree(&mut self, p: Place, on: u8) {
        let models = [M_TREE, M_TREE + 1, M_TREE + 2, M_TREE3];
        let masks = [LOW, MID, HIGH, XL_UP];
        let mut d = 0;
        while d < 4 {
            let lv = masks[d] & from(p.3);
            if lv != 0 {
                self.put(models[d], place(p), p.4, NONE, on, 2, lv, true);
            }
            d += 1;
        }
    }

    /// A grass patch: 12 blades at HIGH, 16 from XL.
    const fn tufts(&mut self, p: Place, on: u8) {
        let hi = HIGH & from(p.3);
        if hi != 0 {
            self.put(M_TUFTS, place(p), p.4, NONE, on, 2, hi, true);
        }
        self.put(
            M_TUFTS_X,
            place(p),
            p.4,
            NONE,
            on,
            2,
            XL_UP & from(p.3),
            true,
        );
    }
}

const fn place(p: Place) -> [i32; 3] {
    [p.0 as i32, p.1 as i32, p.2 as i32]
}

const fn build_things() -> Table {
    let mut t = Table {
        t: [NO_THING; MAX_THINGS],
        n: 0,
    };
    let mut i = 0;
    while i < ISLETS.len() {
        let (p, k) = ISLETS[i];
        t.put(M_ISLET + k, place(p), p.4, NONE, NONE, 0, from(p.3), false);
        i += 1;
    }
    let o = [0, 0, 0];
    t.put3(M_UNDER, o, 0, NONE, NONE, 1, 0, false);
    let masks = [LOW, MID, HIGH];
    let mut d = 0;
    while d < 3 {
        let (m, n) = FLOORS[d];
        let mut s = 0;
        while s < n {
            t.put(m + s, o, 0, NONE, NONE, 1, masks[d], false);
            s += 1;
        }
        d += 1;
    }
    // Supports first: the parent rule needs their keys.
    t.put3(M_TERRACE, o, 0, ON_T1, NONE, 2, 0, false);
    t.put(M_RAMP1, o, 0, ON_RAMP1, NONE, 2, from(0), false);
    t.put3(M_LEVEL2, o, 0, ON_T2, ON_T1, 2, 0, false);
    t.put(M_RAMP2, o, 0, ON_RAMP2, ON_T1, 2, from(0), false);
    t.put3(M_TOWER, o, 0, ON_TOWER, NONE, 2, 0, false);
    t.put3(M_ROOF, o, 0, NONE, ON_TOWER, 2, 0, false);
    t.put(
        M_PILLARS,
        [230, 0, 450],
        0,
        ON_GATE,
        NONE,
        2,
        from(0),
        false,
    );
    t.put(
        M_PILLARS,
        [530, 0, 450],
        0,
        ON_GATE,
        NONE,
        2,
        from(0),
        false,
    );
    t.put(M_BEAM, o, 0, ON_BEAM, NONE, 2, from(0), false);
    let plat_from = [0u8, 0, 1, 2];
    let mut k = 0;
    while k < 4 {
        let (c, h) = PLATS[k];
        let m = if h == 70 { M_PLAT70 } else { M_PLAT60 };
        let at = [c[0], c[1] - PLAT_HY, c[2]];
        t.put(
            m,
            at,
            0,
            ON_P1 + k as u8,
            NONE,
            2,
            from(plat_from[k]),
            false,
        );
        k += 1;
    }
    let mut i = 0;
    while i < TREES.len() {
        let p = TREES[i];
        let on = if p.1 >= 100 { ON_T1 } else { NONE };
        t.tree(p, on);
        i += 1;
    }
    let mut i = 0;
    while i < BUSHES.len() {
        let p = BUSHES[i];
        t.put(M_BUSH, place(p), p.4, NONE, NONE, 2, from(p.3), true);
        i += 1;
    }
    let mut i = 0;
    while i < FENCES.len() {
        let p = FENCES[i];
        t.put(M_FENCE, place(p), p.4, NONE, NONE, 2, from(p.3), true);
        i += 1;
    }
    let mut i = 0;
    while i < TUFTS.len() {
        let p = TUFTS[i];
        let on = if p.1 >= 100 { ON_T1 } else { NONE };
        t.tufts(p, on);
        i += 1;
    }
    let mut i = 0;
    while i < GEMS.len() {
        let p = GEMS[i];
        // The gem model's origin is its middle: placed at the middle.
        let at = [p.0 as i32, p.1 as i32, p.2 as i32];
        t.put(M_GEM, at, 0, NONE, p.4, 2, from(p.3), true);
        i += 1;
    }
    let mut i = 0;
    while i < GEN_ISLETS.n {
        let (p, k) = GEN_ISLETS.p[i];
        t.put(M_ISLET + k, place(p), p.4, NONE, NONE, 0, from(p.3), false);
        i += 1;
    }
    // The fine floors of XL and XXL are not in the table (they took
    // 1.8 KB of the data slot): `draw` draws them after the other
    // floors, where they stood when they were the table's last things.
    t
}

// ---- XL and XXL: more far islets, placed while compiling -----------------------
//
// (A generator for more trees and grass on the island found no free
// place: the hand-placed things and the figure's path fill it. XL and
// XXL get finer trees and grass instead, and these islets.)

/// Far islets placed for XL and XXL.
struct GenIslets {
    p: [(Place, u8); 48],
    n: usize,
}

/// Two rings round the island (15 to 20 m and 23 to 28 m out), at
/// heights from -6 to +7 m, at least 7 m from every other islet; one in
/// three for XL, the rest for XXL.
const fn generate_islets() -> GenIslets {
    let mut g = GenIslets {
        p: [((0, 0, 0, 0, 0), 0); 48],
        n: 0,
    };
    let mut k = 0;
    while k < 48 {
        let h = hash(k, 4242);
        let a = ((k * 65_536 / 48) as u16).wrapping_add((h % 900) as u16);
        let rr = if k % 2 == 0 { 1500 } else { 2300 } + (h >> 10) as i32 % 500;
        let x = (csin(a) * rr) >> SHIFT14;
        let z = (ccos(a) * rr) >> SHIFT14;
        let y = ((h >> 4) % 1300) as i32 - 600;
        let mut ok = true;
        let mut i = 0;
        while i < ISLETS.len() + g.n {
            let q = if i < ISLETS.len() {
                ISLETS[i].0
            } else {
                g.p[i - ISLETS.len()].0
            };
            let (dx, dy, dz) = (
                (q.0 as i32 - x) as i64,
                (q.1 as i32 - y) as i64,
                (q.2 as i32 - z) as i64,
            );
            if dx * dx + dy * dy + dz * dz < 700 * 700 {
                ok = false;
            }
            i += 1;
        }
        if ok {
            let first = if g.n.is_multiple_of(3) { 3 } else { 4 };
            g.p[g.n] = (
                (x as i16, y as i16, z as i16, first, (h >> 24) as u8),
                (h % 3) as u8,
            );
            g.n += 1;
        }
        k += 1;
    }
    g
}

const GEN_ISLETS: GenIslets = generate_islets();

const TABLE: Table = build_things();
/// Things in the table.
pub const THINGS_N: usize = TABLE.n;

const fn first_things<const N: usize>(t: &Table) -> [Thing; N] {
    let mut out = [NO_THING; N];
    let mut i = 0;
    while i < N {
        out[i] = t.t[i];
        i += 1;
    }
    out
}

/// Every thing of the island at every level, in drawing-list order.
pub static THINGS: [Thing; THINGS_N] = first_things(&TABLE);

/// A thing's model.
pub fn model_of(th: &Thing) -> &'static Model<'static> {
    MODELS[usize::from(th.model) % MODELS.len()]
}

/// Where a thing's model origin is in the world (units).
pub fn pos_of(th: &Thing) -> V3 {
    let o = ORIGINS[usize::from(th.model) % ORIGINS.len()];
    [0, 1, 2].map(|k| u(i32::from(th.at[k])).wrapping_add(i32::from(o[k])))
}

// ---- What the figure stands on ---------------------------------------------------

/// A surface the figure can stand on: x0, z0, x1, z1, top at z0 and at
/// z1 (cm; ramps rise along z), its role, the first level it is there.
type Support = ([i32; 6], u8, u8);

const SUPPORTS: [Support; 9] = [
    ([-ISLE, -ISLE, ISLE, ISLE, 0, 0], NONE, 0),
    ([T1[0], T1[1], T1[2], T1[3], T1[4], T1[4]], ON_T1, 0),
    ([T2[0], T2[1], T2[2], T2[3], T2[4], T2[4]], ON_T2, 0),
    (
        [RAMP1[0], RAMP1[2], RAMP1[1], RAMP1[3], RAMP1[4], RAMP1[5]],
        ON_RAMP1,
        0,
    ),
    (
        [RAMP2[0], RAMP2[2], RAMP2[1], RAMP2[3], RAMP2[4], RAMP2[5]],
        ON_RAMP2,
        0,
    ),
    ([150, -50, 290, 90, 110, 110], ON_P1, 0),
    ([330, 100, 470, 240, 190, 190], ON_P1 + 1, 0),
    ([520, -180, 640, -60, 270, 270], ON_P1 + 2, 1),
    ([-80, 260, 40, 380, 220, 220], ON_P1 + 3, 2),
];

/// A ramp's tilt (round x: +z rises), for the shadow on it.
const fn slope(r: [i32; 6]) -> Mat3 {
    let (dz, dy) = ((r[3] - r[2]) as i64, (r[5] - r[4]) as i64);
    let len = isqrt((dz * dz + dy * dy) as u64) as i64;
    let s = (dy * ONE14 as i64 / len) as i32;
    let c = (dz * ONE14 as i64 / len) as i32;
    Mat3([[ONE14, 0, 0], [0, c, s], [0, -s, c]])
}

const SLOPES: [Mat3; 2] = [slope(RAMP1), slope(RAMP2)];

/// Ramps that lean on a terrace's side: the ramp, the terrace, the
/// side's z (cm; the ramp is below it, the terrace above it). Each
/// terrace is listed before its ramp in [`THINGS`].
const LEANS: [(u8, u8, i32); 2] = [(ON_RAMP1, ON_T1, RAMP1[3]), (ON_RAMP2, ON_T2, RAMP2[3])];

/// The highest surface at (x, z) (cm) that is not above `feet` (plus
/// 2 cm), at level `lv`: its height (cm) and role.
pub fn ground(x: i32, z: i32, feet: i32, lv: usize) -> (i32, u8) {
    let mut best = (i32::MIN, NONE);
    for &(s, role, first) in &SUPPORTS {
        if lv < usize::from(first) || x < s[0] || x > s[2] || z < s[1] || z > s[3] {
            continue;
        }
        let top = s[4] + (s[5] - s[4]) * (z - s[1]) / (s[3] - s[1]).max(1);
        if top <= feet + 2 && top > best.0 {
            best = (top, role);
        }
    }
    best
}

// ---- The run -------------------------------------------------------------------

/// One piece of the route: from, to (cm), ticks, a jump.
type Seg = ([i32; 3], [i32; 3], u32, bool);

/// The loop (it ends where it starts): onto platform 1, over the gap to
/// platform 2, down and through the gate, along the far side, up onto
/// the terraces, down the ramps.
pub const ROUTE: [Seg; 15] = [
    ([-350, 0, -150], [-200, 0, -200], 43, false),
    ([-200, 0, -200], [90, 0, 20], 99, false),
    ([90, 0, 20], [220, 110, 20], 36, true),
    ([220, 110, 20], [400, 190, 170], 36, true),
    ([400, 190, 170], [400, 190, 225], 15, false),
    ([400, 190, 225], [380, 0, 340], 36, true),
    ([380, 0, 340], [380, 0, 560], 60, false),
    ([380, 0, 560], [-60, 0, 560], 121, false),
    ([-60, 0, 560], [-230, 100, 560], 40, true),
    ([-230, 100, 560], [-330, 100, 560], 27, false),
    ([-330, 100, 560], [-460, 200, 560], 36, true),
    ([-460, 200, 560], [-475, 200, 450], 31, false),
    ([-475, 200, 450], [-475, 100, 250], 61, false),
    ([-475, 100, 250], [-350, 100, 150], 43, false),
    ([-350, 100, 150], [-350, 0, -150], 84, false),
];

const fn route_starts() -> [u32; 16] {
    let mut s = [0u32; 16];
    let mut i = 0;
    while i < 15 {
        s[i + 1] = s[i] + ROUTE[i].2;
        i += 1;
    }
    assert!(s[15] == LOOP, "the route must take one loop");
    s
}

const STARTS: [u32; 16] = route_starts();

/// Each piece's direction on the ground, 16384 long (x, z).
const fn headings() -> [[i32; 2]; 15] {
    let mut h = [[0i32; 2]; 15];
    let mut i = 0;
    while i < 15 {
        let (a, b, _, _) = ROUTE[i];
        let (dx, dz) = ((b[0] - a[0]) as i64, (b[2] - a[2]) as i64);
        let len = isqrt((dx * dx + dz * dz) as u64) as i64;
        h[i] = [
            (dx * ONE14 as i64 / len) as i32,
            (dz * ONE14 as i64 / len) as i32,
        ];
        i += 1;
    }
    h
}

const HEADINGS: [[i32; 2]; 15] = headings();

/// Where the figure is at tick `t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Runner {
    /// Its feet (units).
    pub feet: V3,
    /// The height the camera follows (units): through a jump it goes
    /// straight from the take-off height to the landing height.
    pub follow_y: i32,
    /// The route piece.
    pub seg: usize,
    /// How far through a jump (0 to 256), or `None` running.
    pub jump: Option<i32>,
}

/// The figure at tick `t` (any tick: the route repeats).
pub fn runner(t: u32) -> Runner {
    let t = t % LOOP;
    let mut i = 0;
    while i + 1 < ROUTE.len() && STARTS[i + 1] <= t {
        i += 1;
    }
    let (a, b, n, jump) = ROUTE[i];
    let k = (t - STARTS[i]) as i32;
    let n = n as i32;
    let (a, b) = (a.map(u), b.map(u));
    let lerp = |q: usize| a[q] + (b[q] - a[q]) * k / n;
    let line_y = lerp(1);
    let y = if jump {
        // A bump of 60 cm plus half the height change above the line.
        let h = u(60) + (b[1] - a[1]).abs() / 2;
        line_y + 4 * h * k * (n - k) / (n * n)
    } else {
        line_y
    };
    Runner {
        feet: [lerp(0), y, lerp(2)],
        follow_y: line_y,
        seg: i,
        jump: if jump { Some(k * 256 / n) } else { None },
    }
}

/// The direction of the route round tick `t`: the route's directions
/// averaged over the ticks `t - w` to `t + w` (16384 long, x and z).
pub fn heading(t: u32, w: u32) -> [i32; 2] {
    let t = (t % LOOP) as i32;
    let (lo, hi) = (t - w as i32, t + w as i32);
    let mut s = [0i32; 2];
    for rep in [-(LOOP as i32), 0, LOOP as i32] {
        for i in 0..ROUTE.len() {
            let a = STARTS[i] as i32 + rep;
            let b = STARTS[i + 1] as i32 + rep;
            let over = b.min(hi) - a.max(lo);
            if over > 0 {
                s[0] += over * HEADINGS[i][0];
                s[1] += over * HEADINGS[i][1];
            }
        }
    }
    let len = isqrt((i64::from(s[0]).pow(2) + i64::from(s[1]).pow(2)) as u64) as i64;
    if len == 0 {
        return [0, ONE14];
    }
    s.map(|v| (i64::from(v) * i64::from(ONE14) / len) as i32)
}

/// The camera looks down this much (22 degrees).
const PITCH: u16 = (22 * 65_536 / 360) as u16;
/// It is this far from the point it looks at (cm), which is this high
/// above the figure's feet (cm).
const CAM_DIST: i32 = 440;
const CAM_UP: i32 = 90;
/// The camera's turn follows the route's direction over 30 ticks each
/// way (it starts to turn before a corner); the figure's over 5.
const CAM_W: u32 = 30;
const HERO_W: u32 = 5;

/// The camera: the eye (world, units) and world to camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Camera {
    pub eye: V3,
    pub rot: Mat3,
}

/// The follow camera at tick `t`.
pub fn camera(t: u32) -> Camera {
    let h = heading(t, CAM_W);
    let yaw = Mat3([[h[1], 0, h[0]], [0, ONE14, 0], [-h[0], 0, h[1]]]);
    let (sp, cp) = (csin(PITCH), ccos(PITCH));
    let pitch = Mat3([[ONE14, 0, 0], [0, cp, -sp], [0, sp, cp]]);
    let r = yaw.mul(&pitch);
    let fwd = r.col(2);
    let run = runner(t);
    let target = [run.feet[0], run.follow_y + u(CAM_UP), run.feet[2]];
    let d = u(CAM_DIST);
    let eye = [0, 1, 2].map(|k| target[k] - ((fwd[k] * d) >> SHIFT14));
    Camera {
        eye,
        rot: r.transposed(),
    }
}

/// The horizon's row (the camera never rolls and always looks down by
/// [`PITCH`]).
pub const HORIZON: i32 = VIEW.mid.1 / SUB - csin(PITCH) * VIEW.focal / ccos(PITCH);

// ---- The figure -------------------------------------------------------------------

/// The figure's origin above its feet (cm).
const HERO_MID: i32 = 64;
/// Limbs: 0 the body, 1 and 2 the legs (left, right), 3 and 4 the arms.
/// Their pivots (cm above the feet).
const PIVOTS: [[i32; 3]; 5] = [
    [0, 0, 0],
    [-10, 48, 0],
    [10, 48, 0],
    [-25, 86, 0],
    [25, 86, 0],
];

/// The figure's model at rest, with each corner's limb.
pub struct HeroBuilt<const V: usize, const F: usize> {
    pub m: Built<V, F>,
    pub group: [u8; V],
}

const fn trim_hero<const V: usize, const F: usize>(b: &B) -> HeroBuilt<V, F> {
    let mut group = [0u8; V];
    let mut i = 0;
    while i < V {
        group[i] = b.g[i];
        i += 1;
    }
    HeroBuilt { m: trim(b), group }
}

/// A figure model (corners at rest, faces, face directions, limbs).
pub struct Hero {
    pub verts: &'static [[i16; 3]],
    pub faces: &'static [Face],
    pub normals: &'static [[i16; 3]],
    pub group: &'static [u8],
}

macro_rules! hero {
    ($name:ident, $b:expr) => {
        static $name: Hero = {
            const BB: B = $b;
            const TT: HeroBuilt<{ BB.nv }, { BB.nf }> = trim_hero(&BB);
            static T: HeroBuilt<{ BB.nv }, { BB.nf }> = TT;
            Hero {
                verts: &T.m.verts,
                faces: &T.m.faces,
                normals: &T.m.normals,
                group: &T.group,
            }
        };
    };
}

/// The figure's parts. `legs`: the legs (with shoes when `split`);
/// `upper`: body, arms, head, hair, the pack (`pack`), hands (`split`).
const fn figure(legs: bool, upper: bool, pack: bool, split: bool) -> B {
    let mut b = B::at([0, HERO_MID, 0]);
    if legs {
        b.grp = 1;
        if split {
            b.cube([-17, 12, -7], [-3, 48, 7], None, PANTS, None);
            b.cube([-18, 0, -9], [-2, 12, 11], Some(HAIR), HAIR, Some(HAIR));
        } else {
            b.cube([-17, 0, -7], [-3, 48, 7], None, PANTS, Some(PANTS));
        }
        b.grp = 2;
        if split {
            b.cube([3, 12, -7], [17, 48, 7], None, PANTS, None);
            b.cube([2, 0, -9], [18, 12, 11], Some(HAIR), HAIR, Some(HAIR));
        } else {
            b.cube([3, 0, -7], [17, 48, 7], None, PANTS, Some(PANTS));
        }
    }
    if upper {
        b.grp = 0;
        b.cube([-20, 46, -11], [20, 92, 11], Some(BODY), BODY, None);
        b.cube([-15, 92, -14], [15, 122, 14], Some(SKIN), SKIN, None);
        b.cube([-16, 106, -16], [16, 126, 6], Some(HAIR), HAIR, None);
        if pack {
            b.cube([-14, 54, -21], [14, 86, -11], Some(PACK), PACK, Some(PACK));
        }
        b.grp = 3;
        b.cube([-30, 48, -6], [-20, 90, 6], Some(BODY), BODY, Some(SKIN));
        if split {
            b.cube([-31, 40, -5], [-19, 50, 5], Some(SKIN), SKIN, Some(SKIN));
        }
        b.grp = 4;
        b.cube([20, 48, -6], [30, 90, 6], Some(BODY), BODY, Some(SKIN));
        if split {
            b.cube([19, 40, -5], [31, 50, 5], Some(SKIN), SKIN, Some(SKIN));
        }
    }
    b
}

hero!(HERO_LOW, figure(true, true, false, false));
hero!(HERO_MID_M, figure(true, true, true, false));
hero!(HERO_LEGS, figure(true, false, false, true));
hero!(HERO_UPPER, figure(false, true, true, true));

/// Most faces of a figure model.
pub const HERO_FACES: usize = 96;

/// The figure's posed corners and face directions (run-time work space,
/// about 1 KB, kept with the renderer).
pub struct Scratch {
    v: [[i16; 3]; MAX_VERTS],
    n: [[i16; 3]; HERO_FACES],
    /// The faces of a fine floor strip (XL, XXL), made at run time.
    f: [Face; FINE_FACES],
}

impl Scratch {
    pub const fn new() -> Scratch {
        Scratch {
            v: [[0; 3]; MAX_VERTS],
            n: [[0; 3]; HERO_FACES],
            f: [Face { v: [0; 3], mat: 0 }; FINE_FACES],
        }
    }
}

impl Default for Scratch {
    fn default() -> Self {
        Scratch::new()
    }
}

/// Degrees to an angle.
const fn deg(a: i32) -> i32 {
    a * 65_536 / 360
}

/// The limbs' turns at tick `t` (legs left, right, arms left, right):
/// a run of one stride in 24 ticks, or the jump pose (eased in and out
/// over the first and last 6 ticks of a jump).
pub fn pose(t: u32) -> [i32; 4] {
    let run = runner(t);
    let ph = ((t % 24) * 65_536 / 24) as u16;
    let s = sin(ph);
    let swing = [deg(35), -deg(35), -deg(30), deg(30)];
    let running = swing.map(|a| (a * s) >> SHIFT14);
    let Some(k) = run.jump else {
        return running;
    };
    let jumping = [deg(45), -deg(25), deg(155), deg(130)];
    // k is 0 to 256 over the jump: ease over the first and last 43.
    let w = (k.min(256 - k) * 6).min(256);
    [0, 1, 2, 3].map(|i| running[i] + (jumping[i] - running[i]) * w / 256)
}

/// Poses a figure model into `sc`: each limb's corners turned round its
/// pivot (forwards, round x), face directions likewise. Returns the
/// corners and the radius (units).
fn pose_into(h: &Hero, angles: &[i32; 4], sc: &mut Scratch) -> (usize, i32) {
    let o = u(HERO_MID);
    let mut rots = [Mat3::IDENTITY; 5];
    let mut piv = [[0i32; 3]; 5];
    for g in 1..5 {
        let a = angles[g - 1] as u16;
        let (s, c) = (sin(a), cos(a));
        rots[g] = Mat3([[ONE14, 0, 0], [0, c, s], [0, -s, c]]);
        let p = PIVOTS[g];
        piv[g] = [u(p[0]), u(p[1]) - o, u(p[2])];
    }
    let nv = h.verts.len().min(MAX_VERTS);
    let mut far = 0u64;
    for i in 0..nv {
        let g = usize::from(h.group[i]) % 5;
        let v = h.verts[i].map(i32::from);
        let p = if g == 0 {
            v
        } else {
            let d = [0, 1, 2].map(|k| v[k] - piv[g][k]);
            let r = rots[g].apply(d);
            [0, 1, 2].map(|k| r[k] + piv[g][k])
        };
        sc.v[i] = p.map(|x| x as i16);
        let d2 = p
            .iter()
            .map(|&x| (i64::from(x) * i64::from(x)) as u64)
            .sum::<u64>();
        far = far.max(d2);
    }
    let nf = h.faces.len().min(HERO_FACES);
    for i in 0..nf {
        let g = usize::from(h.group[usize::from(h.faces[i].v[0])]) % 5;
        let n = h.normals[i].map(i32::from);
        let r = if g == 0 { n } else { rots[g].apply(n) };
        sc.n[i] = r.map(|x| x as i16);
    }
    (nv, isqrt(far) as i32 + 1)
}

// ---- One picture ---------------------------------------------------------------

/// One object (one copy of the renderer's object stage: smaller code).
#[inline(never)]
fn draw_one(r: &mut Renderer, f: &mut Frame, cam: &Mat3, o: &Object<'_>) {
    r.draw_with(f, &VIEW, cam, &IslandPaint, o);
}

/// A thing's turn at tick `t` (gems spin four times a loop).
pub fn rot_of(i: usize, th: &Thing, t: u32) -> Mat3 {
    if th.model == M_GEM {
        let spin = ((t % LOOP) * 1024 / 3) as u16;
        Mat3::rotation(spin.wrapping_add((i as u16).wrapping_mul(5000)), 0, 0)
    } else if th.turn != 0 {
        Mat3::rotation(u16::from(th.turn) << 8, 0, 0)
    } else {
        Mat3::IDENTITY
    }
}

/// Is a thing to be drawn, and where is its middle in camera space?
fn seen(cam: &Camera, th: &Thing) -> Option<V3> {
    let m = model_of(th);
    let p = pos_of(th);
    let rel = [0, 1, 2].map(|k| p[k].wrapping_sub(cam.eye[k]));
    let c = in_view(&VIEW, &cam.rot, rel, m.radius, FAR)?;
    if th.hide && c[2].wrapping_sub(m.radius) < NEAR {
        return None;
    }
    Some(c)
}

fn draw_thing(r: &mut Renderer, f: &mut Frame, cam: &Camera, i: usize, c: V3, t: u32) {
    let th = &THINGS[i];
    #[cfg(feature = "check")]
    {
        r.owner = i as u16;
    }
    let o = Object {
        model: model_of(th),
        rot: rot_of(i, th, t),
        centre: c,
    };
    draw_one(r, f, &cam.rot, &o);
}

/// Fine floor strip `idx` (bit 0: the half, the rest: the strip) of
/// level `lv` (XL or XXL), when its ball is in view: its corners and
/// faces are made into the scratch (corners at their world places, so
/// every strip shares the world origin and shared edges land on the
/// same pixels, as the other floors do), tiles under the first terrace
/// left out, the checker as on the other floors.
fn draw_fine(
    r: &mut Renderer,
    f: &mut Frame,
    cam: &Camera,
    id: u16,
    idx: u8,
    lv: usize,
    sc: &mut Scratch,
) {
    let (tiles, rows, radius) = FINE[lv.clamp(3, 4) - 3];
    let half = i32::from(idx & 1);
    let cols = tiles / 2;
    let step = 2 * ISLE / tiles;
    let x0 = -ISLE + half * ISLE;
    let r0 = i32::from(idx >> 1) * rows;
    let r1 = (r0 + rows).min(tiles);
    let z0 = -ISLE + r0 * step;
    let rel = |p: V3| [0, 1, 2].map(|k| p[k].wrapping_sub(cam.eye[k]));
    let mid = [u(x0 + ISLE / 2), 0, u(z0 + (r1 - r0) * step / 2)];
    if in_view(&VIEW, &cam.rot, rel(mid), u(radius), FAR).is_none() {
        return;
    }
    let mut grid = [u8::MAX; MAX_VERTS];
    let (mut nv, mut nf) = (0usize, 0usize);
    for k in 0..r1 - r0 {
        for c in 0..cols {
            let (xa, za) = (x0 + c * step, z0 + k * step);
            if xa >= T1[0] && xa + step <= T1[2] && za >= T1[1] && za + step <= T1[3] {
                continue;
            }
            let mut q = [0u8; 4];
            for (n, (dc, dk)) in [(0, 0), (1, 0), (1, 1), (0, 1)].into_iter().enumerate() {
                let g = ((k + dk) * (cols + 1) + c + dc) as usize;
                if grid[g] == u8::MAX {
                    let (px, pz) = (xa + dc * step, za + dk * step);
                    sc.v[nv] = [u(px) as i16, 0, u(pz) as i16];
                    grid[g] = nv as u8;
                    nv += 1;
                }
                q[n] = grid[g];
            }
            let mat = if (c + half * cols + k + r0) % 2 == 0 {
                GRASS
            } else {
                GRASS2
            };
            // The builder's turn for faces that look up.
            sc.f[nf] = Face {
                v: [q[0], q[2], q[1]],
                mat,
            };
            sc.f[nf + 1] = Face {
                v: [q[0], q[3], q[2]],
                mat,
            };
            nf += 2;
        }
    }
    if nf == 0 {
        return;
    }
    #[cfg(feature = "check")]
    {
        r.owner = id;
    }
    #[cfg(not(feature = "check"))]
    let _ = id;
    let m = Model {
        verts: &sc.v[..nv],
        faces: &sc.f[..nf],
        normals: &UPS,
        radius: FINE_M.radius,
    };
    let o = Object {
        model: &m,
        rot: Mat3::IDENTITY,
        centre: cam.rot.apply(rel([0, 0, 0])),
    };
    draw_one(r, f, &cam.rot, &o);
}

/// Sorts `list[..n]` far to near by `keys` (insertion sort).
fn sort(keys: &mut [i16; MAX_OBJ], list: &mut [u16; MAX_OBJ], n: usize) {
    for a in 1..n {
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
}

/// List numbers of the shadow and the figure's two models.
const ID_SHADOW: u16 = 0xFFF0;
/// Fine floor strip `s` (the checker's owner number).
const ID_FINE: u16 = 0xFF00;
const ID_LEGS: u16 = 0xFFF1;
const ID_UPPER: u16 = 0xFFF2;

/// The island at tick `tick`, level `load` (1 to 5).
pub fn draw(
    r: &mut Renderer,
    keys: &mut [i16; MAX_OBJ],
    list: &mut [u16; MAX_OBJ],
    sc: &mut Scratch,
    f: &mut Frame,
    load: u32,
    tick: u32,
) -> Counts {
    let lv = (load.clamp(1, LEVELS as u32) - 1) as usize;
    let d = lv.min(2);
    let bit = 1u8 << lv;
    let t = tick % LOOP;
    let cam = camera(t);

    // The sky: two bands, haze below the horizon.
    let (y0, y1) = (WORK.y0, WORK.y1);
    let hz = HORIZON.clamp(y0, y1);
    let b1 = (hz - 5).clamp(y0, hz);
    rect_fill(f, &Rect::new(0, y0, 160, b1 - y0), SKY_HI);
    rect_fill(f, &Rect::new(0, b1, 160, hz - b1), SKY_LOW);
    rect_fill(f, &Rect::new(0, hz, 160, y1 - hz), HAZE);
    r.begin();

    // The far islets, far to near.
    let mut n = 0;
    for (i, th) in THINGS.iter().enumerate() {
        if th.layer == 0
            && th.levels & bit != 0
            && let Some(c) = seen(&cam, th)
        {
            keys[n] = c[2] as i16;
            list[n] = i as u16;
            n += 1;
        }
    }
    sort(keys, list, n);
    for &id in &list[..n] {
        let i = usize::from(id);
        let th = &THINGS[i];
        if let Some(c) = seen(&cam, th) {
            draw_thing(r, f, &cam, i, c, t);
        }
    }

    // The underside and the floor, in list order.
    for (i, th) in THINGS.iter().enumerate() {
        if th.layer != 1 || th.levels & bit == 0 {
            continue;
        }
        if let Some(c) = seen(&cam, th) {
            draw_thing(r, f, &cam, i, c, t);
        }
    }
    // XL and XXL: the fine floor strips, last of the floors.
    if lv >= 3 {
        for s in 0..fine_strips(lv - 3) {
            draw_fine(r, f, &cam, ID_FINE + s as u16, s as u8, lv, sc);
        }
    }

    // The figure and its shadow.
    let run = runner(t);
    let feet_cm = [0, 1, 2].map(|k| run.feet[k] * 100 / M8);
    let (gy, under) = ground(feet_cm[0], feet_cm[2], feet_cm[1], lv);
    let high = feet_cm[1] - gy > 60;
    let shadow = DISCS[usize::from(high)][d];
    let shadow_pos = [run.feet[0], u(gy) + u(1), run.feet[2]];
    let rel = |p: V3| [0, 1, 2].map(|k| p[k].wrapping_sub(cam.eye[k]));
    let shadow_c = in_view(&VIEW, &cam.rot, rel(shadow_pos), shadow.radius, FAR);
    // On a ramp the shadow lies in the ramp's slope.
    let shadow_rot = match under {
        ON_RAMP1 => SLOPES[0],
        ON_RAMP2 => SLOPES[1],
        _ => Mat3::IDENTITY,
    };
    if under == NONE
        && let Some(c) = shadow_c
    {
        #[cfg(feature = "check")]
        {
            r.owner = ID_SHADOW;
        }
        let o = Object {
            model: shadow,
            rot: shadow_rot,
            centre: c,
        };
        draw_one(r, f, &cam.rot, &o);
    }

    // Everything else, far to near, each after what it stands on.
    let mut role_key = [i16::MAX; ROLES];
    let (eye_y, eye_z) = (cam.eye[1], cam.eye[2]);
    let mut n = 0;
    let mut add = |n: &mut usize, key: i32, on: u8, role: u8, id: u16, role_key: &mut [i16]| {
        let mut k = key as i16;
        let parent = role_key[usize::from(on) % ROLES];
        if on != NONE && parent != i16::MAX {
            k = k.min(parent.saturating_sub(2));
        }
        // A ramp leaning on a terrace's side: the plane of that side
        // parts them, so the one on the eye's side is drawn last.
        for &(ramp, terrace, z1) in &LEANS {
            let tk = role_key[usize::from(terrace)];
            if role == ramp && tk != i16::MAX {
                k = if eye_z > u(z1) {
                    k.max(tk.saturating_add(1))
                } else {
                    k.min(tk.saturating_sub(1))
                };
            }
        }
        // The gate: the plane of the pillars' tops parts them from the
        // beam. From below it the beam is drawn before both pillars,
        // from above after both.
        let (far, near) = (
            role_key[usize::from(ON_GATE)],
            role_key[usize::from(GATE_NEAR)],
        );
        if role == ON_BEAM && far != i16::MAX {
            k = if eye_y < u(GATE_Y) {
                k.max(far.saturating_add(1))
            } else {
                k.min(near.saturating_sub(1))
            };
        }
        if role == ON_GATE {
            role_key[usize::from(ON_GATE)] = if far == i16::MAX { k } else { far.max(k) };
            role_key[usize::from(GATE_NEAR)] = if near == i16::MAX { k } else { near.min(k) };
        } else if role != NONE {
            role_key[usize::from(role) % ROLES] = k;
        }
        if *n < MAX_OBJ {
            keys[*n] = k;
            list[*n] = id;
            *n += 1;
        }
    };
    for (i, th) in THINGS.iter().enumerate() {
        if th.layer == 2
            && th.levels & bit != 0
            && let Some(c) = seen(&cam, th)
        {
            add(&mut n, c[2], th.on, th.role, i as u16, &mut role_key);
        }
    }
    if under != NONE
        && let Some(c) = shadow_c
    {
        add(&mut n, c[2], under, ON_SHADOW, ID_SHADOW, &mut role_key);
    }
    let hero_pos = [run.feet[0], run.feet[1] + u(HERO_MID), run.feet[2]];
    let hero_c = in_view(&VIEW, &cam.rot, rel(hero_pos), u(90), FAR);
    if let Some(c) = hero_c {
        if lv >= 2 {
            add(&mut n, c[2], ON_SHADOW, ON_LEGS, ID_LEGS, &mut role_key);
            add(&mut n, c[2], ON_LEGS, NONE, ID_UPPER, &mut role_key);
        } else {
            add(&mut n, c[2], ON_SHADOW, NONE, ID_UPPER, &mut role_key);
        }
    }
    sort(keys, list, n);

    let h = heading(t, HERO_W);
    let hero_rot = Mat3([[h[1], 0, h[0]], [0, ONE14, 0], [-h[0], 0, h[1]]]);
    let angles = pose(t);
    for &id in &list[..n] {
        match id {
            ID_SHADOW => {
                if let Some(c) = shadow_c {
                    #[cfg(feature = "check")]
                    {
                        r.owner = ID_SHADOW;
                    }
                    let o = Object {
                        model: shadow,
                        rot: shadow_rot,
                        centre: c,
                    };
                    draw_one(r, f, &cam.rot, &o);
                }
            }
            ID_LEGS | ID_UPPER => {
                let Some(c) = hero_c else { continue };
                let hm = match (id, lv) {
                    (ID_LEGS, _) => &HERO_LEGS,
                    (_, 0) => &HERO_LOW,
                    (_, 1) => &HERO_MID_M,
                    _ => &HERO_UPPER,
                };
                let (nv, radius) = pose_into(hm, &angles, sc);
                let nf = hm.faces.len().min(HERO_FACES);
                let m = Model {
                    verts: &sc.v[..nv],
                    faces: &hm.faces[..nf],
                    normals: &sc.n[..nf],
                    radius,
                };
                #[cfg(feature = "check")]
                {
                    r.owner = id;
                }
                let o = Object {
                    model: &m,
                    rot: hero_rot,
                    centre: c,
                };
                draw_one(r, f, &cam.rot, &o);
            }
            _ => {
                let i = usize::from(id);
                if let Some(c) = seen(&cam, &THINGS[i]) {
                    draw_thing(r, f, &cam, i, c, t);
                }
            }
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
