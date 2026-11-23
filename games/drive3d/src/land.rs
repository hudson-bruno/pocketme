//! The land block: 32 by 48 cells of 8 m, heights on 33 by 49 corners
//! in 1/4 m steps, and one byte per cell (the surfaces of its two
//! triangles and which diagonal splits it). Baked from the road
//! records while compiling (`bake` is a `const fn`), so the land is
//! constants and costs no state.
//!
//! The rules read the land only through [`Ground`]: the height of the
//! plane of the triangle under a point (the triangle that is drawn,
//! so the car never sinks into what is seen) and its surface.

use crate::road::{POINTS, Road, flag, walk};
use crate::tune::{
    CELL, CELLS_X, CELLS_Z, CORNERS_X, CORNERS_Z, CUT_WIDTH, FLAT, FORD, FORD_BAND, GRASS,
    HEIGHT_STEP, LOW_GRASS, PATTERN_FROM, RISE_PER,
};
use game_3d::{cos_fine, isqrt, sin_fine};

/// What lies under a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    /// The land's height (subs).
    pub height: i32,
    /// Its surface (`tune::GRAVEL`, `GRASS`, `FORD`, `LOW_GRASS`).
    pub surface: u8,
}

/// What the rules need of the land (the stretch, or a test ground).
pub trait Ground {
    /// The height and surface under (x, z), subs. Outside the land the
    /// nearest edge's.
    fn at(&self, x: i32, z: i32) -> Spot;
    /// The land's size (x, z) in subs; the car keeps away from its
    /// edges (`tune::EDGE`).
    fn size(&self) -> (i32, i32);
}

/// The baked land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Land {
    /// Corner heights in 1/4 m, row by row (z), `CORNERS_X` a row.
    pub h: [u8; CORNERS_X * CORNERS_Z],
    /// Cell bytes, row by row (z), `CELLS_X` a row: bits 0 to 2 the
    /// surface of triangle 0, bits 3 to 5 of triangle 1, bit 6 the
    /// diagonal (see [`tri_of`]).
    pub cell: [u8; CELLS_X * CELLS_Z],
}

/// Bit 6 of a cell byte: the cell is split from corner (1,0) to (0,1)
/// instead of (0,0) to (1,1).
pub const DIAG_BIT: u8 = 1 << 6;

/// The stretch's land, baked while compiling.
pub static LAND: Land = bake();

/// The stretch's land as a [`Ground`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Stretch;

impl Ground for Stretch {
    fn at(&self, x: i32, z: i32) -> Spot {
        LAND.at(x, z)
    }
    fn size(&self) -> (i32, i32) {
        (CELLS_X as i32 * CELL, CELLS_Z as i32 * CELL)
    }
}

/// Which triangle of a cell holds the point (fx, fz) inside it (subs
/// from the cell's corner (0,0), each 0 to `CELL - 1`):
///
/// * diagonal (0,0)-(1,1): triangle 0 is (0,0) (1,0) (1,1), where
///   `fx >= fz`; triangle 1 is (0,0) (1,1) (0,1);
/// * diagonal (1,0)-(0,1): triangle 0 is (0,0) (1,0) (0,1), where
///   `fx + fz < CELL`; triangle 1 is (1,0) (1,1) (0,1).
pub const fn tri_of(diag: bool, fx: i32, fz: i32) -> usize {
    if !diag {
        if fx >= fz { 0 } else { 1 }
    } else if fx + fz < CELL {
        0
    } else {
        1
    }
}

/// The corners (as (dx, dz) of the cell, 0 or 1) of a cell's triangle,
/// in the order of [`tri_of`].
pub const fn tri_corners(diag: bool, tri: usize) -> [(usize, usize); 3] {
    match (diag, tri) {
        (false, 0) => [(0, 0), (1, 0), (1, 1)],
        (false, _) => [(0, 0), (1, 1), (0, 1)],
        (true, 0) => [(0, 0), (1, 0), (0, 1)],
        (true, _) => [(1, 0), (1, 1), (0, 1)],
    }
}

impl Land {
    /// The corner height (1/4 m) at corner (i, j).
    pub fn corner(&self, i: usize, j: usize) -> u8 {
        self.h[j * CORNERS_X + i]
    }

    /// The cell byte of cell (i, j).
    pub fn cell_byte(&self, i: usize, j: usize) -> u8 {
        self.cell[j * CELLS_X + i]
    }

    /// The surface of triangle `tri` of cell (i, j).
    pub fn surface(&self, i: usize, j: usize, tri: usize) -> u8 {
        (self.cell_byte(i, j) >> (3 * tri)) & 7
    }

    /// The height and surface under (x, z), subs, from the plane of the
    /// triangle under it. Outside the block, the nearest edge's.
    pub fn at(&self, x: i32, z: i32) -> Spot {
        let max_x = CELLS_X as i32 * CELL - 1;
        let max_z = CELLS_Z as i32 * CELL - 1;
        let (x, z) = (x.clamp(0, max_x), z.clamp(0, max_z));
        let (i, j) = ((x / CELL) as usize, (z / CELL) as usize);
        let (fx, fz) = (x % CELL, z % CELL);
        let b = self.cell_byte(i, j);
        let diag = b & DIAG_BIT != 0;
        let tri = tri_of(diag, fx, fz);
        let q = |di: usize, dj: usize| i64::from(self.corner(i + di, j + dj));
        let (fx, fz) = (i64::from(fx), i64::from(fz));
        let c = i64::from(CELL);
        // Heights in 1/4 m, positions in subs: the plane's height in
        // subs is base * 1024 + (a * fx + b * fz) / 32 (CELL = 32 * 1024).
        let (base, num) = match (diag, tri) {
            (false, 0) => (q(0, 0), (q(1, 0) - q(0, 0)) * fx + (q(1, 1) - q(1, 0)) * fz),
            (false, _) => (q(0, 0), (q(1, 1) - q(0, 1)) * fx + (q(0, 1) - q(0, 0)) * fz),
            (true, 0) => (q(0, 0), (q(1, 0) - q(0, 0)) * fx + (q(0, 1) - q(0, 0)) * fz),
            (true, _) => (
                q(1, 1),
                (q(0, 1) - q(1, 1)) * (c - fx) + (q(1, 0) - q(1, 1)) * (c - fz),
            ),
        };
        let height = base * i64::from(HEIGHT_STEP) + num.div_euclid(c / i64::from(HEIGHT_STEP));
        Spot {
            height: height as i32,
            surface: (b >> (3 * tri)) & 7,
        }
    }

    /// The highest and lowest corner of cell (i, j), 1/4 m.
    pub fn cell_range(&self, i: usize, j: usize) -> (u8, u8) {
        let c = [
            self.corner(i, j),
            self.corner(i + 1, j),
            self.corner(i, j + 1),
            self.corner(i + 1, j + 1),
        ];
        let (mut lo, mut hi) = (c[0], c[0]);
        let mut k = 1;
        while k < 4 {
            if c[k] < lo {
                lo = c[k];
            }
            if c[k] > hi {
                hi = c[k];
            }
            k += 1;
        }
        (lo, hi)
    }
}

/// The road point nearest to (x, z): first every 4th point, then the
/// 4 each side of the best. Returns (index, squared distance).
///
/// The speed work: for |x|, |z| <= [`FAST_REACH`] (2^29) the
/// differences are taken in 32 bits (|x - road.x[k]| <= 2^30, as every
/// road point lies within the same reach, checked while compiling) and
/// squared in 64 bits from them; each square is below 2^60 and the sum
/// below 2^61, so no sum can run over and every distance is the one
/// [`nearest_point_slow`] works out with 64-bit differences. The scan,
/// the strict `<` (the first of two equal points wins) and the result
/// are the same. Points farther out take [`nearest_point_slow`].
/// `tests/speed_proofs.rs` checks the two against each other.
pub const fn nearest_point(road: &Road, x: i32, z: i32) -> (usize, i64) {
    if x < -FAST_REACH || x > FAST_REACH || z < -FAST_REACH || z > FAST_REACH {
        return nearest_point_slow(road, x, z);
    }
    let mut best = 0usize;
    let mut best_d = i64::MAX;
    let mut k = 0usize;
    while k < POINTS {
        let d = d2_fast(road, k, x, z);
        if d < best_d {
            best_d = d;
            best = k;
        }
        k += 4;
    }
    let lo = best.saturating_sub(4);
    let hi = if best + 4 < POINTS {
        best + 4
    } else {
        POINTS - 1
    };
    let mut k = lo;
    while k <= hi {
        let d = d2_fast(road, k, x, z);
        if d < best_d {
            best_d = d;
            best = k;
        }
        k += 1;
    }
    (best, best_d)
}

/// [`nearest_point`] for any (x, z), with 64-bit differences: the same
/// scan as there, kept for points out of its fast range (and for the
/// tests).
pub const fn nearest_point_slow(road: &Road, x: i32, z: i32) -> (usize, i64) {
    let mut best = 0usize;
    let mut best_d = i64::MAX;
    let mut k = 0usize;
    while k < POINTS {
        let d = d2(road, k, x, z);
        if d < best_d {
            best_d = d;
            best = k;
        }
        k += 4;
    }
    let lo = best.saturating_sub(4);
    let hi = if best + 4 < POINTS {
        best + 4
    } else {
        POINTS - 1
    };
    let mut k = lo;
    while k <= hi {
        let d = d2(road, k, x, z);
        if d < best_d {
            best_d = d;
            best = k;
        }
        k += 1;
    }
    (best, best_d)
}

const fn d2(road: &Road, k: usize, x: i32, z: i32) -> i64 {
    let dx = x as i64 - road.x[k] as i64;
    let dz = z as i64 - road.z[k] as i64;
    dx * dx + dz * dz
}

/// How far from the origin (subs, each way) [`nearest_point`] works in
/// 32-bit differences.
pub const FAST_REACH: i32 = 1 << 29;

const _: () = {
    // Every road point lies within the fast reach, so a difference of
    // two values within it fits in 32 bits. (`walk` makes the only
    // road there is: `ROAD`, `THINGS` and the land's bake all use it.)
    let road = walk();
    let mut k = 0;
    while k < POINTS {
        assert!(road.x[k] >= -FAST_REACH && road.x[k] <= FAST_REACH);
        assert!(road.z[k] >= -FAST_REACH && road.z[k] <= FAST_REACH);
        k += 1;
    }
};

/// [`d2`] for x, z and the road point all within [`FAST_REACH`]: the
/// same number (see [`nearest_point`]). The 32-bit differences lie
/// within 2^30 there, so they cannot wrap and need no check.
#[inline(always)]
const fn d2_fast(road: &Road, k: usize, x: i32, z: i32) -> i64 {
    let dx = x.wrapping_sub(road.x[k]) as i64;
    let dz = z.wrapping_sub(road.z[k]) as i64;
    (dx * dx).wrapping_add(dz * dz)
}

/// The signed distance (subs, plus to the right of the road's way) of
/// (x, z) from the road's line through point `k`.
pub const fn side_of(road: &Road, k: usize, x: i32, z: i32) -> i32 {
    let dx = x as i64 - road.x[k] as i64;
    let dz = z as i64 - road.z[k] as i64;
    let a = road.heading[k];
    ((dx * cos_fine(a) as i64 - dz * sin_fine(a) as i64) / 16_384) as i32
}

/// The distance (subs) of (x, z) along the road's way at point `k`.
pub const fn along_of(road: &Road, k: usize, x: i32, z: i32) -> i32 {
    let dx = x as i64 - road.x[k] as i64;
    let dz = z as i64 - road.z[k] as i64;
    let a = road.heading[k];
    ((dx * sin_fine(a) as i64 + dz * cos_fine(a) as i64) / 16_384) as i32
}

/// A small fixed pattern of 0, 1 or 2 height steps for corner (i, j).
const fn pattern(i: usize, j: usize) -> i64 {
    ((i * 7 + j * 13 + (i * j) % 5) % 3) as i64
}

/// The middle road point of the ford piece (the first `FORD` piece).
const fn ford_middle() -> usize {
    let mut first = 0usize;
    let mut p = 0usize;
    while p < crate::road::PIECES.len() {
        let pc = crate::road::PIECES[p];
        if pc.surface == FORD {
            return first + pc.len as usize / 2;
        }
        first += pc.len as usize;
        p += 1;
    }
    0
}

/// Road points before and after the cut corner whose inside is still
/// low grass (20 m). DEFAULT (review F4).
const CUT_REACH: usize = 10;

/// Turns the road records into the land (a `const fn`; tests also run
/// it as plain code and compare the bytes):
///
/// 1. every corner's height is the height of the nearest road point,
///    plus the shoulder: beyond `FLAT` from the road the land rises
///    1 m for every 8 m out, and beyond `PATTERN_FROM` a fixed pattern
///    of 0 to 0.5 m is added; on the inside of a `CUT` corner the land
///    stays at the road's height up to `CUT_WIDTH` out;
/// 2. a cell's diagonal runs the road's way at the cell's corner
///    nearest the road (both or neither of x and z growing);
/// 3. a triangle is road when its middle is within half the road's
///    width of the road's line (the mean of its corners' distances),
///    else grass, or low grass on the inside of the `CUT` corner within
///    `CUT_WIDTH` of the road's line (from `CUT_REACH` road points
///    before the corner to as many after it, so the chord across the
///    corner is low grass all along); the ford's band (`FORD_BAND` wide, across the whole
///    block) is water; last, every triangle the road's line crosses
///    (looked at every 1/8 m) is road, so the line is road all along.
pub const fn bake() -> Land {
    let road = walk();
    let mut land = Land {
        h: [0; CORNERS_X * CORNERS_Z],
        cell: [0; CELLS_X * CELLS_Z],
    };
    // Per corner: signed distance from the road and nearest point.
    let mut side = [0i32; CORNERS_X * CORNERS_Z];
    let mut near = [0u16; CORNERS_X * CORNERS_Z];
    let mut j = 0usize;
    while j < CORNERS_Z {
        let mut i = 0usize;
        while i < CORNERS_X {
            let (x, z) = (i as i32 * CELL, j as i32 * CELL);
            let (k, d2) = nearest_point(&road, x, z);
            let dist = isqrt(d2 as u64) as i64;
            let s = side_of(&road, k, x, z);
            let pc = crate::road::PIECES[road.piece[k] as usize];
            let cut = pc.flags & flag::CUT != 0
                && (s as i64) * (pc.turn as i64) > 0
                && dist < CUT_WIDTH as i64;
            let mut h = road.y[k] as i64;
            if !cut {
                if dist > FLAT as i64 {
                    h += (dist - FLAT as i64) / RISE_PER as i64;
                }
                if dist > PATTERN_FROM as i64 {
                    h += pattern(i, j) * HEIGHT_STEP as i64;
                }
            }
            let mut q = (h + HEIGHT_STEP as i64 / 2).div_euclid(HEIGHT_STEP as i64);
            if q < 0 {
                q = 0;
            }
            if q > 255 {
                q = 255;
            }
            land.h[j * CORNERS_X + i] = q as u8;
            side[j * CORNERS_X + i] = s;
            near[j * CORNERS_X + i] = k as u16;
            i += 1;
        }
        j += 1;
    }
    let ford = ford_middle();
    // The cut corner's road points, and the way it turns.
    let (mut cut_lo, mut cut_hi, mut cut_turn) = (usize::MAX, 0usize, 0i64);
    let mut k = 0usize;
    while k < POINTS {
        let pc = crate::road::PIECES[road.piece[k] as usize];
        if pc.flags & flag::CUT != 0 {
            if k < cut_lo {
                cut_lo = k;
            }
            cut_hi = k;
            cut_turn = pc.turn as i64;
        }
        k += 1;
    }
    let mut j = 0usize;
    while j < CELLS_Z {
        let mut i = 0usize;
        while i < CELLS_X {
            // The corner of the cell nearest the road picks the diagonal.
            let mut best = j * CORNERS_X + i;
            let mut c = 0usize;
            while c < 4 {
                let at = (j + c / 2) * CORNERS_X + i + c % 2;
                if side[at].unsigned_abs() < side[best].unsigned_abs() {
                    best = at;
                }
                c += 1;
            }
            let a = road.heading[near[best] as usize];
            let (sx, cz) = (sin_fine(a), cos_fine(a));
            let diag = (sx < 0) != (cz < 0) && sx != 0 && cz != 0;
            let mut byte = if diag { DIAG_BIT } else { 0 };
            let mut tri = 0usize;
            while tri < 2 {
                let cs = tri_corners(diag, tri);
                let mut sum_side = 0i64;
                let mut sum_x = 0i64;
                let mut sum_z = 0i64;
                let mut nearest_corner = (j + cs[0].1) * CORNERS_X + i + cs[0].0;
                let mut c = 0usize;
                while c < 3 {
                    let at = (j + cs[c].1) * CORNERS_X + i + cs[c].0;
                    sum_side += side[at] as i64;
                    sum_x += ((i + cs[c].0) as i64) * CELL as i64;
                    sum_z += ((j + cs[c].1) as i64) * CELL as i64;
                    if side[at].unsigned_abs() < side[nearest_corner].unsigned_abs() {
                        nearest_corner = at;
                    }
                    c += 1;
                }
                let k = near[nearest_corner] as usize;
                let pc = crate::road::PIECES[road.piece[k] as usize];
                let half = pc.width as i64 * HEIGHT_STEP as i64 / 2;
                let mid_side = sum_side / 3;
                let cut = k + CUT_REACH >= cut_lo
                    && k <= cut_hi + CUT_REACH
                    && mid_side * cut_turn > 0
                    && mid_side.abs() < CUT_WIDTH as i64;
                let mut s = if mid_side.abs() <= half {
                    pc.surface
                } else if cut {
                    LOW_GRASS
                } else {
                    GRASS
                };
                let along = along_of(&road, ford, (sum_x / 3) as i32, (sum_z / 3) as i32);
                if (along as i64).abs() * 2 <= FORD_BAND as i64 {
                    s = FORD;
                }
                byte |= s << (3 * tri);
                tri += 1;
            }
            land.cell[j * CELLS_X + i] = byte;
            i += 1;
        }
        j += 1;
    }
    // The road's line: every triangle it crosses is road.
    let mut k = 0usize;
    while k + 1 < POINTS {
        let s = crate::road::PIECES[road.piece[k + 1] as usize].surface;
        let mut t = 0i64;
        while t < 16 {
            let x = road.x[k] as i64 + (road.x[k + 1] - road.x[k]) as i64 * t / 16;
            let z = road.z[k] as i64 + (road.z[k + 1] - road.z[k]) as i64 * t / 16;
            mark(&mut land, x as i32, z as i32, s);
            t += 1;
        }
        k += 1;
    }
    let last = POINTS - 1;
    let s = crate::road::PIECES[road.piece[last] as usize].surface;
    mark(&mut land, road.x[last], road.z[last], s);
    land
}

/// Sets the surface of the triangle under (x, z).
const fn mark(land: &mut Land, x: i32, z: i32, s: u8) {
    let (i, j) = ((x / CELL) as usize, (z / CELL) as usize);
    if i >= CELLS_X || j >= CELLS_Z || x < 0 || z < 0 {
        return;
    }
    let at = j * CELLS_X + i;
    let b = land.cell[at];
    let tri = tri_of(b & DIAG_BIT != 0, x % CELL, z % CELL);
    let shift = 3 * tri as u32;
    land.cell[at] = (b & !(7 << shift)) | (s << shift);
}
