//! What a game counts while it draws, chosen while compiling.

use game_kit::Frame;

use crate::fill::{Filled, Rect, fill_tri_counted, fill_tri_with};
use crate::render::Stats;
use crate::sums::Sums;

/// The counts a game keeps while drawing (for its cost tests and its
/// log). The renderer and [`crate::draw_poly`] call one of these at each
/// step; a method a counter leaves out does nothing, and the compiler
/// builds no code for it. So a game pays only for the numbers it reads.
///
/// Ready ones: [`Stats`] (KESTREL's: objects, corners, faces, faces in
/// front, cut faces, triangles, dropped triangles, pixels) and `()`
/// (counts nothing). A game with other wishes writes its own, as CAMBER
/// does (corners projected and filler rows too).
///
/// Each method gets the setup's sums `S`, so a counter can count with
/// checked or wrapping sums like the rest of the library.
pub trait Count {
    /// Count the pixel rows the filler visits. When true the filler is
    /// [`fill_tri_counted`] (one more sum per row) and
    /// [`Count::filled`] gets the rows; when false it is
    /// [`fill_tri_with`] and the rows are 0.
    const ROWS: bool = false;
    /// Every count at 0 (for `Renderer::new` and `Renderer::begin`).
    const ZERO: Self;
    /// An object was drawn (at least partly in view).
    #[inline(always)]
    fn object<S: Sums>(&mut self) {}
    /// `n` corners were turned into camera space.
    #[inline(always)]
    fn verts<S: Sums>(&mut self, _n: u32) {}
    /// One face of an object was looked at (before back faces are
    /// dropped).
    #[inline(always)]
    fn face<S: Sums>(&mut self) {}
    /// A face (or a [`crate::draw_poly`] polygon) is in front and at
    /// least partly in view: it will be drawn.
    #[inline(always)]
    fn front<S: Sums>(&mut self) {}
    /// That face had to be cut by a plane.
    #[inline(always)]
    fn clipped<S: Sums>(&mut self) {}
    /// One corner was projected onto the picture (two divisions).
    #[inline(always)]
    fn projected<S: Sums>(&mut self) {}
    /// One triangle was filled.
    #[inline(always)]
    fn filled<S: Sums>(&mut self, _done: Filled) {}
    /// A triangle did not fit the renderer's list (should never happen).
    #[inline(always)]
    fn dropped<S: Sums>(&mut self) {}
}

impl Count for Stats {
    const ZERO: Self = Stats {
        objects: 0,
        verts: 0,
        faces: 0,
        front: 0,
        clipped: 0,
        tris: 0,
        dropped: 0,
        pixels: 0,
    };
    #[inline(always)]
    #[track_caller]
    fn object<S: Sums>(&mut self) {
        self.objects = S::count(self.objects, 1);
    }
    #[inline(always)]
    #[track_caller]
    fn verts<S: Sums>(&mut self, n: u32) {
        self.verts = S::count(self.verts, n);
    }
    #[inline(always)]
    #[track_caller]
    fn face<S: Sums>(&mut self) {
        self.faces = S::count(self.faces, 1);
    }
    #[inline(always)]
    #[track_caller]
    fn front<S: Sums>(&mut self) {
        self.front = S::count(self.front, 1);
    }
    #[inline(always)]
    #[track_caller]
    fn clipped<S: Sums>(&mut self) {
        self.clipped = S::count(self.clipped, 1);
    }
    #[inline(always)]
    #[track_caller]
    fn filled<S: Sums>(&mut self, done: Filled) {
        self.pixels = S::count(self.pixels, done.pixels);
        self.tris = S::count(self.tris, 1);
    }
    #[inline(always)]
    #[track_caller]
    fn dropped<S: Sums>(&mut self) {
        self.dropped = S::count(self.dropped, 1);
    }
}

/// Counts nothing.
impl Count for () {
    const ZERO: Self = ();
}

/// Fills a triangle with the filler `K` asks for, and counts it. (A
/// sum that runs over is reported at the caller's line.)
#[inline(always)]
#[track_caller]
pub(crate) fn fill_into<S: Sums, K: Count>(
    f: &mut Frame,
    clip: &Rect,
    p: &[[i32; 2]; 3],
    c: u8,
    st: &mut K,
) {
    let done = if K::ROWS {
        fill_tri_counted::<S>(f, clip, p, c)
    } else {
        Filled {
            pixels: fill_tri_with::<S>(f, clip, p, c),
            rows: 0,
        }
    };
    st.filled::<S>(done);
}
