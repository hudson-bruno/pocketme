//! Step G1: the land below (SPEC 3.10, "The land").
//!
//! Three square rings round the camera: near (8 m cells), mid (16 m)
//! and far (32 m, only while the camera is high). Each ring is drawn far
//! to near without a depth buffer, the far ring first:
//!
//! - rings: far, mid, near (each ring lies wholly between the camera and
//!   the next one out);
//! - in a ring: the rows north of the camera's row from the farthest
//!   in, then the rows south of it from the farthest in, then its row;
//! - in a row: the same with the columns east, west, then the camera's;
//! - in a cell: of its two triangles (split SW to NE) the one across the
//!   diagonal from the camera first.
//!
//! A cell can only hide a cell that is no nearer in rows and no nearer in
//! columns on the same side, so this order is far to near for a height
//! field. Cracks: a ring's outer border meets the next ring's cells,
//! which are twice as wide; the border's in-between corners take the
//! mean of their neighbours along the border, so both sides have the
//! same edge. Corners are worked out once a picture in two row caches.
//!
//! Every triangle is flat: 4 light steps from its turn to the sun, 3
//! haze steps by its farthest corner, its material from its slope and
//! height (`site`), and on near cells a checker of two light steps.

use super::palette;
use super::site::{self, Site};
use super::tune::{
    FAR_FROM, FAR_R, HAZE_FROM, HAZE_FULL, HAZE_LOOK_REACH, MESA_LIFT, MID_R, MID_R_HIGH, NEAR_R,
    SHAPE, STEEP_COS,
};
use crate::fixed::{V3, sub, wide};
use crate::poly::{Camera, MAX_POLY, Stats, View, fill_tri, plane_dist};
use crate::world::M;
use game_kit::Frame;

/// Corners in a cached row: the widest ring is 29 corners across
/// (mid HIGH: 448 m / 16 m + 1; far: 896 m / 32 m + 1).
const ROW: usize = 29;

#[derive(Clone, Copy)]
struct Corner {
    /// Camera space.
    c: [i32; 3],
    /// On screen, 1/16 pixel (when in front of the near plane).
    s: [i32; 2],
    /// Planes it is outside of.
    out: u8,
    /// Its height, and whether water covers it.
    y: i32,
    wet: bool,
    done: bool,
}

const NO_CORNER: Corner = Corner {
    c: [0; 3],
    s: [0; 2],
    out: 0,
    y: 0,
    wet: false,
    done: false,
};

/// One ring: its cell size, its square and the square it leaves out.
#[derive(Clone, Copy)]
struct Ring {
    cs: i32,
    x0: i32,
    z0: i32,
    n: i32,
    hole: Option<[i32; 4]>,
    /// The next ring out is drawn: fix this ring's border.
    fix: bool,
    near: bool,
}

/// Where the land's triangles go: the picture (the game), or a test's
/// recorder. One kind only in the game, so one copy of the walk.
pub trait Paint {
    /// Fills one triangle: corners in camera space and on screen (1/16
    /// pixel), its colour. Returns the pixels painted.
    fn tri(&mut self, c: &[[i32; 3]; 3], s: &[[i32; 2]; 3], colour: u8) -> u32;
}

/// The game's [`Paint`]: into the frame, inside `area`.
pub struct ToFrame<'a> {
    pub f: &'a mut Frame,
    pub area: crate::poly::Rect,
}

impl Paint for ToFrame<'_> {
    fn tri(&mut self, _c: &[[i32; 3]; 3], s: &[[i32; 2]; 3], colour: u8) -> u32 {
        fill_tri(self.f, &self.area, s, colour)
    }
}

/// What a picture of the land needs.
struct Walk<'a> {
    site: &'a Site,
    view: &'a View,
    cam: &'a Camera,
    planes: [[i32; 4]; 5],
    /// The view's own sides (no guard band) in world space, as (normal,
    /// constant), for dropping cells out of view.
    cull: [([i64; 3], i64); 5],
    sun: V3,
    haze_from: i32,
    haze_full: i32,
    high: i32,
    lo: i32,
    hi: i32,
    tag: [i32; 2],
    /// Step G2: cells wholly farther than this (squared, horizontally)
    /// are in the entry's fog and not drawn.
    fog2: i64,
    /// The two row caches (the caller's local, filled in place: built
    /// inside the struct they were copied, and `draw`'s frame passed the
    /// chip's 4 KB limit).
    rows: &'a mut [[Corner; ROW]; 2],
    tris: u32,
    pixels: u32,
}

fn snap(v: i32, to: i32) -> i32 {
    (v + to / 2).div_euclid(to) * to
}

impl Walk<'_> {
    /// The row caches for corner rows `a` and `b` (indices into the
    /// ring), keeping a row that is there already.
    fn slots(&mut self, a: i32, b: i32) -> (usize, usize) {
        let pa = self.tag.iter().position(|&t| t == a);
        let pb = self.tag.iter().position(|&t| t == b);
        let reset = |w: &mut Self, k: usize, t: i32| {
            w.tag[k] = t;
            for c in w.rows[k].iter_mut() {
                c.done = false;
            }
        };
        match (pa, pb) {
            (Some(x), Some(y)) => (x, y),
            (Some(x), None) => {
                reset(self, 1 - x, b);
                (x, 1 - x)
            }
            (None, Some(y)) => {
                reset(self, 1 - y, a);
                (1 - y, y)
            }
            (None, None) => {
                reset(self, 0, a);
                reset(self, 1, b);
                (0, 1)
            }
        }
    }

    /// The ground at (x, z) and whether water covers it.
    fn ground(&self, x: i32, z: i32) -> (i32, bool) {
        let h = site::height(self.site, x, z);
        (h.max(self.site.wet), h < self.site.wet)
    }

    /// Corner (i, j) of ring `r`, cached in row slot `k`.
    fn corner(&mut self, r: &Ring, k: usize, i: i32, j: i32) -> Corner {
        let cached = self.rows[k][i as usize];
        if cached.done {
            return cached;
        }
        let v = self.corner_at(r, i, j);
        self.rows[k][i as usize] = v;
        v
    }

    /// Corner (i, j) of ring `r`, worked out (no cache): the same value
    /// as the cache's, so shared edges meet exactly.
    fn corner_at(&self, r: &Ring, i: i32, j: i32) -> Corner {
        let (x, z) = (r.x0 + i * r.cs, r.z0 + j * r.cs);
        let edge_z = j == 0 || j == r.n;
        let edge_x = i == 0 || i == r.n;
        let (y, wet) = if r.fix && edge_z && i % 2 == 1 {
            let (a, b) = (self.ground(x - r.cs, z), self.ground(x + r.cs, z));
            ((a.0 + b.0) / 2, a.1 && b.1)
        } else if r.fix && edge_x && j % 2 == 1 {
            let (a, b) = (self.ground(x, z - r.cs), self.ground(x, z + r.cs));
            ((a.0 + b.0) / 2, a.1 && b.1)
        } else {
            self.ground(x, z)
        };
        let c = self.cam.rot.apply(sub([x, y, z], self.cam.pos));
        let mut out = 0u8;
        for (b, pl) in self.planes.iter().enumerate() {
            if plane_dist(pl, &c) < 0 {
                out |= 1 << b;
            }
        }
        let s = if out & 1 == 0 {
            self.view.project(c)
        } else {
            [0; 2]
        };
        Corner {
            c,
            s,
            out,
            y,
            wet,
            done: true,
        }
    }

    /// The crack fix's last part: an odd border corner (the middle one
    /// of `v`) lies on the next ring's edge in the world, but its place
    /// on screen is rounded to 1/16 pixel, so a pixel can fall between
    /// the two rings. The sliver between the three corners covers it.
    fn sliver(&mut self, f: &mut impl Paint, v: [Corner; 3], colour: u8) {
        if v[0].out | v[1].out | v[2].out != 0 {
            return;
        }
        let (a, b, c) = (v[0].s, v[1].s, v[2].s);
        if wide(b[0] - a[0], c[1] - a[1]) == wide(c[0] - a[0], b[1] - a[1]) {
            return;
        }
        self.pixels += f.tri(&[v[0].c, v[1].c, v[2].c], &[a, b, c], colour);
        self.tris += 1;
    }

    /// Whether any of cell (x0..x1, z0..z1)'s box (with the site's
    /// height bounds) may be in view.
    fn cell_seen(&self, x0: i32, x1: i32, z0: i32, z1: i32) -> bool {
        self.cull.iter().all(|(n, e)| {
            let p = [
                if n[0] >= 0 { x1 } else { x0 },
                if n[1] >= 0 { self.hi } else { self.lo },
                if n[2] >= 0 { z1 } else { z0 },
            ];
            n[0] * i64::from(p[0]) + n[1] * i64::from(p[1]) + n[2] * i64::from(p[2]) + e >= 0
        })
    }

    /// One triangle: its colour, then filled (cut by the planes when a
    /// corner is outside one). `p` are the corners' places (x, z).
    fn tri(
        &mut self,
        f: &mut impl Paint,
        v: [Corner; 3],
        p: [[i32; 2]; 3],
        checker: bool,
    ) -> Option<u8> {
        if v[0].out & v[1].out & v[2].out != 0 {
            return None;
        }
        let w = |k: usize| [p[k][0], v[k].y, p[k][1]];
        let (a, b, c) = (w(0), w(1), w(2));
        // n = (c - a) x (b - a): up for both of a cell's triangles.
        // Parts at most 32 m and 64 m high: products below 2^27.
        let (e1, e2) = (sub(c, a), sub(b, a));
        let mut n = [
            wide(e1[1], e2[2]) - wide(e1[2], e2[1]),
            wide(e1[2], e2[0]) - wide(e1[0], e2[2]),
            wide(e1[0], e2[1]) - wide(e1[1], e2[0]),
        ];
        // Made below 2^15 a part.
        let big = n.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
        let shift = (64 - big.leading_zeros()).saturating_sub(15);
        for v in n.iter_mut() {
            *v >>= shift;
        }
        // Turned away from the eye: hidden by the land in front.
        let eye = sub(a, self.cam.pos);
        if n[0] * i64::from(eye[0]) + n[1] * i64::from(eye[1]) + n[2] * i64::from(eye[2]) >= 0 {
            return None;
        }
        let nn = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
        let d = (n[0] * i64::from(self.sun[0])
            + n[1] * i64::from(self.sun[1])
            + n[2] * i64::from(self.sun[2]))
            >> 14;
        // cos above 3/4, 1/2, 1/4 (in squares).
        let light: u8 = if d <= 0 {
            3
        } else if 16 * d * d > 9 * nn {
            0
        } else if 4 * d * d > nn {
            1
        } else if 16 * d * d > nn {
            2
        } else {
            3
        };
        let far = v[0].c[2].max(v[1].c[2]).max(v[2].c[2]);
        let haze = if far >= self.haze_full {
            2
        } else if far >= self.haze_from {
            1
        } else {
            0
        };
        let colour = if v[0].wet && v[1].wet && v[2].wet {
            palette::water(haze, light)
        } else {
            let steep = n[1] * n[1] * (1 << 28) < i64::from(STEEP_COS) * i64::from(STEEP_COS) * nn;
            let m = if steep {
                1
            } else if (a[1] + b[1] + c[1]) / 3 > self.high {
                2
            } else {
                0
            };
            // The checker: two neighbouring light steps.
            palette::land(m, haze, if checker { light ^ 1 } else { light })
        };
        if v[0].out | v[1].out | v[2].out == 0 {
            self.pixels += f.tri(&[v[0].c, v[1].c, v[2].c], &[v[0].s, v[1].s, v[2].s], colour);
            self.tris += 1;
            return Some(colour);
        }
        let mut poly = [[0i32; 3]; MAX_POLY];
        poly[0] = v[0].c;
        poly[1] = v[1].c;
        poly[2] = v[2].c;
        let k = clip(&mut poly, &self.planes, v[0].out | v[1].out | v[2].out);
        if k < 3 {
            return None;
        }
        let s0 = self.view.project(poly[0]);
        let mut prev = self.view.project(poly[1]);
        for q in 2..k {
            let s = self.view.project(poly[q]);
            self.pixels += f.tri(&[poly[0], poly[q - 1], poly[q]], &[s0, prev, s], colour);
            self.tris += 1;
            prev = s;
        }
        Some(colour)
    }

    /// Cell (i, j) of ring `r`, rows in slots (k0: row j, k1: row j + 1).
    fn cell(&mut self, f: &mut impl Paint, r: &Ring, i: i32, j: i32, k0: usize, k1: usize) {
        let (x0, z0) = (r.x0 + i * r.cs, r.z0 + j * r.cs);
        let (x1, z1) = (x0 + r.cs, z0 + r.cs);
        if let Some(h) = r.hole
            && x0 >= h[0]
            && x1 <= h[1]
            && z0 >= h[2]
            && z1 <= h[3]
        {
            return;
        }
        if !self.cell_seen(x0, x1, z0, z1) {
            return;
        }
        if self.fog2 < i64::MAX {
            let (cx, cz) = (self.cam.pos[0], self.cam.pos[2]);
            let dx = i64::from((x0 - cx).max(cx - x1).max(0));
            let dz = i64::from((z0 - cz).max(cz - z1).max(0));
            if dx * dx + dz * dz > self.fog2 {
                return;
            }
        }
        let sw = self.corner(r, k0, i, j);
        let se = self.corner(r, k0, i + 1, j);
        let nw = self.corner(r, k1, i, j + 1);
        let ne = self.corner(r, k1, i + 1, j + 1);
        let checker = r.near && (i + j) % 2 == 1;
        let t_se = ([sw, se, ne], [[x0, z0], [x1, z0], [x1, z1]]);
        let t_nw = ([sw, ne, nw], [[x0, z0], [x1, z1], [x0, z1]]);
        // The camera east of the SW-NE diagonal: the NW triangle is
        // across it, farther: drawn first.
        let east = (self.cam.pos[0] - x0) - (self.cam.pos[2] - z0) > 0;
        let (first, second) = if east { (t_nw, t_se) } else { (t_se, t_nw) };
        let a = self.tri(f, first.0, first.1, checker);
        let b = self.tri(f, second.0, second.1, checker);
        // Slivers along the border, one per odd corner (from the cell
        // that starts at it).
        if let (true, Some(col)) = (r.fix, b.or(a)) {
            let n = r.n;
            if i % 2 == 1 {
                for jj in [0, n] {
                    if j == jj || j + 1 == jj {
                        let v = [-1, 0, 1].map(|d| self.corner_at(r, i + d, jj));
                        self.sliver(f, v, col);
                    }
                }
            }
            if j % 2 == 1 {
                for ii in [0, n] {
                    if i == ii || i + 1 == ii {
                        let v = [-1, 0, 1].map(|d| self.corner_at(r, ii, j + d));
                        self.sliver(f, v, col);
                    }
                }
            }
        }
    }

    /// The columns of row `j`, far to near.
    fn row(&mut self, f: &mut impl Paint, r: &Ring, j: i32, ic: i32) {
        let (k0, k1) = self.slots(j, j + 1);
        let ic = ic.clamp(-1, r.n);
        let mut i = r.n - 1;
        while i > ic {
            self.cell(f, r, i, j, k0, k1);
            i -= 1;
        }
        for i in 0..ic.min(r.n) {
            self.cell(f, r, i, j, k0, k1);
        }
        if (0..r.n).contains(&ic) {
            self.cell(f, r, ic, j, k0, k1);
        }
    }

    /// One ring, far rows first.
    fn ring(&mut self, f: &mut impl Paint, r: &Ring) {
        self.tag = [i32::MIN; 2];
        let ic = (self.cam.pos[0] - r.x0).div_euclid(r.cs);
        let jc = (self.cam.pos[2] - r.z0).div_euclid(r.cs).clamp(-1, r.n);
        let mut j = r.n - 1;
        while j > jc {
            self.row(f, r, j, ic);
            j -= 1;
        }
        for j in 0..jc.min(r.n) {
            self.row(f, r, j, ic);
        }
        if (0..r.n).contains(&jc) {
            self.row(f, r, jc, ic);
        }
    }
}

/// Where edge a-b crosses a plane (`da`, `db`: the ends' distances, of
/// different signs), worked out from the lower end (by its parts) so
/// that the two triangles on an edge get the very same point: no pixel
/// falls between them where the view's planes cut the land.
fn cross_at(a: [i32; 3], da: i64, b: [i32; 3], db: i64) -> [i32; 3] {
    let (a, da, b, db) = if a <= b {
        (a, da, b, db)
    } else {
        (b, db, a, da)
    };
    // As `poly::clip_polygon`: |da| at most |da - db|, below 2^32; the
    // parts' differences below 2^23: products below 2^55.
    let den = da.wrapping_sub(db);
    [0, 1, 2].map(|j| {
        let d = i64::from(b[j].wrapping_sub(a[j]));
        a[j].wrapping_add((d.wrapping_mul(da) / den) as i32)
    })
}

/// A triangle (in `poly`) cut by the planes whose bits are set in
/// `which`, one after the other. Returns the corners left (0 when
/// fewer than 3).
fn clip(poly: &mut [[i32; 3]; MAX_POLY], planes: &[[i32; 4]; 5], which: u8) -> usize {
    let mut n = 3;
    let mut tmp = [[0i32; 3]; MAX_POLY];
    for (k, pl) in planes.iter().enumerate() {
        if which & (1 << k) == 0 || n == 0 {
            continue;
        }
        let mut m = 0;
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let (da, db) = (plane_dist(pl, &a), plane_dist(pl, &b));
            if da >= 0 && m < MAX_POLY {
                tmp[m] = a;
                m += 1;
            }
            if (da >= 0) != (db >= 0) && m < MAX_POLY {
                tmp[m] = cross_at(a, da, b, db);
                m += 1;
            }
        }
        poly[..m].copy_from_slice(&tmp[..m]);
        n = m;
    }
    if n < 3 { 0 } else { n }
}

/// The square ring of cell size `cs` and half-width `ext` round the
/// camera, centred on `snap_to`, leaving out `hole`.
fn square(cam: V3, cs: i32, ext: i32, snap_to: i32) -> (i32, i32, i32) {
    let (cx, cz) = (snap(cam[0], snap_to), snap(cam[2], snap_to));
    (cx - ext, cz - ext, 2 * ext / cs)
}

/// Draws the land into `view` for detail level `lv` (0 LOW, 1 MID, 2
/// HIGH). `sun` points to the sun (16384 long, the site's axes). Step
/// G2: `fog` (1/256 of the reach; 256 or more: none) is how far the
/// entry's fog lets the eye see. The triangles and pixels are added to
/// `stats`. Returns the reach (the far edge drawn).
#[allow(clippy::too_many_arguments)]
#[inline(never)]
pub fn draw(
    f: &mut impl Paint,
    view: &View,
    cam: &Camera,
    s: &Site,
    lv: usize,
    sun: V3,
    fog: i32,
    stats: &mut Stats,
) -> i32 {
    let lv = lv.min(2);
    let over = cam.pos[1] - site::surface(s, cam.pos[0], cam.pos[2]);
    let far_on = over >= FAR_FROM;
    let reach = if far_on { FAR_R[lv] } else { MID_R[lv] };
    let reach = if s.look == 6 {
        reach * HAZE_LOOK_REACH / 100
    } else {
        reach
    };
    // The fog: the haze closes in with it.
    let (fog2, seen) = if fog >= 256 {
        (i64::MAX, reach)
    } else {
        let d = reach * fog.max(0) / 256;
        (i64::from(d) * i64::from(d), d.max(M))
    };
    let sh = &SHAPE[usize::from(s.look % 8)];
    let sum = sh.amp[0] + sh.amp[1] + sh.amp[2];
    // The view's sides without the guard band, a little wider (4 px),
    // and the near plane, turned into world space.
    let fo = view.focal;
    let (gx, gy) = (view.area.w() / 2 + 4, view.area.h() / 2 + 4);
    let sides = [
        [0, 0, 1],
        [fo, 0, gx],
        [-fo, 0, gx],
        [0, -fo, gy],
        [0, fo, gy],
    ];
    let mut cull = [([0i64; 3], 0i64); 5];
    let r = &cam.rot.0;
    for (k, p) in sides.iter().enumerate() {
        // n_world = R^T p (in 1/16384); the plane holds at the eye.
        let n = [0, 1, 2].map(|c| {
            i64::from(r[0][c]) * i64::from(p[0])
                + i64::from(r[1][c]) * i64::from(p[1])
                + i64::from(r[2][c]) * i64::from(p[2])
        });
        let e = -(n[0] * i64::from(cam.pos[0])
            + n[1] * i64::from(cam.pos[1])
            + n[2] * i64::from(cam.pos[2]));
        cull[k] = (n, e);
    }
    let mut rows = [[NO_CORNER; ROW]; 2];
    let mut w = Walk {
        site: s,
        view,
        cam,
        planes: view.planes(),
        cull,
        sun,
        haze_from: seen * HAZE_FROM / 100,
        haze_full: seen * HAZE_FULL / 100,
        high: (sh.amp[0] + sh.amp[1]) * 3 / 10,
        lo: -sum - 2 * M,
        hi: sum + MESA_LIFT + 8 * M,
        tag: [i32::MIN; 2],
        fog2,
        rows: &mut rows,
        tris: 0,
        pixels: 0,
    };
    let near_sq = square(cam.pos, 8 * M, NEAR_R[lv], 16 * M);
    let mid_r = if far_on { MID_R_HIGH[lv] } else { MID_R[lv] };
    let mid_sq = square(cam.pos, 16 * M, mid_r, 32 * M);
    let hole_of = |q: (i32, i32, i32), cs: i32| Some([q.0, q.0 + q.2 * cs, q.1, q.1 + q.2 * cs]);
    if far_on {
        let far_sq = square(cam.pos, 32 * M, FAR_R[lv], 32 * M);
        let ring = Ring {
            cs: 32 * M,
            x0: far_sq.0,
            z0: far_sq.1,
            n: far_sq.2,
            hole: hole_of(mid_sq, 16 * M),
            fix: false,
            near: false,
        };
        w.ring(f, &ring);
    }
    let ring = Ring {
        cs: 16 * M,
        x0: mid_sq.0,
        z0: mid_sq.1,
        n: mid_sq.2,
        hole: hole_of(near_sq, 8 * M),
        fix: far_on,
        near: false,
    };
    w.ring(f, &ring);
    let ring = Ring {
        cs: 8 * M,
        x0: near_sq.0,
        z0: near_sq.1,
        n: near_sq.2,
        hole: None,
        fix: true,
        near: true,
    };
    w.ring(f, &ring);
    stats.tris += w.tris;
    stats.pixels += w.pixels;
    reach
}
