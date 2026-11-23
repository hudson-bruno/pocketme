//! A game's choices, made while compiling.

use crate::NEAR;
use crate::sums::{Checked, Sums};
use crate::view::View;

/// How x and y are divided by the distance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lens {
    /// In 64 bits: any point the clipping lets through (KESTREL). On the
    /// chip a 64-bit division is a slow library call.
    Wide,
    /// In 32 bits when the point is small enough (|x|, |y| below 2^20
    /// and the lens below 128 pixels), else in 64 bits with the result
    /// held to +-2^24 (CAMBER). Same pixels as `Wide` inside that range.
    Fast,
    /// In 32 bits only (BENCH): the game must keep |x| and |y| times
    /// the lens times 16 below 2^31. The smallest and fastest.
    Narrow,
}

/// How faces whose back is to the eye are found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cull {
    /// From the three turned corners (64-bit cross product; KESTREL).
    /// Exact for any matrix, scaled or not.
    Corners,
    /// From the model's face direction turned into the camera (one
    /// matrix times a vector; CAMBER). Needs a matrix without a mirror.
    Normal,
}

/// The depth a face is sorted by (far to near).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    /// The average of the three corners' distances (KESTREL).
    Middle,
    /// The farthest corner's distance (CAMBER).
    Farthest,
}

/// Everything a game decides about its 3D once, while compiling. Make
/// an empty type and fill in only what differs from [`Standard`]:
///
/// ```
/// use game_3d::{Lens, Setup, Wrapping, M8};
/// struct MySetup;
/// impl Setup for MySetup {
///     type Sums = Wrapping;
///     const NEAR: i32 = M8 / 4;
///     const LENS: Lens = Lens::Fast;
/// }
/// ```
pub trait Setup: 'static {
    /// Checked or wrapping sums.
    type Sums: Sums;
    /// The near plane (1/256 m in front of the eye).
    const NEAR: i32 = NEAR;
    /// The perspective division.
    const LENS: Lens = Lens::Wide;
    /// Back faces.
    const CULL: Cull = Cull::Corners;
    /// The depth faces are sorted by.
    const DEPTH: Depth = Depth::Middle;
    /// Cut shared edges the same way from both sides (CAMBER's rule), so
    /// two polygons that share an edge get the very same crossing point
    /// at a plane and no crack opens between them. Costs a comparison
    /// per crossing; only needed for shared corners ([`crate::Pt`]).
    const SEAM_PROOF: bool = false;
    /// Objects have a size ([`crate::Object::scale`]; KESTREL). With
    /// `false` (CAMBER) the size is not read: every model is drawn at
    /// its own size, the matrix is not scaled (nine products less per
    /// object) and the object's ball is the model's own radius.
    const SCALE: bool = true;
    /// When an object's whole ball is inside every plane, skip the
    /// plane tests of its corners (KESTREL: four plane sums per object
    /// buy five per corner). With `false` (CAMBER) every corner is
    /// tested: less code, the same picture whenever the corners lie in
    /// the ball.
    const INSIDE_SHORTCUT: bool = true;

    /// Where a camera-space point lands, in 1/16 pixel.
    #[inline(always)]
    fn project(view: &View, p: [i32; 3]) -> [i32; 2] {
        match Self::LENS {
            Lens::Wide => view.project_with::<Self::Sums>(p),
            Lens::Fast => view.project_fast::<Self::Sums>(p),
            Lens::Narrow => view.project_narrow::<Self::Sums>(p),
        }
    }

    /// The depth of a face from its corners' distances.
    #[inline(always)]
    #[track_caller]
    fn depth(a: i32, b: i32, c: i32) -> i32 {
        match Self::DEPTH {
            Depth::Middle => Self::Sums::add(Self::Sums::add(a, b), c) / 3,
            Depth::Farthest => a.max(b).max(c),
        }
    }
}

/// KESTREL's setup: checked sums, 64-bit lens, near plane at half a
/// metre, back faces by corners, faces sorted by their middle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Standard;

impl Setup for Standard {
    type Sums = Checked;
}
