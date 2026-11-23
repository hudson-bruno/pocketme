//! Simple 3D with polygons: KESTREL's use of the shared 3D library,
//! `game-3d` (the triangle filler, clipping, back faces, painter's order
//! and shading all live there; see its README).
//!
//! KESTREL draws with the library's [`Standard`](game_3d::Standard)
//! setup: checked sums (a sum that runs over stops the game, as before),
//! the 64-bit lens, the near plane at half a metre, back faces found
//! from the turned corners, faces sorted by their middle. Its renderer
//! holds up to [`MAX_VERTS`] corners and [`MAX_TRIS`] triangles per
//! model.
//!
//! Units: positions in 1/256 m ([`M8`]), screen positions in 1/16 pixel
//! ([`SUB`]), matrices in 1/16384 ([`crate::fixed::ONE14`]).

pub use game_3d::{
    Camera, Face, GUARD, M8, MAX_POLY, Model, NEAR, Object, Planes, RawFace, Rect, SUB, Shading,
    Stats, View, clip_polygon, cross_of, fill_tri, fill_tri_slow, normals, orient, part_middle,
    plane_dist, radius_of, rect_fill,
};

/// Most corners in one model.
pub const MAX_VERTS: usize = 48;
/// Most triangles one model can put on screen at once (after back
/// faces are dropped and clipped faces are cut into triangles).
pub const MAX_TRIS: usize = 112;

/// The renderer's working memory: one model's corners and triangles.
/// About 2.4 KB; kept in the game's state, never on the stack.
pub type Renderer = game_3d::Renderer<game_3d::Standard, MAX_VERTS, MAX_TRIS>;
