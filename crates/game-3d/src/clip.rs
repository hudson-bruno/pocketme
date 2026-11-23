//! Cutting polygons at planes, in 3D.

use crate::MAX_POLY;
use crate::setup::{Setup, Standard};
use crate::sums::Sums;
use crate::view::plane_dist_with;

/// Cuts a convex polygon (camera space) to the inside of the planes
/// whose bits are set in `which` (bit k: `planes[k]`), one plane after
/// the other (Sutherland and Hodgman's way). Returns the number of
/// corners left in `poly` (0 when nothing is inside). [`Standard`]
/// setup, room for [`MAX_POLY`] corners.
#[inline(always)]
pub fn clip_polygon(
    poly: &mut [[i32; 3]; MAX_POLY],
    n: usize,
    planes: &[[i32; 4]; 5],
    which: u8,
) -> usize {
    clip_polygon_with::<Standard, MAX_POLY>(poly, n, planes, which)
}

/// [`clip_polygon`] with the setup `C` and room for `P` corners (corners
/// beyond `P` are lost). A triangle cut by five planes has at most 8
/// corners; a polygon of `k` corners at most `k + 5`.
///
/// With `C::SEAM_PROOF` the crossing point of an edge is worked out from
/// its two corners in a fixed order (the smaller corner first), so two
/// polygons that walk a shared edge in opposite directions get the very
/// same point, and no crack opens between them.
pub fn clip_polygon_with<C: Setup, const P: usize>(
    poly: &mut [[i32; 3]; P],
    mut n: usize,
    planes: &[[i32; 4]; 5],
    which: u8,
) -> usize {
    let mut tmp = [[0i32; 3]; P];
    for (k, pl) in planes.iter().enumerate() {
        if which & (1 << k) == 0 || n == 0 {
            continue;
        }
        let dist = |p: &[i32; 3]| plane_dist_with::<C::Sums>(pl, p);
        let mut m = 0;
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            let (da, db) = (dist(&a), dist(&b));
            if da >= 0 && m < P {
                tmp[m] = a;
                m += 1;
            }
            if (da >= 0) != (db >= 0) && m < P {
                // The point where the edge crosses the plane.
                let (a, b, da, db) = if C::SEAM_PROOF && a > b {
                    (b, a, db, da)
                } else {
                    (a, b, da, db)
                };
                let den = <C::Sums>::sub64(da, db);
                let mut p = [0i32; 3];
                for (j, v) in p.iter_mut().enumerate() {
                    *v = <C::Sums>::add64(
                        i64::from(a[j]),
                        <C::Sums>::mul64(<C::Sums>::sub64(i64::from(b[j]), i64::from(a[j])), da)
                            / den,
                    ) as i32;
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
