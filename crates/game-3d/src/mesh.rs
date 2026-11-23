//! Shared corners: floors, tracks and walls made of many polygons whose
//! corners are each turned and projected once (CAMBER's way).
//!
//! Turn each corner into the camera once with [`Pt::new`], then hand
//! each polygon's corners to [`draw_poly`]. Polygons are drawn in the
//! order given (the game sorts them, far to near). Use a [`Setup`] with
//! `SEAM_PROOF = true`: two polygons that share an edge are then cut at
//! the very same points, and no crack of background shows between them.

use game_kit::Frame;

use crate::GUARD;
use crate::clip::clip_polygon_with;
use crate::count::{Count, fill_into};
use crate::setup::Setup;
use crate::view::{Planes, View};

/// A corner in camera space, with the planes it is outside of and (when
/// it is inside all of them) where it lands on the screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pt {
    pub c: [i32; 3],
    pub out: u8,
    pub s: [i32; 2],
}

impl Pt {
    /// The point `c` (camera space): its planes and, if inside, its place
    /// on the screen (`planes` from [`View::clip_planes_at`] with the
    /// setup's near plane). Counts one corner in `st` ([`Count::verts`],
    /// and [`Count::projected`] when it is projected).
    pub fn new<C: Setup>(c: [i32; 3], view: &View, planes: &Planes, st: &mut impl Count) -> Pt {
        let out = planes.outcode::<C::Sums>(&c);
        let s = if out == 0 {
            st.projected::<C::Sums>();
            C::project(view, c)
        } else {
            [0, 0]
        };
        st.verts::<C::Sums>(1);
        Pt { c, out, s }
    }
}

/// Fills a convex polygon of up to `P` shared corners in one colour: as
/// it is when every corner is inside the view, cut in 3D when not (room
/// for `P` corners after the cut: give five more than the polygon has).
/// Returns false when nothing of it was in view.
pub fn draw_poly<C: Setup, const P: usize>(
    f: &mut Frame,
    view: &View,
    planes: &Planes,
    pts: &[&Pt],
    c: u8,
    st: &mut impl Count,
) -> bool {
    type S<C> = <C as Setup>::Sums;
    let n = pts.len().min(P);
    let (mut and, mut or) = (0xFFu8, 0u8);
    for p in &pts[..n] {
        and &= p.out;
        or |= p.out;
    }
    if and != 0 || n < 3 {
        return false;
    }
    st.front::<S<C>>();
    if or == 0 {
        for k in 1..n - 1 {
            fill_into::<S<C>, _>(f, &view.area, &[pts[0].s, pts[k].s, pts[k + 1].s], c, st);
        }
        return true;
    }
    st.clipped::<S<C>>();
    let mut poly = [[0i32; 3]; P];
    for (q, p) in poly.iter_mut().zip(&pts[..n]) {
        *q = p.c;
    }
    let m = clip_polygon_with::<C, P>(&mut poly, n, &planes.p, or);
    if m < 3 {
        return false;
    }
    let mut sp = [[0i32; 2]; P];
    for k in 0..m {
        let s = C::project(view, poly[k]);
        st.projected::<S<C>>();
        sp[k] = [s[0].clamp(-GUARD, GUARD), s[1].clamp(-GUARD, GUARD)];
    }
    for k in 1..m - 1 {
        fill_into::<S<C>, _>(f, &view.area, &[sp[0], sp[k], sp[k + 1]], c, st);
    }
    true
}
