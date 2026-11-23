//! Simple 3D with polygons: everything between a model table and the
//! pixels, in whole numbers. Nothing in here knows the game. Taken from
//! KESTREL (`games/kestrel/src/poly.rs`); changed here: more corners and
//! triangles per model (the planet's sphere), and a 2D line for laser
//! beams and dust streaks ([`line`]).
//!
//! The way a model reaches the picture:
//!
//! 1. **Models** ([`Model`]) are small tables: corners (vertices) in
//!    1/256 m and faces (three corner numbers and a material). Each face
//!    belongs to a convex *part*; [`orient`] turns every face so its
//!    front looks away from its part's middle, while compiling, and
//!    [`normals`] works out each face's direction (its "normal") for the
//!    light. So a table never has to be wound by hand.
//! 2. **Turning and moving**: each corner is turned by the object's
//!    rotation matrix, scaled, moved to the object's place and then
//!    into the camera's view ([`Camera`]: x right, y up, z forward).
//! 3. **Back faces** are dropped: a face whose front looks away from the
//!    eye is hidden behind the rest of its part.
//! 4. **Clipping**: a face that crosses the near plane (just in front of
//!    the eye) or reaches far outside the view is cut to the part that
//!    is inside, in 3D, before it is drawn ([`clip_polygon`]). Faces only
//!    a little outside the view are not cut: the filler simply stops at
//!    the view's edge (a "guard band" around the view saves the cutting).
//! 5. **Perspective**: x and y are divided by the distance z
//!    (`x * focal / z`), to 1/16 of a pixel ([`SUB`]).
//! 6. **Painter's order**: there is no depth buffer (no memory for one).
//!    The game draws its objects from far to near, and inside one object
//!    the faces are sorted far to near too ([`Renderer::draw`]).
//! 7. **Filling** ([`fill_tri`]): each triangle is filled row by row in
//!    one flat colour. The edges are followed exactly (whole numbers,
//!    no rounding drift), with the usual "top-left" rule, so two
//!    triangles that share an edge never both paint a pixel on it and
//!    never leave a gap. [`fill_tri_slow`] is the slow, obvious version
//!    the tests compare it with.
//!
//! Units: positions in 1/256 m ([`M8`]), screen positions in 1/16 pixel
//! ([`SUB`]), matrices in 1/16384 ([`crate::fixed::ONE14`]).

use game_kit::{Frame, H, W};

use crate::fixed::{Mat3, ONE14, SHIFT14, isqrt, sub, wide};

/// One metre in model and camera units.
pub const M8: i32 = 256;
/// Steps per pixel in screen positions (1/16 pixel).
pub const SUB: i32 = 16;
/// The filler takes screen positions from -GUARD to +GUARD (in 1/16
/// pixel: about 512 pixels each way). Beyond that it draws nothing, so
/// its sums never run over. The renderer's clipping keeps every
/// triangle well inside.
pub const GUARD: i32 = 8191;
/// The near plane: nothing closer to the eye than this (1/256 m).
pub const NEAR: i32 = M8 / 2;
/// Most corners in one model.
pub const MAX_VERTS: usize = 64;
/// Most triangles one model can put on screen at once (after back
/// faces are dropped and clipped faces are cut into triangles).
pub const MAX_TRIS: usize = 128;

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

/// The first pixel row whose middle (row * 16 + 8) is at or below `y`
/// (`y` in 1/16 pixel).
fn first_row(y: i32) -> i32 {
    // y is within GUARD (2^13): no sum can run over.
    let y = y.wrapping_sub(SUB / 2);
    y.div_euclid(SUB)
        .wrapping_add(i32::from(y.rem_euclid(SUB) != 0))
}

/// Follows one edge of a triangle down the pixel rows, exactly: for
/// each row it gives the first pixel whose middle is at or right of the
/// edge (rounded up), with no rounding error that grows row by row.
///
/// For the edge from `a` to `b` (b below a) and row `y`:
/// `N = a.x*dy + (16y + 8 - a.y)*dx - 8*dy`, and that pixel is
/// `ceil(N / (16*dy))`. Going one row down adds `16*dx` to N; the
/// quotient and the remainder are kept apart, so no division is needed
/// per row.
struct Edge {
    q: i32,
    r: i32,
    d: i32,
    sq: i32,
    sr: i32,
}

impl Edge {
    /// Range: corners are within [`GUARD`] (2^13) and rows within the
    /// picture, so every product here is below 2^28: the sums are written
    /// without overflow checks (they cost 340 bytes of chip code).
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
}

/// `n` divided by `d` (above 0), rounded down, and the remainder
/// (0..d). Small numbers (below 2^28): the sums cannot run over.
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

impl Edge {
    /// The first pixel at or right of the edge on this row.
    /// (Same range as [`Edge::new`]: the sums cannot run over.)
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

/// Fills a triangle in one colour, inside `clip` (and the picture).
/// Corners in 1/16 pixel; any order, any turning direction. Returns the
/// pixels painted.
///
/// A pixel is painted when its middle is inside the triangle. A middle
/// exactly on an edge is painted only for a left edge or a flat top
/// edge (the "top-left rule"), so triangles that share an edge share
/// its pixels out exactly once. A triangle with no area paints nothing.
/// Corners beyond [`GUARD`] paint nothing.
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
    // Which side of the long edge (v0 to v2) the middle corner is on.
    // Corners within GUARD (2^13): products below 2^28, no overflow.
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
            // Rows and columns are on the picture: the sums are small.
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

/// The slow, obvious triangle filler the tests compare [`fill_tri`]
/// with: every pixel of `clip` is tested on its own against the three
/// edges, with the same top-left rule. Not used by the game.
pub fn fill_tri_slow(f: &mut Frame, clip: &Rect, p: &[[i32; 2]; 3], c: u8) -> u32 {
    if p.iter()
        .any(|v| v[0].unsigned_abs() > GUARD as u32 || v[1].unsigned_abs() > GUARD as u32)
    {
        return 0;
    }
    let clip = clip.on_screen();
    let v = p.map(|q| [i64::from(q[0]), i64::from(q[1])]);
    // Twice the area; its sign says which way round the corners go.
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
                // Edge function, made positive inside.
                let e = sign * ((b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0]));
                let (ex, ey) = (-(sign * (b[1] - a[1])), sign * (b[0] - a[0]));
                // On the edge: only a left edge (inside lies to its
                // right, ex > 0) or a flat top edge (inside below it).
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

/// A model: corners in 1/256 m, faces, and each face's direction (unit
/// length in 1/16384, made while compiling), and the radius of a ball
/// round the origin that holds every corner (1/256 m).
#[derive(Clone, Copy, Debug)]
pub struct Model {
    pub verts: &'static [[i16; 3]],
    pub faces: &'static [Face],
    pub normals: &'static [[i16; 3]],
    pub radius: i32,
}

/// The face's direction before any turning: `(b - a) x (c - a)`. The
/// front of a face is the side this points to.
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
/// part (two corners swapped where needed). Runs while compiling.
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

/// Each face's direction, one long (16384). Runs while compiling.
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

// ---- Camera, view, shading -------------------------------------------------------

/// Where the eye is and which way it looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Camera {
    /// The eye, in world units (1/256 m).
    pub pos: [i32; 3],
    /// World to camera: the camera's own turning, taken back (for a
    /// camera turned by `R`, this is `R` transposed).
    pub rot: Mat3,
}

/// The window on the picture the 3D view is drawn in, and its lens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    /// The pixels the view may paint.
    pub area: Rect,
    /// The lens: a thing 1 m wide at 1 m distance is this many pixels.
    pub focal: i32,
    /// The guard band: faces reaching more than this many times half
    /// the view's width (or height) beyond the middle are cut in 3D;
    /// closer ones are only cut by the filler.
    pub guard: i32,
    /// The middle of the view in 1/16 pixel, worked out once.
    mid: (i32, i32),
}

impl View {
    pub const fn new(area: Rect, focal: i32) -> View {
        View {
            area,
            focal,
            guard: 2,
            mid: (
                area.x0 * SUB + area.w() * SUB / 2,
                area.y0 * SUB + area.h() * SUB / 2,
            ),
        }
    }

    /// The middle of the view in 1/16 pixel.
    pub const fn centre(&self) -> (i32, i32) {
        self.mid
    }

    /// Where a point in camera space lands, in 1/16 pixel (z at least
    /// [`NEAR`]). A point far off the side may come out as any number
    /// far off the picture (the filler drops corners beyond [`GUARD`]),
    /// so the last sums wrap instead of being checked.
    #[inline(never)]
    pub fn project(&self, p: [i32; 3]) -> [i32; 2] {
        let f = self.focal.wrapping_mul(SUB);
        let z = i64::from(p[2].max(1));
        let q = |v: i32| (wide(v, f) / z) as i32;
        [
            self.mid.0.wrapping_add(q(p[0])),
            self.mid.1.wrapping_sub(q(p[1])),
        ]
    }

    /// The five planes a drawn point must be inside of, as (a, b, c, e):
    /// inside when `a*x + b*y + c*z + e >= 0`. Near, left, right, top,
    /// bottom (the sides at the guard band).
    pub fn planes(&self) -> [[i32; 4]; 5] {
        let f = self.focal;
        let gx = self.area.w() / 2 * self.guard;
        let gy = self.area.h() / 2 * self.guard;
        [
            [0, 0, 1, -NEAR],
            [f, 0, gx, 0],
            [-f, 0, gx, 0],
            [0, -f, gy, 0],
            [0, f, gy, 0],
        ]
    }

    /// The planes and their lengths, worked out once for many objects.
    pub fn clip_planes(&self) -> Planes {
        let p = self.planes();
        let mut len = [0i64; 5];
        for (l, pl) in len.iter_mut().zip(p.iter()) {
            // Plane parts below 2^16: the sum is below 2^34.
            let sq = wide(pl[0], pl[0])
                .wrapping_add(wide(pl[1], pl[1]))
                .wrapping_add(wide(pl[2], pl[2]));
            *l = isqrt(sq as u64) as i64;
        }
        Planes { p, len }
    }
}

/// A view's five planes ([`View::planes`]) and the length of each
/// plane's (a, b, c), so the distance of a point from a plane is
/// `(a*x + b*y + c*z + e) / len`. The square roots are taken once per
/// view, not once per object (on the chip a 64-bit division is slow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Planes {
    pub p: [[i32; 4]; 5],
    pub len: [i64; 5],
}

impl Planes {
    /// How far `c` (camera space) is inside side plane `k`, times the
    /// plane's length (negative: outside).
    fn side(&self, k: usize, c: [i32; 3]) -> i64 {
        plane_dist(&self.p[k], &c)
    }
}

/// `a*x + b*y + c*z + e` for the plane (a, b, c, e) and the point: how far
/// the point is inside the plane, times the length of (a, b, c)
/// (negative: outside). One function for every caller, not inlined: on
/// the chip a 64-bit sum is long, and the code slot is small.
#[inline(never)]
pub fn plane_dist(pl: &[i32; 4], p: &[i32; 3]) -> i64 {
    // Plane parts are below 2^16 and points below 2^31: every product is
    // below 2^47, the sum far inside i64.
    wide(pl[0], p[0])
        .wrapping_add(wide(pl[1], p[1]))
        .wrapping_add(wide(pl[2], p[2]))
        .wrapping_add(i64::from(pl[3]))
}

/// How faces get their palette number: palette number =
/// `shade * mats + material`, with shade 0 the brightest and
/// `shades - 1` the darkest. Light falls along `light` (unit length, in
/// 1/16384, pointing from the light to the scene). Materials whose bit
/// is set in `glow` shine by themselves: no light, but still fog. Fog
/// darkens faces from `fog_from` to `fog_to` (camera distance, 1/256 m)
/// by up to `fog_shades` steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shading {
    pub mats: u8,
    pub shades: u8,
    pub light: [i32; 3],
    pub glow: u32,
    pub fog_from: i32,
    pub fog_to: i32,
    pub fog_shades: u8,
    /// Materials the fog never dims (bit per material): what the player
    /// must find from afar.
    pub clear: u32,
    /// Shade of a face turned fully away from the light (0..shades).
    pub dark_side: u8,
}

impl Shading {
    /// The shade of a face from its turned direction (world space,
    /// 1/16384), material and depth.
    pub fn shade(&self, mat: u8, n_world: [i32; 3], depth: i32) -> u8 {
        // Bit `mat` (materials below 32: the shift never runs over).
        let bit = 1u32.wrapping_shl(u32::from(mat));
        let lit = if self.glow & bit != 0 {
            0
        } else {
            // Light on the face: 0 (none) .. 16384 (straight on). Both
            // are unit vectors (parts at most 2^14), so the sum is at
            // most 3 * 2^28 and cannot run over: no checks.
            let n = n_world;
            let l = self.light;
            let d = (n[0].wrapping_mul(l[0]))
                .wrapping_add(n[1].wrapping_mul(l[1]))
                .wrapping_add(n[2].wrapping_mul(l[2]))
                .wrapping_neg()
                >> SHIFT14;
            let d = d.clamp(0, ONE14);
            // 0..=2^14 times a shade below 2^8.
            ((ONE14
                .wrapping_sub(d)
                .wrapping_mul(i32::from(self.dark_side)))
                >> SHIFT14) as u8
        };
        let fog = if self.clear & bit != 0 || depth <= self.fog_from || self.fog_to <= self.fog_from
        {
            0
        } else {
            // Depths below 2^23, fog shades below 2^8: no overflow.
            let span = self.fog_to.wrapping_sub(self.fog_from);
            let t = depth.wrapping_sub(self.fog_from).min(span);
            (t.wrapping_mul(i32::from(self.fog_shades)) / span) as u8
        };
        // Shades and materials are small; the palette has 256 entries
        // (an index past it would only be a wrong colour).
        lit.wrapping_add(fog).min(self.shades.wrapping_sub(1))
    }

    pub fn colour(&self, mat: u8, shade: u8) -> u8 {
        shade.wrapping_mul(self.mats).wrapping_add(mat)
    }
}

// ---- Clipping -------------------------------------------------------------------

/// Most corners a triangle can have after it is cut by five planes.
pub const MAX_POLY: usize = 8;

/// Cuts a convex polygon (camera space) to the inside of the planes
/// whose bits are set in `which` (bit k: `planes[k]`), one plane after
/// the other (Sutherland and Hodgman's way). Returns the number of
/// corners left in `poly` (0 when nothing is inside).
pub fn clip_polygon(
    poly: &mut [[i32; 3]; MAX_POLY],
    mut n: usize,
    planes: &[[i32; 4]; 5],
    which: u8,
) -> usize {
    let mut tmp = [[0i32; 3]; MAX_POLY];
    for (k, pl) in planes.iter().enumerate() {
        if which & (1 << k) == 0 || n == 0 {
            continue;
        }
        let dist = |p: &[i32; 3]| plane_dist(pl, p);
        let mut m = 0;
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            let (da, db) = (dist(&a), dist(&b));
            if da >= 0 && m < MAX_POLY {
                tmp[m] = a;
                m += 1;
            }
            if (da >= 0) != (db >= 0) && m < MAX_POLY {
                // The point where the edge crosses the plane.
                // Range: |da| is at most |den| and below 2^32 (corners
                // within 2^22 of the eye, plane parts below 2^10), and
                // |b - a| below 2^23, so the product is below 2^55: it is
                // written without the 64-bit overflow check (checked in
                // debug builds).
                let den = da.wrapping_sub(db);
                let mut p = [0i32; 3];
                for (j, v) in p.iter_mut().enumerate() {
                    let d = i64::from(b[j].wrapping_sub(a[j]));
                    debug_assert!(d.checked_mul(da).is_some(), "clip out of range");
                    *v = a[j].wrapping_add((d.wrapping_mul(da) / den) as i32);
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

/// What the renderer did for the last picture (for the cost tests and
/// the log).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Objects drawn (at least partly in view).
    pub objects: u32,
    /// Corners turned and moved into camera space (each one a matrix
    /// times a vector; most are also projected: two divisions).
    pub verts: u32,
    /// Faces of those objects (before back faces are dropped).
    pub faces: u32,
    /// Faces left after back faces and faces fully outside are dropped.
    pub front: u32,
    /// Faces that had to be cut by a plane.
    pub clipped: u32,
    /// Triangles filled.
    pub tris: u32,
    /// Triangles that did not fit [`MAX_TRIS`] (should stay 0).
    pub dropped: u32,
    /// Pixels painted by the filler (a pixel painted twice counts twice).
    pub pixels: u32,
}

/// One triangle waiting for its turn: corners in 1/16 pixel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tri {
    p: [[i16; 2]; 3],
    c: u8,
}

/// The renderer's working memory: one model's corners and triangles.
/// About 3.3 KB; kept in the game's state, never on the stack.
pub struct Renderer {
    /// Corners in camera space.
    cs: [[i32; 3]; MAX_VERTS],
    /// Corners on screen (1/16 pixel), for corners inside all planes.
    sp: [[i16; 2]; MAX_VERTS],
    /// Which planes each corner is outside of (bit k: plane k).
    out: [u8; MAX_VERTS],
    tris: [Tri; MAX_TRIS],
    /// Depth (bigger is farther) << 8 | triangle number, for sorting.
    keys: [u32; MAX_TRIS],
    /// The last view drawn in, and its planes (worked out again only
    /// when the view changes).
    seen: Option<View>,
    planes: Planes,
    pub stats: Stats,
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer::new()
    }
}

/// One object to draw: a model, where it is, how it is turned and how
/// big it is.
#[derive(Clone, Copy, Debug)]
pub struct Object<'a> {
    pub model: &'a Model,
    /// Its middle, in world units (1/256 m).
    pub pos: [i32; 3],
    /// Model to world turning.
    pub rot: Mat3,
    /// Size: 256 is the model's own size.
    pub scale: i32,
    /// Faces of material `swap.0` are drawn in material `swap.1` (one
    /// model, several looks: a gate that is next, later or passed).
    pub swap: (u8, u8),
}

/// Where an object's middle is in camera space, and whether its ball is
/// in view at all (`planes` from [`View::clip_planes`]).
pub fn in_view(
    planes: &Planes,
    cam: &Camera,
    pos: [i32; 3],
    radius: i32,
    far: i32,
) -> Option<[i32; 3]> {
    // Positions inside the world (2^29) and radii below 2^24: the
    // differences and sums fit (checked in debug builds by `sub`).
    let c = cam.rot.apply(sub(pos, cam.pos));
    if c[2].wrapping_add(radius) < NEAR || c[2].wrapping_sub(radius) > far {
        return None;
    }
    for k in 1..5 {
        if planes.side(k, c) < wide(radius.wrapping_neg(), planes.len[k] as i32) {
            return None;
        }
    }
    Some(c)
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
            }; MAX_TRIS],
            keys: [0; MAX_TRIS],
            seen: None,
            planes: Planes {
                p: [[0; 4]; 5],
                len: [0; 5],
            },
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
        }
    }

    /// Starts a new picture's counts.
    pub fn begin(&mut self) {
        self.stats = Stats::default();
    }

    /// Draws one object into `f`, inside `view.area`, faces far to near.
    /// Objects themselves must be drawn far to near by the caller.
    /// Returns false when the object is not in view.
    pub fn draw(
        &mut self,
        f: &mut Frame,
        view: &View,
        cam: &Camera,
        shading: &Shading,
        o: &Object<'_>,
        far: i32,
    ) -> bool {
        if self.seen != Some(*view) {
            self.seen = Some(*view);
            self.planes = view.clip_planes();
        }
        let m = o.model;
        // Model radii below 2^13 and scales below 2^18 (step M2).
        debug_assert!(m.radius.checked_mul(o.scale).is_some());
        let radius = (m.radius.wrapping_mul(o.scale) / 256).wrapping_add(1);
        let Some(centre) = in_view(&self.planes, cam, o.pos, radius, far) else {
            return false;
        };
        let nv = m.verts.len().min(MAX_VERTS);
        let planes = self.planes.p;
        // Model to camera: the camera's turn after the object's, scaled.
        let to_cam = cam.rot.mul(&o.rot).scaled(o.scale);
        // Is the whole ball inside every plane? Then nothing is cut.
        let all_in = centre[2].wrapping_sub(radius) >= NEAR
            && (1..5)
                .all(|k| self.planes.side(k, centre) >= wide(radius, self.planes.len[k] as i32));
        for i in 0..nv {
            let v = m.verts[i];
            let p = to_cam.apply([i32::from(v[0]), i32::from(v[1]), i32::from(v[2])]);
            // Both parts are below 2^23 (the object is in view, at most
            // `far` away; the model is smaller): no checks needed.
            let p = [0, 1, 2].map(|k| p[k].wrapping_add(centre[k]));
            self.cs[i] = p;
            let mut out = 0u8;
            if !all_in {
                for (k, pl) in planes.iter().enumerate() {
                    if plane_dist(pl, &p) < 0 {
                        out |= 1u8.wrapping_shl(k as u32);
                    }
                }
            }
            self.out[i] = out;
            if out == 0 {
                let s = view.project(p);
                self.sp[i] = [s[0] as i16, s[1] as i16];
            }
        }
        self.stats.objects = self.stats.objects.wrapping_add(1);
        self.stats.verts = self.stats.verts.wrapping_add(nv as u32);
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
            let (a, b, c) = (self.cs[ia], self.cs[ib], self.cs[ic]);
            if !faces_eye(a, b, c) {
                continue;
            }
            self.stats.front = self.stats.front.wrapping_add(1);
            let nm = m.normals[fi];
            let nw = o
                .rot
                .apply([i32::from(nm[0]), i32::from(nm[1]), i32::from(nm[2])]);
            // Corners are below 2^23: the sum cannot run over.
            let depth = a[2].wrapping_add(b[2]).wrapping_add(c[2]) / 3;
            let mat = if face.mat == o.swap.0 {
                o.swap.1
            } else {
                face.mat
            };
            let colour = shading.colour(mat, shading.shade(mat, nw, depth));
            if oa | ob | oc == 0 {
                let t = Tri {
                    p: [self.sp[ia], self.sp[ib], self.sp[ic]],
                    c: colour,
                };
                self.push(&mut nt, t, depth);
                continue;
            }
            // Cut by the planes the corners are outside of.
            self.stats.clipped = self.stats.clipped.wrapping_add(1);
            let mut poly = [[0i32; 3]; MAX_POLY];
            poly[0] = a;
            poly[1] = b;
            poly[2] = c;
            let n = clip_polygon(&mut poly, 3, &planes, oa | ob | oc);
            if n < 3 {
                continue;
            }
            let mut sp = [[0i16; 2]; MAX_POLY];
            for k in 0..n {
                let s = view.project(poly[k]);
                sp[k] = [
                    s[0].clamp(-GUARD, GUARD) as i16,
                    s[1].clamp(-GUARD, GUARD) as i16,
                ];
            }
            // A fan of triangles from the first corner.
            // n is 3..=MAX_POLY here.
            for k in 1..n.wrapping_sub(1) {
                let t = Tri {
                    p: [sp[0], sp[k], sp[k.wrapping_add(1)]],
                    c: colour,
                };
                self.push(&mut nt, t, depth);
            }
        }
        // Far to near (insertion sort: a few dozen triangles).
        let keys = &mut self.keys[..nt];
        for i in 1..keys.len() {
            let k = keys[i];
            let mut j = i;
            // j > 0 in the loop: j - 1 never wraps.
            while j > 0 && keys[j.wrapping_sub(1)] < k {
                keys[j] = keys[j.wrapping_sub(1)];
                j = j.wrapping_sub(1);
            }
            keys[j] = k;
        }
        for i in 0..nt {
            // The low byte is a triangle number below MAX_TRIS (128).
            let t = self.tris[(self.keys[i] & 0x7F) as usize];
            let p = t.p.map(|q| [i32::from(q[0]), i32::from(q[1])]);
            let painted = fill_tri(f, &view.area, &p, t.c);
            self.stats.pixels = self.stats.pixels.wrapping_add(painted);
            self.stats.tris = self.stats.tris.wrapping_add(1);
        }
        true
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

/// Whether the front of the face a, b, c (camera space) looks at the
/// eye (at the origin): its direction points back towards the eye.
pub fn faces_eye(a: [i32; 3], b: [i32; 3], c: [i32; 3]) -> bool {
    // (Ranges below: no sum or difference here can run over.)
    let u = [0, 1, 2].map(|k| b[k].wrapping_sub(a[k]));
    let w = [0, 1, 2].map(|k| c[k].wrapping_sub(a[k]));
    let x = |i: usize, j: usize| wide(u[i], w[j]).wrapping_sub(wide(u[j], w[i]));
    let n = [x(1, 2), x(2, 0), x(0, 1)];
    // Range: everything drawn is within 8 km (2^21) of the eye (bodies
    // farther than 4 km are drawn at 4 km, made smaller), and no face is
    // longer than 2 km (2^19: a ring face of the biggest ball), so each
    // part of `n` is below 2^39 and the sum below 2^62. So the sum is
    // written without 64-bit overflow checks (they cost about 440 bytes
    // of chip code here); the debug check below runs in the PC tests and
    // the fuzz run.
    let s = n[0]
        .wrapping_mul(i64::from(a[0]))
        .wrapping_add(n[1].wrapping_mul(i64::from(a[1])))
        .wrapping_add(n[2].wrapping_mul(i64::from(a[2])));
    debug_assert!(
        n[0].checked_mul(i64::from(a[0]))
            .zip(n[1].checked_mul(i64::from(a[1])))
            .zip(n[2].checked_mul(i64::from(a[2])))
            .and_then(|((x, y), z)| x.checked_add(y)?.checked_add(z))
            == Some(s),
        "faces_eye out of range"
    );
    s < 0
}

/// Fills a rectangle (the part on the picture) in one colour.
pub fn rect_fill(f: &mut Frame, r: &Rect, c: u8) {
    let r = r.on_screen();
    for y in r.y0..r.y1 {
        let at = y as usize * W;
        f.0[at + r.x0 as usize..at + r.x1 as usize].fill(c);
    }
}

/// Draws a line from `a` to `b` (whole pixels) inside `clip`, one pixel
/// a step along the longer side. An end far off the picture (more than
/// 512 pixels out) is first moved half way to the other end, again until
/// it is near, so the loop stays short; the line keeps its direction to
/// within a pixel.
pub fn line(f: &mut Frame, clip: &Rect, a: [i32; 2], b: [i32; 2], c: u8) {
    bolt(f, clip, a, b, c, (0, 8), false);
}

/// Part of a line: the eighths `part.0..=part.1` of the way from `a` to
/// `b` (after far ends are moved in, as for [`line`]); `thick` adds a
/// second pixel beside each one.
#[inline(never)]
pub fn bolt(
    f: &mut Frame,
    clip: &Rect,
    a: [i32; 2],
    b: [i32; 2],
    c: u8,
    part: (i32, i32),
    thick: bool,
) {
    let r = clip.on_screen();
    let far = |p: [i32; 2]| p[0].unsigned_abs() > 512 || p[1].unsigned_abs() > 512;
    // Halves: no sum leaves i32.
    let mid = |p: [i32; 2], q: [i32; 2]| {
        [
            (p[0] / 2).wrapping_add(q[0] / 2),
            (p[1] / 2).wrapping_add(q[1] / 2),
        ]
    };
    let (mut a, mut b) = (a, b);
    for _ in 0..32 {
        if far(a) {
            a = mid(a, b);
        }
        if far(b) {
            b = mid(b, a);
        }
    }
    if far(a) || far(b) {
        return;
    }
    // Both ends within 512: |dx|, |dy|, n <= 1024, products below 2^21.
    let (dx, dy) = (b[0].wrapping_sub(a[0]), b[1].wrapping_sub(a[1]));
    let n = dx.unsigned_abs().max(dy.unsigned_abs()).max(1) as i32;
    // The second pixel: beside the line, across its longer side.
    let side = if dx.unsigned_abs() >= dy.unsigned_abs() {
        (0, 1)
    } else {
        (1, 0)
    };
    let (p0, p1) = (part.0.max(0).min(8), part.1.max(0).min(8));
    for k in n.wrapping_mul(p0) / 8..=n.wrapping_mul(p1) / 8 {
        let (x, y) = (
            a[0].wrapping_add(dx.wrapping_mul(k) / n),
            a[1].wrapping_add(dy.wrapping_mul(k) / n),
        );
        let mut put = |x: i32, y: i32| {
            if x >= r.x0 && x < r.x1 && y >= r.y0 && y < r.y1 {
                f.0[y as usize * W + x as usize] = c;
            }
        };
        put(x, y);
        if thick {
            put(x.wrapping_add(side.0), y.wrapping_add(side.1));
        }
    }
}
