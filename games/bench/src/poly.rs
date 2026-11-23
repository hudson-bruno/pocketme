//! Simple 3D with flat triangles, in whole numbers. The triangle filler
//! ([`fill_tri`]) and the model tables are MERCATOR's (`poly.rs`, itself
//! from KESTREL), unchanged. The renderer is MERCATOR's, made cheaper
//! where the bench can promise small numbers:
//!
//! * every sum is 32 bits ([`crate::fixed`]): the scenes stay within
//!   32 m of the eye, so no 64-bit product is needed except where a
//!   clipped edge is cut ([`clip_polygon`]);
//! * the projection divides 32-bit numbers (one instruction on the chip;
//!   MERCATOR divides 64-bit numbers, a long ROM routine; CAMBER has the
//!   same 32-bit shortcut for small numbers);
//! * back faces and light are tested in the model's own space: the eye
//!   and the light are turned into the model once per object, so a face
//!   costs two short sums and no turning of its normal;
//! * no fog, no glow, no scaling, no material swap.
//!
//! The way a model reaches the picture: corners turned and moved into
//! the camera's space, flagged against five planes (near, and the four
//! sides at twice the view's size: the "guard band"), projected (x and y
//! divided by z, to 1/16 pixel); back faces dropped; faces that cross a
//! plane cut in 3D; the object's triangles sorted far to near and filled.
//! There is no depth buffer: objects are drawn far to near by the caller.
//!
//! Units: positions in 1/256 m ([`M8`]), screen positions in 1/16 pixel
//! ([`SUB`]), matrices and normals in 1/16384.

use game_kit::{Frame, H, W};

use crate::fixed::{Mat3, ONE14, SHIFT14, V3, dot, isqrt};

/// One metre in model and camera units.
pub const M8: i32 = 256;
/// Steps per pixel in screen positions (1/16 pixel).
pub const SUB: i32 = 16;
/// The filler takes screen positions from -GUARD to +GUARD (1/16 pixel).
/// Beyond that it draws nothing, so its sums never run over.
pub const GUARD: i32 = 8191;
/// The near plane: nothing closer to the eye than this (1/256 m).
pub const NEAR: i32 = M8 / 2;
/// Most corners in one model.
pub const MAX_VERTS: usize = 64;
/// Most triangles one model can put on screen at once.
pub const MAX_TRIS: usize = 128;
/// Materials and shades of the scene colours: palette number =
/// `first + shade * MATS + material`.
pub const MATS: u8 = 4;
pub const SHADES: u8 = 8;

// ---- Rectangles and the triangle filler ---------------------------------------

/// A rectangle of pixels: x0..x1, y0..y1 (the ends not included).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect {
            x0: x,
            y0: y,
            x1: x + w,
            y1: y + h,
        }
    }

    pub const fn w(&self) -> i32 {
        self.x1 - self.x0
    }

    pub const fn h(&self) -> i32 {
        self.y1 - self.y0
    }

    /// The part of this rectangle that is on the picture.
    pub fn on_screen(&self) -> Rect {
        Rect {
            x0: self.x0.clamp(0, W as i32),
            y0: self.y0.clamp(0, H as i32),
            x1: self.x1.clamp(0, W as i32),
            y1: self.y1.clamp(0, H as i32),
        }
    }
}

/// The first pixel row whose middle (row * 16 + 8) is at or below `y`.
pub fn first_row(y: i32) -> i32 {
    // y is within GUARD (2^13): no sum can run over.
    let y = y.wrapping_sub(SUB / 2);
    y.div_euclid(SUB)
        .wrapping_add(i32::from(y.rem_euclid(SUB) != 0))
}

/// Follows one edge of a triangle down the pixel rows, exactly (see
/// MERCATOR's `poly.rs` for the sums).
struct Edge {
    q: i32,
    r: i32,
    d: i32,
    sq: i32,
    sr: i32,
}

impl Edge {
    /// Corners within [`GUARD`] and rows on the picture: every product
    /// here is below 2^28.
    fn new(a: [i32; 2], b: [i32; 2], row: i32) -> Edge {
        let dx = b[0].wrapping_sub(a[0]);
        let dy = b[1].wrapping_sub(a[1]);
        let d = SUB.wrapping_mul(dy);
        let n = a[0]
            .wrapping_mul(dy)
            .wrapping_add(
                SUB.wrapping_mul(row)
                    .wrapping_add(SUB / 2)
                    .wrapping_sub(a[1])
                    .wrapping_mul(dx),
            )
            .wrapping_sub((SUB / 2).wrapping_mul(dy));
        let s = SUB.wrapping_mul(dx);
        let (q, r) = floor_div(n, d);
        let (sq, sr) = floor_div(s, d);
        Edge { q, r, d, sq, sr }
    }

    /// The first pixel at or right of the edge on this row.
    fn x(&self) -> i32 {
        self.q.wrapping_add(i32::from(self.r != 0))
    }

    fn step(&mut self) {
        self.q = self.q.wrapping_add(self.sq);
        self.r = self.r.wrapping_add(self.sr);
        if self.r >= self.d {
            self.r = self.r.wrapping_sub(self.d);
            self.q = self.q.wrapping_add(1);
        }
    }
}

/// `n` divided by `d` (above 0), rounded down, and the remainder.
#[inline(never)]
fn floor_div(n: i32, d: i32) -> (i32, i32) {
    let d = d.max(1);
    let (q, r) = (n / d, n % d);
    if r < 0 {
        (q.wrapping_sub(1), r.wrapping_add(d))
    } else {
        (q, r)
    }
}

/// Fills a triangle in one colour, inside `clip` (and the picture).
/// Corners in 1/16 pixel; any order. Returns the pixels painted.
///
/// A pixel is painted when its middle is inside the triangle; a middle
/// exactly on an edge only for a left or flat top edge (the "top-left
/// rule"). Corners beyond [`GUARD`] paint nothing.
pub fn fill_tri(f: &mut Frame, clip: &Rect, p: &[[i32; 2]; 3], c: u8) -> u32 {
    if p.iter()
        .any(|v| v[0].unsigned_abs() > GUARD as u32 || v[1].unsigned_abs() > GUARD as u32)
    {
        return 0;
    }
    let clip = clip.on_screen();
    let (mut v0, mut v1, mut v2) = (p[0], p[1], p[2]);
    if v0[1] > v1[1] {
        core::mem::swap(&mut v0, &mut v1);
    }
    if v1[1] > v2[1] {
        core::mem::swap(&mut v1, &mut v2);
    }
    if v0[1] > v1[1] {
        core::mem::swap(&mut v0, &mut v1);
    }
    let d = |a: [i32; 2], b: [i32; 2], k: usize| b[k].wrapping_sub(a[k]);
    let cross = d(v0, v2, 0)
        .wrapping_mul(d(v0, v1, 1))
        .wrapping_sub(d(v0, v2, 1).wrapping_mul(d(v0, v1, 0)));
    if cross == 0 {
        return 0;
    }
    let mid_right = cross < 0;
    let r0 = first_row(v0[1]).max(clip.y0);
    let r1 = first_row(v1[1]).clamp(r0, clip.y1.max(r0));
    let r2 = first_row(v2[1]).min(clip.y1);
    if r0 >= r2 {
        return 0;
    }
    let mut long = Edge::new(v0, v2, r0);
    let mut painted = 0u32;
    let mut span = |row: i32, long: &Edge, short: &Edge| {
        let (l, r) = if mid_right {
            (long.x(), short.x())
        } else {
            (short.x(), long.x())
        };
        let (l, r) = (l.max(clip.x0), r.min(clip.x1));
        if l < r {
            let at = (row as usize).wrapping_mul(W);
            f.0[at.wrapping_add(l as usize)..at.wrapping_add(r as usize)].fill(c);
            painted = painted.wrapping_add(r.wrapping_sub(l) as u32);
        }
    };
    if r0 < r1 {
        let mut short = Edge::new(v0, v1, r0);
        for row in r0..r1 {
            span(row, &long, &short);
            long.step();
            short.step();
        }
    }
    let start = r1.max(r0);
    if start < r2 {
        let mut short = Edge::new(v1, v2, start);
        for row in start..r2 {
            span(row, &long, &short);
            long.step();
            short.step();
        }
    }
    painted
}

/// The slow, obvious filler the tests compare [`fill_tri`] with: every
/// pixel of `clip` tested on its own. Not used by the game.
pub fn fill_tri_slow(f: &mut Frame, clip: &Rect, p: &[[i32; 2]; 3], c: u8) -> u32 {
    let clip = clip.on_screen();
    let v = p.map(|q| [i64::from(q[0]), i64::from(q[1])]);
    let area =
        (v[1][0] - v[0][0]) * (v[2][1] - v[0][1]) - (v[1][1] - v[0][1]) * (v[2][0] - v[0][0]);
    if area == 0 {
        return 0;
    }
    let sign = area.signum();
    let mut painted = 0;
    for y in clip.y0..clip.y1 {
        for x in clip.x0..clip.x1 {
            let (px, py) = (i64::from(x * SUB + SUB / 2), i64::from(y * SUB + SUB / 2));
            let inside = (0..3).all(|k| {
                let a = v[k];
                let b = v[(k + 1) % 3];
                let e = sign * ((b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0]));
                let (ex, ey) = (-(sign * (b[1] - a[1])), sign * (b[0] - a[0]));
                e > 0 || (e == 0 && (ex > 0 || (ex == 0 && ey > 0)))
            });
            if inside {
                f.0[y as usize * W + x as usize] = c;
                painted += 1;
            }
        }
    }
    painted
}

/// Whether the box round a triangle (corners in 1/16 pixel) holds no
/// pixel middle of `area`: then [`fill_tri`] would paint nothing, so a
/// renderer that knows it can skip the call. Never true for a triangle
/// that would paint (a painted pixel's middle is inside the triangle,
/// so inside its box). Corners within [`GUARD`]: no sum runs over.
pub fn no_pixel_middle(p: &[[i32; 2]; 3], area: &Rect) -> bool {
    let lo = |k: usize| p[0][k].min(p[1][k]).min(p[2][k]);
    let hi = |k: usize| p[0][k].max(p[1][k]).max(p[2][k]);
    // The first middle at or after the low side, the last at or before
    // the high side (`first_row` works the same for columns).
    let x0 = first_row(lo(0)).max(area.x0);
    let x1 = (first_row(hi(0).wrapping_add(1)) - 1).min(area.x1 - 1);
    let y0 = first_row(lo(1)).max(area.y0);
    let y1 = (first_row(hi(1).wrapping_add(1)) - 1).min(area.y1 - 1);
    x0 > x1 || y0 > y1
}

/// Fills a rectangle (the part on the picture) in one colour.
pub fn rect_fill(f: &mut Frame, r: &Rect, c: u8) {
    let r = r.on_screen();
    for y in r.y0..r.y1 {
        let at = y as usize * W;
        f.0[at + r.x0 as usize..at + r.x1 as usize].fill(c);
    }
}

// ---- Models -------------------------------------------------------------------

/// A face as written in a model table: three corners, a material, and
/// the convex part it belongs to (for turning its front outwards).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawFace {
    pub v: [u8; 3],
    pub mat: u8,
    pub part: u8,
}

/// A face ready to draw: corners in front-face order, and a material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    pub v: [u8; 3],
    pub mat: u8,
}

/// A model: corners in 1/256 m, faces, each face's direction (unit
/// length in 1/16384, made while compiling), and the radius of a ball
/// round the origin that holds every corner.
#[derive(Clone, Copy, Debug)]
pub struct Model<'a> {
    pub verts: &'a [[i16; 3]],
    pub faces: &'a [Face],
    pub normals: &'a [[i16; 3]],
    pub radius: i32,
}

/// The face's direction before any turning: `(b - a) x (c - a)`.
pub const fn cross_of(v: &[[i16; 3]], f: [u8; 3]) -> [i64; 3] {
    let a = v[f[0] as usize];
    let b = v[f[1] as usize];
    let c = v[f[2] as usize];
    let u = [
        (b[0] - a[0]) as i64,
        (b[1] - a[1]) as i64,
        (b[2] - a[2]) as i64,
    ];
    let w = [
        (c[0] - a[0]) as i64,
        (c[1] - a[1]) as i64,
        (c[2] - a[2]) as i64,
    ];
    [
        u[1] * w[2] - u[2] * w[1],
        u[2] * w[0] - u[0] * w[2],
        u[0] * w[1] - u[1] * w[0],
    ]
}

/// The middle of a part: the average of its faces' corners.
pub const fn part_middle(v: &[[i16; 3]], faces: &[RawFace], part: u8) -> [i64; 3] {
    let mut s = [0i64; 3];
    let mut n = 0i64;
    let mut i = 0;
    while i < faces.len() {
        if faces[i].part == part {
            let mut k = 0;
            while k < 3 {
                let p = v[faces[i].v[k] as usize];
                s[0] += p[0] as i64;
                s[1] += p[1] as i64;
                s[2] += p[2] as i64;
                k += 1;
            }
            n += 3;
        }
        i += 1;
    }
    if n == 0 {
        return s;
    }
    [s[0] / n, s[1] / n, s[2] / n]
}

/// Turns every face so that its front looks away from the middle of its
/// part. Runs while compiling.
pub const fn orient<const N: usize>(v: &[[i16; 3]], raw: &[RawFace; N]) -> [Face; N] {
    let mut out = [Face { v: [0; 3], mat: 0 }; N];
    let mut i = 0;
    while i < N {
        let f = raw[i];
        let m = part_middle(v, raw, f.part);
        let n = cross_of(v, f.v);
        let a = v[f.v[0] as usize];
        let out_dir = [a[0] as i64 - m[0], a[1] as i64 - m[1], a[2] as i64 - m[2]];
        let dot = n[0] * out_dir[0] + n[1] * out_dir[1] + n[2] * out_dir[2];
        out[i] = if dot < 0 {
            Face {
                v: [f.v[0], f.v[2], f.v[1]],
                mat: f.mat,
            }
        } else {
            Face { v: f.v, mat: f.mat }
        };
        i += 1;
    }
    out
}

/// Each face's direction, 16384 long. Runs while compiling.
pub const fn normals<const N: usize>(v: &[[i16; 3]], faces: &[Face; N]) -> [[i16; 3]; N] {
    let mut out = [[0i16; 3]; N];
    let mut i = 0;
    while i < N {
        let n = cross_of(v, faces[i].v);
        let len = isqrt((n[0] * n[0] + n[1] * n[1] + n[2] * n[2]) as u64) as i64;
        if len > 0 {
            out[i] = [
                (n[0] * ONE14 as i64 / len) as i16,
                (n[1] * ONE14 as i64 / len) as i16,
                (n[2] * ONE14 as i64 / len) as i16,
            ];
        }
        i += 1;
    }
    out
}

/// The radius of the smallest ball round the origin holding every
/// corner (rounded up). Runs while compiling.
pub const fn radius_of(v: &[[i16; 3]]) -> i32 {
    let mut best = 0u64;
    let mut i = 0;
    while i < v.len() {
        let p = v[i];
        let d = (p[0] as i64 * p[0] as i64 + p[1] as i64 * p[1] as i64 + p[2] as i64 * p[2] as i64)
            as u64;
        if d > best {
            best = d;
        }
        i += 1;
    }
    isqrt(best) as i32 + 1
}

// ---- View and planes ------------------------------------------------------------

/// The window on the picture the 3D view is drawn in, its lens, and its
/// five planes (all worked out while compiling).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    /// The pixels the view may paint.
    pub area: Rect,
    /// A thing 1 m wide at 1 m distance is this many pixels.
    pub focal: i32,
    /// The middle of the view in 1/16 pixel.
    pub mid: (i32, i32),
    /// The planes a drawn point must be inside of, as (a, b, c, e):
    /// inside when `a*x + b*y + c*z + e >= 0`. Near, left, right, top,
    /// bottom (the sides at twice the view's size: the guard band).
    pub planes: [[i32; 4]; 5],
    /// The length of each plane's (a, b, c).
    pub len: [i32; 5],
}

impl View {
    pub const fn new(area: Rect, focal: i32) -> View {
        let gx = area.w() / 2 * 2;
        let gy = area.h() / 2 * 2;
        let planes = [
            [0, 0, 1, -NEAR],
            [focal, 0, gx, 0],
            [-focal, 0, gx, 0],
            [0, -focal, gy, 0],
            [0, focal, gy, 0],
        ];
        let mut len = [0i32; 5];
        let mut k = 0;
        while k < 5 {
            let p = planes[k];
            len[k] = isqrt((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]) as u64) as i32;
            k += 1;
        }
        View {
            area,
            focal,
            mid: (
                area.x0 * SUB + area.w() * SUB / 2,
                area.y0 * SUB + area.h() * SUB / 2,
            ),
            planes,
            len,
        }
    }

    /// Where a point in camera space lands, in 1/16 pixel (z at least
    /// [`NEAR`]). Parts below 2^15 and `focal * 16` below 2^11: the
    /// products are below 2^26, so the division is a 32-bit one.
    #[inline(never)]
    pub fn project(&self, p: V3) -> [i32; 2] {
        let f = self.focal.wrapping_mul(SUB);
        let z = p[2].max(1);
        debug_assert!(i64::from(p[0]).abs() * i64::from(f) < 1 << 31);
        debug_assert!(i64::from(p[1]).abs() * i64::from(f) < 1 << 31);
        [
            self.mid.0.wrapping_add(p[0].wrapping_mul(f) / z),
            self.mid.1.wrapping_sub(p[1].wrapping_mul(f) / z),
        ]
    }
}

/// `a*x + b*y + c*z + e`: how far the point is inside the plane, times
/// the plane's length. Plane parts below 2^9 and points below 2^16:
/// every product is below 2^25.
#[inline(always)]
pub fn plane_dist(pl: &[i32; 4], p: &V3) -> i32 {
    dot([pl[0], pl[1], pl[2]], *p).wrapping_add(pl[3])
}

/// Where a point is in camera space, and whether a ball of `radius`
/// round it is in view at all (nearer than `far`). `rot` is world to
/// camera; the eye is at the world's origin.
pub fn in_view(view: &View, rot: &Mat3, pos: V3, radius: i32, far: i32) -> Option<V3> {
    let c = rot.apply(pos);
    if c[2].wrapping_add(radius) < NEAR || c[2].wrapping_sub(radius) > far {
        return None;
    }
    for k in 1..5 {
        if plane_dist(&view.planes[k], &c) < radius.wrapping_neg().wrapping_mul(view.len[k]) {
            return None;
        }
    }
    Some(c)
}

// ---- Clipping -------------------------------------------------------------------

/// Most corners a triangle can have after it is cut by five planes.
pub const MAX_POLY: usize = 8;

/// Cuts a convex polygon (camera space) to the inside of the planes
/// whose bits are set in `which`, one plane after the other (Sutherland
/// and Hodgman's way). Returns the corners left (0 when nothing is).
pub fn clip_polygon(
    poly: &mut [V3; MAX_POLY],
    mut n: usize,
    planes: &[[i32; 4]; 5],
    which: u8,
) -> usize {
    let mut tmp = [[0i32; 3]; MAX_POLY];
    for (k, pl) in planes.iter().enumerate() {
        if which & (1 << k) == 0 || n == 0 {
            continue;
        }
        let mut m = 0;
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            let (da, db) = (plane_dist(pl, &a), plane_dist(pl, &b));
            if da >= 0 && m < MAX_POLY {
                tmp[m] = a;
                m += 1;
            }
            if (da >= 0) != (db >= 0) && m < MAX_POLY {
                // |b - a| below 2^16 and |da| below 2^25: the product
                // needs 64 bits (only here, for cut faces).
                let den = i64::from(da.wrapping_sub(db));
                let mut p = [0i32; 3];
                for (j, v) in p.iter_mut().enumerate() {
                    let d = i64::from(b[j].wrapping_sub(a[j]));
                    *v = a[j].wrapping_add((d.wrapping_mul(i64::from(da)) / den) as i32);
                }
                tmp[m] = p;
                m += 1;
            }
        }
        poly[..m].copy_from_slice(&tmp[..m]);
        n = m;
    }
    if n < 3 { 0 } else { n }
}

// ---- The renderer --------------------------------------------------------------

/// What the renderer did since [`Renderer::begin`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Objects drawn (at least partly in view).
    pub objects: u32,
    /// Corners turned, moved and (most) projected.
    pub verts: u32,
    /// Faces of those objects.
    pub faces: u32,
    /// Faces left after back faces and faces fully outside are dropped.
    pub front: u32,
    /// Faces that had to be cut by a plane.
    pub clipped: u32,
    /// Triangles handed to the filler; with [`Paint::ONLY_PAINTED`] (the
    /// city) only those that painted at least one pixel.
    pub tris: u32,
    /// Triangles that did not fit [`MAX_TRIS`] (should stay 0).
    pub dropped: u32,
    /// Pixels painted (a pixel painted twice counts twice).
    pub pixels: u32,
}

/// One triangle waiting for its turn: corners in 1/16 pixel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tri {
    p: [[i16; 2]; 3],
    c: u8,
    /// The corners' depths (camera space), for the checker only.
    #[cfg(feature = "check")]
    z: [i16; 3],
}

/// Triangles the recorder keeps per picture (feature `check`).
#[cfg(feature = "check")]
pub const REC_MAX: usize = 8192;

/// One triangle handed to the filler, as the recorder keeps it (feature
/// `check`, PC only: a measuring tool, never in a game build): corners
/// in 1/16 pixel, their depths in camera space, the colour, and the
/// number [`Renderer::owner`] had when it was drawn.
#[cfg(feature = "check")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecTri {
    pub p: [[i16; 2]; 3],
    pub z: [i16; 3],
    pub c: u8,
    pub owner: u16,
}

/// The renderer's working memory: one model's corners and triangles
/// (about 3.4 KB).
pub struct Renderer {
    /// Corners in camera space.
    cs: [V3; MAX_VERTS],
    /// Corners on screen (1/16 pixel), for corners inside all planes.
    pub sp: [[i16; 2]; MAX_VERTS],
    /// Which planes each corner is outside of (bit k: plane k).
    out: [u8; MAX_VERTS],
    tris: [Tri; MAX_TRIS],
    /// Depth << 8 | triangle number, for sorting.
    keys: [u32; MAX_TRIS],
    pub stats: Stats,
    /// The recorder (feature `check` only): every triangle handed to the
    /// filler since [`Renderer::begin`], in order; `nrec` may pass
    /// [`REC_MAX`] (then the rest were not kept).
    #[cfg(feature = "check")]
    pub rec: [RecTri; REC_MAX],
    #[cfg(feature = "check")]
    pub nrec: usize,
    /// Set by the caller before each object: kept with its triangles.
    #[cfg(feature = "check")]
    pub owner: u16,
}

/// One object to draw.
#[derive(Clone, Copy, Debug)]
pub struct Object<'a> {
    pub model: &'a Model<'a>,
    /// Model to world turning.
    pub rot: Mat3,
    /// Its middle in camera space (from [`in_view`]).
    pub centre: V3,
}

/// The light and colours of a scene: light falls along `light` (world,
/// 16384 long, from the light to the scene); palette number = `first +
/// shade * MATS + material`, shade 0 the brightest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Light {
    pub dir: V3,
    pub first: u8,
}

/// How an object's faces get their colours: the light's direction
/// (world, 16384 long) and the palette number of a material at a shade
/// (0 the brightest, at most `SHADES - 2`). [`Light`] is the balls'
/// way; the city gives each object its own four materials.
pub trait Paint {
    /// Draw the object's triangles in the order of its faces instead of
    /// sorting them far to near. For models whose face order is already
    /// right from every place the eye can be (the city's: flat ground
    /// pieces in layers, a box on top drawn before the box under it).
    const KEEP_ORDER: bool = false;
    /// Count only what is really drawn (the city's rule; the balls keep
    /// the first round's rule, every triangle handed to the filler):
    /// a triangle whose box holds no pixel middle of the view is not
    /// handed to the filler at all, and [`Stats::tris`] counts only the
    /// triangles that painted at least one pixel.
    const ONLY_PAINTED: bool = false;
    fn dir(&self) -> V3;
    fn colour(&self, mat: u8, shade: u8) -> u8;
}

impl Paint for Light {
    #[inline(always)]
    fn dir(&self) -> V3 {
        self.dir
    }

    #[inline(always)]
    fn colour(&self, mat: u8, shade: u8) -> u8 {
        self.first
            .wrapping_add(shade.wrapping_mul(MATS))
            .wrapping_add(mat)
    }
}

impl Renderer {
    pub const fn new() -> Renderer {
        Renderer {
            cs: [[0; 3]; MAX_VERTS],
            sp: [[0; 2]; MAX_VERTS],
            out: [0; MAX_VERTS],
            tris: [Tri {
                p: [[0; 2]; 3],
                c: 0,
                #[cfg(feature = "check")]
                z: [0; 3],
            }; MAX_TRIS],
            keys: [0; MAX_TRIS],
            stats: Stats {
                objects: 0,
                verts: 0,
                faces: 0,
                front: 0,
                clipped: 0,
                tris: 0,
                dropped: 0,
                pixels: 0,
            },
            #[cfg(feature = "check")]
            rec: [RecTri {
                p: [[0; 2]; 3],
                z: [0; 3],
                c: 0,
                owner: 0,
            }; REC_MAX],
            #[cfg(feature = "check")]
            nrec: 0,
            #[cfg(feature = "check")]
            owner: 0,
        }
    }

    /// Starts a new picture's counts.
    pub fn begin(&mut self) {
        self.stats = Stats::default();
        #[cfg(feature = "check")]
        {
            self.nrec = 0;
        }
    }

    /// The corner stage: every corner of `m` turned by `to_cam`, moved
    /// to `centre` (camera space), flagged against the planes (unless
    /// `all_in`) and projected when inside; at most `limit` corners.
    /// Returns the corners done.
    #[inline(never)]
    pub fn verts(
        &mut self,
        view: &View,
        m: &Model,
        to_cam: &Mat3,
        centre: V3,
        all_in: bool,
        limit: usize,
    ) -> usize {
        let nv = m.verts.len().min(MAX_VERTS).min(limit);
        for i in 0..nv {
            let v = m.verts[i];
            let p = to_cam.apply([i32::from(v[0]), i32::from(v[1]), i32::from(v[2])]);
            // Both below 2^14 (the scene is within 32 m): no overflow.
            let p = [0, 1, 2].map(|k| p[k].wrapping_add(centre[k]));
            self.cs[i] = p;
            let mut out = 0u8;
            if !all_in {
                for (k, pl) in view.planes.iter().enumerate() {
                    if plane_dist(pl, &p) < 0 {
                        out |= 1u8 << k;
                    }
                }
            }
            self.out[i] = out;
            if out == 0 {
                let s = view.project(p);
                self.sp[i] = [s[0] as i16, s[1] as i16];
            }
        }
        // Below MAX_VERTS (64): no overflow.
        self.stats.verts = self.stats.verts.wrapping_add(nv as u32);
        nv
    }

    /// Draws one object into `f`, inside `view.area`, faces far to near.
    /// `cam` is world to camera. Objects must be drawn far to near by
    /// the caller.
    pub fn draw(&mut self, f: &mut Frame, view: &View, cam: &Mat3, light: &Light, o: &Object<'_>) {
        self.draw_with(f, view, cam, light, o);
    }

    /// [`Renderer::draw`] with any [`Paint`].
    #[inline(always)]
    pub fn draw_with<P: Paint>(
        &mut self,
        f: &mut Frame,
        view: &View,
        cam: &Mat3,
        paint: &P,
        o: &Object<'_>,
    ) {
        let m = o.model;
        let centre = o.centre;
        let radius = m.radius;
        let to_cam = cam.mul(&o.rot);
        // Is the whole ball inside every plane? Then nothing is cut.
        let all_in = centre[2].wrapping_sub(radius) >= NEAR
            && (1..5)
                .all(|k| plane_dist(&view.planes[k], &centre) >= radius.wrapping_mul(view.len[k]));
        let nv = self.verts(view, m, &to_cam, centre, all_in, MAX_VERTS);
        // The eye and the light in the model's own space: a face looks
        // at the eye when its normal points back towards it.
        let back = to_cam.transposed();
        let eye = back.apply([0, 1, 2].map(|k| centre[k].wrapping_neg()));
        let lm = o.rot.transposed().apply(paint.dir());
        self.stats.objects = self.stats.objects.wrapping_add(1);
        let mut nt = 0usize;
        for (fi, face) in m.faces.iter().enumerate() {
            self.stats.faces = self.stats.faces.wrapping_add(1);
            let [ia, ib, ic] = face.v.map(usize::from);
            if ia >= nv || ib >= nv || ic >= nv {
                continue;
            }
            let (oa, ob, oc) = (self.out[ia], self.out[ib], self.out[ic]);
            if oa & ob & oc != 0 {
                continue;
            }
            let nm = m.normals[fi];
            let n = [i32::from(nm[0]), i32::from(nm[1]), i32::from(nm[2])];
            let va = m.verts[ia];
            // Model corners below 2^9 and the eye within 32 m (2^13).
            let to_a = [0, 1, 2].map(|k| i32::from(va[k]).wrapping_sub(eye[k]));
            if dot(n, to_a) >= 0 {
                continue;
            }
            self.stats.front = self.stats.front.wrapping_add(1);
            // Light on the face: 0 (none) to 16384 (straight on).
            let lit = (dot(n, lm) >> SHIFT14).wrapping_neg().clamp(0, ONE14);
            // (16384 - lit) * 6 is below 2^17.
            let shade =
                (ONE14.wrapping_sub(lit).wrapping_mul(i32::from(SHADES - 2)) >> SHIFT14) as u8;
            let colour = paint.colour(face.mat, shade);
            let (a, b, c) = (self.cs[ia], self.cs[ib], self.cs[ic]);
            // Three depths below 2^14 each: the sum is below 2^16.
            let depth = a[2].wrapping_add(b[2]).wrapping_add(c[2]);
            if oa | ob | oc == 0 {
                let t = Tri {
                    p: [self.sp[ia], self.sp[ib], self.sp[ic]],
                    c: colour,
                    #[cfg(feature = "check")]
                    z: [a[2] as i16, b[2] as i16, c[2] as i16],
                };
                self.push(&mut nt, t, depth);
                continue;
            }
            self.stats.clipped = self.stats.clipped.wrapping_add(1);
            let mut poly = [[0i32; 3]; MAX_POLY];
            poly[0] = a;
            poly[1] = b;
            poly[2] = c;
            let n = clip_polygon(&mut poly, 3, &view.planes, oa | ob | oc);
            let mut sp = [[0i16; 2]; MAX_POLY];
            for k in 0..n {
                let s = view.project(poly[k]);
                sp[k] = [
                    s[0].clamp(-GUARD, GUARD) as i16,
                    s[1].clamp(-GUARD, GUARD) as i16,
                ];
            }
            // A fan of triangles from the first corner (n is 0 or 3..=8).
            for k in 1..n.saturating_sub(1) {
                let t = Tri {
                    p: [sp[0], sp[k], sp[k.wrapping_add(1)]],
                    c: colour,
                    #[cfg(feature = "check")]
                    z: [
                        poly[0][2] as i16,
                        poly[k][2] as i16,
                        poly[k.wrapping_add(1)][2] as i16,
                    ],
                };
                self.push(&mut nt, t, depth);
            }
        }
        // Far to near (insertion sort: a few dozen triangles).
        if !P::KEEP_ORDER {
            let keys = &mut self.keys[..nt];
            for i in 1..keys.len() {
                let k = keys[i];
                let mut j = i;
                while j > 0 && keys[j.wrapping_sub(1)] < k {
                    keys[j] = keys[j.wrapping_sub(1)];
                    j = j.wrapping_sub(1);
                }
                keys[j] = k;
            }
        }
        for i in 0..nt {
            let t = self.tris[(self.keys[i] & 0x7F) as usize];
            let p = t.p.map(|q| [i32::from(q[0]), i32::from(q[1])]);
            if P::ONLY_PAINTED && no_pixel_middle(&p, &view.area) {
                continue;
            }
            #[cfg(feature = "check")]
            {
                if self.nrec < REC_MAX {
                    self.rec[self.nrec] = RecTri {
                        p: t.p,
                        z: t.z,
                        c: t.c,
                        owner: self.owner,
                    };
                }
                self.nrec += 1;
            }
            let painted = fill_tri(f, &view.area, &p, t.c);
            self.stats.pixels = self.stats.pixels.wrapping_add(painted);
            if !P::ONLY_PAINTED || painted > 0 {
                self.stats.tris = self.stats.tris.wrapping_add(1);
            }
        }
    }

    fn push(&mut self, nt: &mut usize, t: Tri, depth: i32) {
        if *nt >= MAX_TRIS {
            self.stats.dropped = self.stats.dropped.wrapping_add(1);
            return;
        }
        self.tris[*nt] = t;
        self.keys[*nt] = ((depth.clamp(0, 0xFF_FFFF) as u32) << 8) | *nt as u32;
        *nt = nt.wrapping_add(1);
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer::new()
    }
}
