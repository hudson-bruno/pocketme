//! The games' shared 3D library: flat-shaded polygon models, in whole
//! numbers, drawn into the console's 160 x 120 picture.
//!
//! Like `game-kit`, this crate is compiled into each game that uses it,
//! never into the console: a game that does not use it pays nothing, and
//! a game that does gets only the parts it calls. It has no heap, no
//! floating point and no unsafe code, and it depends only on `game-kit`
//! (for the picture, [`game_kit::Frame`]).
//!
//! # Drawing a model
//!
//! ```
//! use game_3d::{Camera, Mat3, Object, Rect, Renderer, Shading, Standard, View, M8};
//! # use game_3d::{Face, Model};
//! # static VERTS: [[i16; 3]; 4] = [[0, 0, 0], [256, 0, 0], [0, 256, 0], [0, 0, 256]];
//! # static FACES: [Face; 1] = [Face { v: [0, 2, 1], mat: 0 }];
//! # static NORMALS: [[i16; 3]; 1] = [[0, 0, -16384]];
//! # static MODEL: Model = Model { verts: &VERTS, faces: &FACES, normals: &NORMALS, radius: 444 };
//! # let mut frame = game_kit::Frame::new();
//! // Working memory for up to 48 corners and 112 triangles per model.
//! // About 2.4 KB: keep it in the game's state, not on the stack.
//! let mut r: Renderer<Standard, 48, 112> = Renderer::new();
//! // The whole picture, with a lens of 96 pixels per metre at 1 m.
//! let view = View::new(Rect::new(0, 0, 160, 120), 96);
//! // An eye 3 m behind the middle of the world, looking along +z.
//! let cam = Camera { pos: [0, 0, -3 * M8], rot: Mat3::IDENTITY };
//! let light = Shading {
//!     mats: 4, shades: 4, light: [0, -16384, 0], glow: 0,
//!     fog_from: 0, fog_to: 0, fog_shades: 0, clear: 0, dark_side: 3,
//! };
//! let rock = Object { model: &MODEL, pos: [0, 0, 0], rot: Mat3::IDENTITY, scale: 256, swap: (0, 0) };
//! r.begin();
//! r.draw(&mut frame, &view, &cam, &light, &rock, 100 * M8);
//! ```
//!
//! # How a model reaches the picture
//!
//! 1. **Models** ([`Model`]) are small tables: corners in 1/256 m and
//!    faces (three corner numbers and a material). [`orient`] turns every
//!    face so its front looks out of its convex part, and [`normals`]
//!    works out each face's direction, both while compiling.
//! 2. **Turning and moving**: each corner is turned by the object's
//!    matrix ([`Mat3`]), scaled, moved to the object's place and into
//!    the camera's view ([`Camera`]: x right, y up, z forward).
//! 3. **Back faces** are dropped ([`Cull`]).
//! 4. **Clipping**: a face that crosses the near plane or reaches far
//!    outside the view is cut in 3D ([`clip_polygon`]); a face only a
//!    little outside is left to the filler (the "guard band").
//! 5. **Perspective**: x and y are divided by the distance ([`Lens`]),
//!    to 1/16 of a pixel ([`SUB`]).
//! 6. **Painter's order**: no depth buffer. The game draws its objects
//!    far to near; inside one object the faces are sorted far to near.
//! 7. **Filling** ([`fill_tri`]): row by row in one colour, edges
//!    followed exactly with the "top-left" rule, so triangles sharing an
//!    edge never paint a pixel twice and never leave a gap.
//!
//! Shared corners (a floor or a track made of many polygons, each
//! corner turned once) use [`Pt`] and [`draw_poly`] instead of a
//! [`Renderer`].
//!
//! # Choices made while compiling
//!
//! Nothing below is decided at run time; a game names its choices once,
//! in a [`Setup`], and the compiler builds only those:
//!
//! * the list sizes (corners and triangles per model): `Renderer<_, V, T>`;
//! * checked or wrapping sums ([`Checked`], [`Wrapping`]): `Setup::Sums`;
//! * 64-bit or 32-bit perspective ([`Lens`]): `Setup::LENS`;
//! * the near plane: `Setup::NEAR`;
//! * back faces by corners or by the model's face directions ([`Cull`]);
//! * the depth a face is sorted by ([`Depth`]);
//! * crack-free cuts for shared edges: `Setup::SEAM_PROOF`;
//! * whether objects have a size (the matrix scaled by `Object::scale`)
//!   or are drawn at the model's own size: `Setup::SCALE`;
//! * whether an object wholly inside the view skips the corner tests
//!   (the whole-ball shortcut) or every corner is tested:
//!   `Setup::INSIDE_SHORTCUT`;
//! * what is counted while drawing: the [`Count`] type, the fifth
//!   parameter of `Renderer<_, V, T, _, K>` (default [`Stats`]); a count
//!   no counter keeps costs no code;
//! * the shading rule: the [`Shade`] type handed to `draw` ([`Shading`]
//!   with fog, [`Lit`]`<false>` without, [`Steps`], [`Flat`]);
//! * the far distance is the `far` handed to `draw`.
//!
//! [`Standard`] is KESTREL's setup and the one to start from.
//!
//! # Units
//!
//! Positions in 1/256 m ([`M8`]), screen positions in 1/16 pixel
//! ([`SUB`]), matrices, sine and cosine in 1/16384 ([`ONE14`]), angles
//! in 1/65536 of a turn.
//!
//! # Overflow
//!
//! The chip build of a game has overflow checks on unless the game
//! turns them off (`games/build-chip.sh`; CAMBER does). With checks on,
//! [`Checked`] sums that run over stop the game (as a panic in this
//! crate's file); with checks off they wrap round like [`Wrapping`]
//! ones, and a range mistake shows as a wrong picture instead. The library keeps its own sums in range for
//! models up to about 120 m across and views up to the picture's size;
//! the numbers above are what it is tested with.

#![no_std]
#![forbid(unsafe_code)]

mod clip;
mod count;
mod fill;
mod fixed;
mod mesh;
mod model;
mod render;
mod setup;
mod shade;
mod sums;
mod view;

pub use clip::{clip_polygon, clip_polygon_with};
pub use count::Count;
pub use fill::{Filled, Rect, fill_tri, fill_tri_counted, fill_tri_slow, fill_tri_with, rect_fill};
pub use fixed::{Mat3, ONE14, SHIFT14, cos, cos_fine, deg, isqrt, sin, sin_fine};
pub use mesh::{Pt, draw_poly};
pub use model::{
    Face, Model, RawFace, cross_of, normals, orient, orient_by_middle, part_middle, radius_of,
};
pub use render::{Object, Renderer, Stats, faces_eye, faces_eye_normal, in_view};
pub use setup::{Cull, Depth, Lens, Setup, Standard};
pub use shade::{Flat, Lit, Shade, Shading, Steps};
pub use sums::{Checked, Sums, Wrapping};
pub use view::{Camera, Planes, View, plane_dist, plane_dist_with};

/// One metre in model and camera units.
pub const M8: i32 = 256;
/// Steps per pixel in screen positions (1/16 pixel).
pub const SUB: i32 = 16;
/// The filler takes screen positions from -GUARD to +GUARD (in 1/16
/// pixel: about 512 pixels each way). Beyond that it draws nothing, so
/// its sums never run over. The renderer's clipping keeps every
/// triangle well inside.
pub const GUARD: i32 = 8191;
/// The usual near plane: nothing closer to the eye than this (1/256 m).
/// A [`Setup`] may choose another.
pub const NEAR: i32 = M8 / 2;
/// Most corners a triangle can have after it is cut by five planes.
pub const MAX_POLY: usize = 8;
