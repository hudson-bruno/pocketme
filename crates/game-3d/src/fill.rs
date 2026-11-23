//! Rectangles and the triangle filler.

use game_kit::{Frame, H, W};

use crate::sums::{Checked, Sums};
use crate::{GUARD, SUB};

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
fn first_row<S: Sums>(y: i32) -> i32 {
    S::add(
        S::sub(y, SUB / 2).div_euclid(SUB),
        i32::from(S::sub(y, SUB / 2).rem_euclid(SUB) != 0),
    )
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
    fn new<S: Sums>(a: [i32; 2], b: [i32; 2], row: i32) -> Edge {
        let dx = S::sub(b[0], a[0]);
        let dy = S::sub(b[1], a[1]);
        let d = S::mul(SUB, dy);
        let n = S::sub(
            S::add(
                S::mul(a[0], dy),
                S::mul(S::sub(S::add(S::mul(SUB, row), SUB / 2), a[1]), dx),
            ),
            S::mul(SUB / 2, dy),
        );
        let s = S::mul(SUB, dx);
        let (q, r) = S::floor_div(n, d);
        let (sq, sr) = S::floor_div(s, d);
        Edge { q, r, d, sq, sr }
    }

    /// The first pixel at or right of the edge on this row.
    fn x<S: Sums>(&self) -> i32 {
        S::add(self.q, i32::from(self.r != 0))
    }

    fn step<S: Sums>(&mut self) {
        self.q = S::add(self.q, self.sq);
        self.r = S::add(self.r, self.sr);
        if self.r >= self.d {
            self.r = S::sub(self.r, self.d);
            self.q = S::add(self.q, 1);
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
#[inline(always)]
pub fn fill_tri(f: &mut Frame, clip: &Rect, p: &[[i32; 2]; 3], c: u8) -> u32 {
    fill_tri_with::<Checked>(f, clip, p, c)
}

/// [`fill_tri`] with the sums `S`.
pub fn fill_tri_with<S: Sums>(f: &mut Frame, clip: &Rect, p: &[[i32; 2]; 3], c: u8) -> u32 {
    fill_body::<S, false>(f, clip, p, c).pixels
}

/// What the filler did for one triangle: pixels painted and pixel rows
/// visited (a row is counted even when the triangle paints nothing on
/// it, as happens on a row cut off by the clip rectangle's sides).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filled {
    pub pixels: u32,
    pub rows: u32,
}

/// [`fill_tri_with`] that also counts the rows it visits (CAMBER's cost
/// tests; worked out once per triangle, not per row). The same pixels.
pub fn fill_tri_counted<S: Sums>(f: &mut Frame, clip: &Rect, p: &[[i32; 2]; 3], c: u8) -> Filled {
    fill_body::<S, true>(f, clip, p, c)
}

/// The filler itself, with or without the row count (`ROWS`; without
/// it the count is not built). The row loop is the same either way.
#[inline(always)]
fn fill_body<S: Sums, const ROWS: bool>(
    f: &mut Frame,
    clip: &Rect,
    p: &[[i32; 2]; 3],
    c: u8,
) -> Filled {
    let none = Filled::default();
    if p.iter()
        .any(|v| v[0].unsigned_abs() > GUARD as u32 || v[1].unsigned_abs() > GUARD as u32)
    {
        return none;
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
    let cross = S::sub(
        S::mul(S::sub(v2[0], v0[0]), S::sub(v1[1], v0[1])),
        S::mul(S::sub(v2[1], v0[1]), S::sub(v1[0], v0[0])),
    );
    if cross == 0 {
        return none;
    }
    let mid_right = cross < 0;
    let r0 = first_row::<S>(v0[1]).max(clip.y0);
    let r1 = first_row::<S>(v1[1]).clamp(r0, clip.y1.max(r0));
    let r2 = first_row::<S>(v2[1]).min(clip.y1);
    if r0 >= r2 {
        return none;
    }
    let mut pixels = 0u32;
    let mut long = Edge::new::<S>(v0, v2, r0);
    let mut span = |row: i32, long: &Edge, short: &Edge| {
        let (l, r) = if mid_right {
            (long.x::<S>(), short.x::<S>())
        } else {
            (short.x::<S>(), long.x::<S>())
        };
        let (l, r) = (l.max(clip.x0), r.min(clip.x1));
        if l < r {
            let at = S::umul(row as usize, W);
            f.0[S::uadd(at, l as usize)..S::uadd(at, r as usize)].fill(c);
            pixels = S::count(pixels, S::sub(r, l) as u32);
        }
    };
    if r0 < r1 {
        let mut short = Edge::new::<S>(v0, v1, r0);
        for row in r0..r1 {
            span(row, &long, &short);
            long.step::<S>();
            short.step::<S>();
        }
    }
    let start = r1.max(r0);
    if start < r2 {
        let mut short = Edge::new::<S>(v1, v2, start);
        for row in start..r2 {
            span(row, &long, &short);
            long.step::<S>();
            short.step::<S>();
        }
    }
    // The rows visited: r0..r1 and then r1..r2 (r0 <= r1 <= r2).
    let rows = if ROWS { S::sub(r2, r0) as u32 } else { 0 };
    Filled { pixels, rows }
}

/// The slow, obvious triangle filler the tests compare [`fill_tri`]
/// with: every pixel of `clip` is tested on its own against the three
/// edges, with the same top-left rule. Not for games.
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

/// Fills a rectangle (the part on the picture) in one colour.
pub fn rect_fill(f: &mut Frame, r: &Rect, c: u8) {
    let r = r.on_screen();
    for y in r.y0..r.y1 {
        let at = y as usize * W;
        f.0[at + r.x0 as usize..at + r.x1 as usize].fill(c);
    }
}
