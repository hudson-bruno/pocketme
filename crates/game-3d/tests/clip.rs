//! Cutting in 3D: at the near plane and at the view's sides only what is
//! inside is kept (from KESTREL's tests); the seam-proof rule gives the
//! same crossing point whichever way an edge is walked; polygons of up
//! to 16 corners are cut whole.

mod common;

use common::{Dice, plane_d};
use game_3d::{
    Checked, Lens, M8, MAX_POLY, NEAR, Rect, Setup, Standard, View, Wrapping, clip_polygon,
    clip_polygon_with,
};

/// CAMBER's choices: seam-proof cuts, near plane at a quarter metre.
struct Seams;
impl Setup for Seams {
    type Sums = Checked;
    const NEAR: i32 = M8 / 4;
    const LENS: Lens = Lens::Fast;
    const SEAM_PROOF: bool = true;
}

/// KESTREL's choices with wrapping sums (MERCATOR's way of writing
/// sums).
struct Wrap;
impl Setup for Wrap {
    type Sums = Wrapping;
}

#[test]
fn cutting_at_the_near_plane() {
    let view = View::new(Rect::new(0, 10, 160, 96), 96);
    let planes = view.planes();
    // In front: unchanged.
    let mut poly = [[0; 3]; MAX_POLY];
    poly[..3].copy_from_slice(&[[0, 0, 1000], [100, 0, 1000], [0, 100, 1200]]);
    let before = poly;
    assert_eq!(clip_polygon(&mut poly, 3, &planes, 1), 3);
    assert_eq!(poly, before);
    // All behind the eye: nothing.
    poly[..3].copy_from_slice(&[[0, 0, -10], [100, 0, -500], [0, 100, NEAR - 1]]);
    assert_eq!(clip_polygon(&mut poly, 3, &planes, 1), 0);
    // One corner behind: four corners, none nearer than the near plane.
    poly[..3].copy_from_slice(&[[0, 0, -300], [200, 0, 800], [-200, 50, 900]]);
    let n = clip_polygon(&mut poly, 3, &planes, 1);
    assert_eq!(n, 4);
    for p in &poly[..n] {
        assert!(p[2] >= NEAR - 1, "{p:?} is in front of the near plane");
    }
    // Two corners behind: still a triangle.
    poly[..3].copy_from_slice(&[[0, 0, -300], [200, 0, -800], [-200, 50, 900]]);
    assert_eq!(clip_polygon(&mut poly, 3, &planes, 1), 3);
}

#[test]
fn cutting_at_every_plane_keeps_only_whats_inside() {
    let view = View::new(Rect::new(16, 18, 128, 80), 76);
    let planes = view.planes();
    let mut d = Dice(0x5EED);
    let mut cut = 0;
    for _ in 0..20_000 {
        let mut poly = [[0; 3]; MAX_POLY];
        for p in poly.iter_mut().take(3) {
            *p = [
                d.range(-9000, 9000),
                d.range(-9000, 9000),
                d.range(-3000, 9000),
            ];
        }
        let mut wrapped = poly;
        let n = clip_polygon(&mut poly, 3, &planes, 0b1_1111);
        // Wrapping sums give the very same corners.
        let nw = clip_polygon_with::<Wrap, MAX_POLY>(&mut wrapped, 3, &planes, 0b1_1111);
        assert_eq!(n, nw);
        assert_eq!(poly[..n], wrapped[..n]);
        assert!(n == 0 || (3..=MAX_POLY).contains(&n));
        if n > 0 {
            cut += 1;
        }
        for p in &poly[..n] {
            for pl in &planes {
                // Rounding may leave a corner a hair outside.
                assert!(
                    plane_d(pl, p) >= -2 * i64::from(pl[0].abs() + pl[1].abs() + pl[2].abs()),
                    "{p:?} outside {pl:?}"
                );
            }
        }
    }
    assert!(cut > 1000);
}

/// The corners that lie on plane 0 after cutting `poly` at it.
fn crossings<C: Setup>(poly: &[[i32; 3]], planes: &[[i32; 4]; 5]) -> Vec<[i32; 3]> {
    let mut p = [[0; 3]; 16];
    p[..poly.len()].copy_from_slice(poly);
    let n = clip_polygon_with::<C, 16>(&mut p, poly.len(), planes, 1);
    p[..n]
        .iter()
        .copied()
        .filter(|c| !poly.contains(c))
        .collect()
}

#[test]
fn a_seam_proof_cut_gives_the_same_point_from_both_sides() {
    // Two triangles share the edge a-b, which crosses the near plane;
    // they walk it in opposite directions.
    let view = View::new(Rect::new(0, 0, 160, 120), 80);
    let planes = view.planes_at(Seams::NEAR);
    let mut d = Dice(0x5EA);
    let mut differed = 0;
    for _ in 0..5000 {
        let a = [
            d.range(-3000, 3000),
            d.range(-3000, 3000),
            d.range(-2000, 0),
        ];
        let b = [
            d.range(-3000, 3000),
            d.range(-3000, 3000),
            d.range(200, 6000),
        ];
        let c1 = [
            d.range(-3000, 3000),
            d.range(-3000, 3000),
            d.range(200, 6000),
        ];
        let c2 = [
            d.range(-3000, 3000),
            d.range(-3000, 3000),
            d.range(200, 6000),
        ];
        // Seam-proof: the shared edge's crossing is in both, exactly.
        let one = crossings::<Seams>(&[a, b, c1], &planes);
        let two = crossings::<Seams>(&[b, a, c2], &planes);
        let shared: Vec<_> = one.iter().filter(|p| two.contains(p)).collect();
        assert!(!shared.is_empty(), "{a:?} {b:?}: no shared crossing");
        // Without it the two points may differ by rounding.
        let one = crossings::<Standard>(&[a, b, c1], &planes);
        let two = crossings::<Standard>(&[b, a, c2], &planes);
        if !one.iter().any(|p| two.contains(p)) {
            differed += 1;
        }
    }
    assert!(differed > 0, "the test never met a rounding difference");
}

#[test]
fn a_sixteen_corner_polygon_is_cut_whole() {
    // A ring of 11 corners round the view's middle, 2 m deep, tipped so
    // it crosses the near plane and the left and right sides: up to 16
    // corners after the cut.
    let view = View::new(Rect::new(0, 0, 160, 120), 80);
    let planes = view.planes_at(Seams::NEAR);
    let mut ring = [[0i32; 3]; 16];
    let n = 11;
    for (k, p) in ring.iter_mut().take(n).enumerate() {
        let a = (k as u32 * 65_536 / n as u32) as u16;
        let (s, c) = (game_3d::sin(a), game_3d::cos(a));
        *p = [
            (c * 3000) >> 14,
            (s * 900) >> 14,
            2 * M8 + ((c * 900) >> 14),
        ];
    }
    // With room for 32 corners nothing can be lost: the reference.
    let mut wide = [[0i32; 3]; 32];
    wide[..16].copy_from_slice(&ring);
    let want = clip_polygon_with::<Seams, 32>(&mut wide, n, &planes, 0b1_1111);
    let m = clip_polygon_with::<Seams, 16>(&mut ring, n, &planes, 0b1_1111);
    assert!(m >= 3, "the ring is partly in view");
    assert_eq!(m, want, "16 corners of room are enough for 11 + 5");
    assert_eq!(ring[..m], wide[..m]);
    for p in &ring[..m] {
        for pl in &planes {
            assert!(plane_d(pl, p) >= -2 * i64::from(pl[0].abs() + pl[1].abs() + pl[2].abs()));
        }
    }
}
