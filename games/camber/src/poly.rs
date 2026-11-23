//! Simple 3D with polygons, in whole numbers: CAMBER's choices for the
//! shared 3D library (`crates/game-3d`), and CAMBER's own counts.
//!
//! The drawing itself (the triangle filler, the planes and the cutting,
//! the lens, back faces, the model tables made while compiling and the
//! model renderer) is game-3d's, shared with KESTREL. What CAMBER
//! chooses, while compiling ([`Camber`]):
//!
//! * **Points shared by many polygons** ([`Pt`]): a corner of the track
//!   is turned into the camera's view, tested against the five planes
//!   and projected once, then used by every polygon that touches it.
//!   Polygons, not only triangles ([`draw_poly`]): a quad of road is cut
//!   as one polygon (one cut per plane, not two) and filled as a fan;
//!   a polygon may have up to [`MAX_POLY`] corners after it is cut.
//! * **Seams that cannot open** (`SEAM_PROOF`): where an edge is cut by
//!   a plane, the crossing point is worked out from the two corners in a
//!   fixed order, so two polygons that share a cut edge get exactly the
//!   same point, and the filler's top-left rule shares its pixels out
//!   with no gap.
//! * **A 32-bit projection** (`Lens::Fast`): every point the renderer
//!   keeps is inside the guard band, where `x * focal * 16` fits 32
//!   bits; the chip divides 32-bit numbers in hardware and 64-bit ones
//!   in a long routine.
//! * The model renderer ([`Renderer`]) draws the ships: back faces by
//!   the stored face directions, faces sorted by their farthest corner,
//!   no object sizes, every corner tested against the planes.
//! * Checked sums: a sum that runs over stops the game, on the PC and on
//!   the chip alike.
//!
//! Units: positions in 1/256 m ([`M8`]), screen positions in 1/16 pixel
//! ([`SUB`]), matrices in 1/16384 ([`crate::fixed::ONE14`]). Camera
//! space: x right, y up, z forward.

use game_3d::{Count, Cull, Depth, Lens, Setup};

pub use game_3d::{
    Camera, Checked, Face, Filled, GUARD, M8, Model, Object, Planes, Pt, RawFace, Rect, SUB, Steps,
    View, draw_poly, fill_tri_counted, fill_tri_slow, normals, orient_by_middle, radius_of,
};

/// CAMBER's 3D choices (see the module's notes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Camber;

impl Setup for Camber {
    type Sums = Checked;
    /// The chase camera sits low and close; a quarter metre keeps the
    /// road right under it whole.
    const NEAR: i32 = M8 / 4;
    const LENS: Lens = Lens::Fast;
    const CULL: Cull = Cull::Normal;
    const DEPTH: Depth = Depth::Farthest;
    const SEAM_PROOF: bool = true;
    const SCALE: bool = false;
    const INSIDE_SHORTCUT: bool = false;
}

/// The near plane: nothing closer to the eye than this (1/256 m).
pub const NEAR: i32 = Camber::NEAR;
/// Most corners in one model.
pub const MAX_VERTS: usize = 28;
/// Most triangles one model can put on screen at once.
pub const MAX_TRIS: usize = 44;
/// Most corners a polygon can have after it is cut by five planes: a
/// road strip has up to 10 (five road points on each boundary) and each
/// plane adds at most one.
pub const MAX_POLY: usize = 16;

/// The model renderer: its working memory is about 1.1 KB.
pub type Renderer = game_3d::Renderer<Camber, MAX_VERTS, MAX_TRIS, MAX_POLY, Stats>;

/// What the renderer did for one picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Points turned into camera space (a matrix times a vector, or
    /// sums of turned vectors).
    pub verts: u32,
    /// Points projected (two divisions each).
    pub projected: u32,
    /// Polygons offered (track quads and model faces in view).
    pub polys: u32,
    /// Polygons that had to be cut by a plane.
    pub clipped: u32,
    /// Triangles filled.
    pub tris: u32,
    /// Rows (spans) the filler visited.
    pub rows: u32,
    /// Pixels painted (a pixel painted twice counts twice).
    pub pixels: u32,
    /// Model triangles that did not fit [`MAX_TRIS`] (should stay 0).
    pub dropped: u32,
}

/// The counts game-3d hands over, in CAMBER's own fields (plain sums,
/// checked like every other sum in the game).
impl Count for Stats {
    const ROWS: bool = true;
    const ZERO: Self = Stats {
        verts: 0,
        projected: 0,
        polys: 0,
        clipped: 0,
        tris: 0,
        rows: 0,
        pixels: 0,
        dropped: 0,
    };
    #[inline(always)]
    fn verts<S: game_3d::Sums>(&mut self, n: u32) {
        self.verts += n;
    }
    #[inline(always)]
    fn projected<S: game_3d::Sums>(&mut self) {
        self.projected += 1;
    }
    #[inline(always)]
    fn front<S: game_3d::Sums>(&mut self) {
        self.polys += 1;
    }
    #[inline(always)]
    fn clipped<S: game_3d::Sums>(&mut self) {
        self.clipped += 1;
    }
    // One copy for the three places that fill (less code; one call a
    // triangle).
    #[inline(never)]
    fn filled<S: game_3d::Sums>(&mut self, done: Filled) {
        self.tris += 1;
        self.rows += done.rows;
        self.pixels += done.pixels;
    }
    #[inline(always)]
    fn dropped<S: game_3d::Sums>(&mut self) {
        self.dropped += 1;
    }
}
