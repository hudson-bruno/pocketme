//! The triangle filler (from KESTREL's tests): the fast filler gives
//! exactly the pixels of the slow, obvious one, for random and odd
//! triangles and any clipping rectangle, with checked and with wrapping
//! sums; triangles that share an edge never paint a pixel twice and
//! never leave a gap.
//!
//! Tests may use floating point; the library never does.

mod common;

use common::{Dice, FULL, blank, corner};
use game_3d::{
    Checked, Rect, SUB, Sums, Wrapping, fill_tri, fill_tri_counted, fill_tri_slow, fill_tri_with,
};
use game_kit::{Frame, H, W};

fn fast_equals_slow<S: Sums>(seed: u64) {
    let mut d = Dice(seed);
    let mut painted_total = 0u64;
    for n in 0..6000 {
        let mut p = [corner(&mut d), corner(&mut d), corner(&mut d)];
        // Small triangles too: a corner near another.
        if n % 4 == 0 {
            p[1] = [p[0][0] + d.range(-40, 40), p[0][1] + d.range(-40, 40)];
            p[2] = [p[0][0] + d.range(-40, 40), p[0][1] + d.range(-40, 40)];
        }
        let clip = if n % 3 == 0 {
            let x0 = d.range(-10, 150);
            let y0 = d.range(-10, 110);
            Rect::new(x0, y0, d.range(0, 80), d.range(0, 60))
        } else {
            FULL
        };
        let (mut a, mut b) = (blank(), blank());
        let na = fill_tri_with::<S>(&mut a, &clip, &p, 7);
        let nb = fill_tri_slow(&mut b, &clip, &p, 7);
        assert_eq!(na, nb, "triangle {n} {p:?} clip {clip:?}: counts");
        assert!(
            a.0 == b.0,
            "triangle {n} {p:?} clip {clip:?}: pixels differ"
        );
        painted_total += u64::from(na);
    }
    assert!(painted_total > 100_000, "the triangles were nearly empty");
}

#[test]
fn the_fast_filler_paints_exactly_the_slow_fillers_pixels() {
    fast_equals_slow::<Checked>(0x7E57_0001);
}

#[test]
fn with_wrapping_sums_the_filler_paints_the_same_pixels() {
    fast_equals_slow::<Wrapping>(0x7E57_0001);
    fast_equals_slow::<Wrapping>(0x7E57_0002);
}

#[test]
fn odd_triangles() {
    let mut f = blank();
    // No area: three corners on a line, and all corners the same.
    assert_eq!(
        fill_tri(&mut f, &FULL, &[[0, 0], [800, 800], [1600, 1600]], 1),
        0
    );
    assert_eq!(fill_tri(&mut f, &FULL, &[[88, 88]; 3], 1), 0);
    // A tiny triangle round one pixel's middle paints just that pixel.
    let mid = [5 * SUB + 8, 6 * SUB + 8];
    let t = [
        [mid[0] - 2, mid[1] - 2],
        [mid[0] + 3, mid[1] - 1],
        [mid[0], mid[1] + 3],
    ];
    assert_eq!(fill_tri(&mut f, &FULL, &t, 9), 1);
    assert_eq!(f.0[6 * W + 5], 9);
    // A tiny triangle between pixel middles paints nothing.
    let mut g = blank();
    let t = [
        [5 * SUB + 9, 6 * SUB + 9],
        [5 * SUB + 12, 6 * SUB + 9],
        [5 * SUB + 10, 6 * SUB + 12],
    ];
    assert_eq!(fill_tri(&mut g, &FULL, &t, 9), 0);
    // A corner beyond the guard band: nothing (and no overflow).
    assert_eq!(
        fill_tri(
            &mut g,
            &FULL,
            &[[0, 0], [game_3d::GUARD + 1, 0], [0, 100]],
            1
        ),
        0
    );
    assert_eq!(
        fill_tri(
            &mut g,
            &FULL,
            &[[i32::MIN, 0], [i32::MAX, 0], [0, i32::MAX]],
            1
        ),
        0
    );
    assert_eq!(
        fill_tri_with::<Wrapping>(
            &mut g,
            &FULL,
            &[[i32::MIN, 0], [i32::MAX, 0], [0, i32::MAX]],
            1
        ),
        0
    );
    // The same triangle in all six corner orders paints the same pixels.
    let p = [[13, 40], [1900, 300], [700, 1700]];
    let mut first = blank();
    let n0 = fill_tri(&mut first, &FULL, &p, 3);
    assert!(n0 > 100);
    for order in [[0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let mut h = blank();
        let q = order.map(|i| p[i]);
        assert_eq!(fill_tri(&mut h, &FULL, &q, 3), n0);
        assert!(h.0 == first.0, "order {order:?}");
    }
    // A triangle covering the whole picture paints every pixel once.
    let mut all = blank();
    let big = [[-2000, -2000], [8000, -2000], [-2000, 8000]];
    assert_eq!(fill_tri(&mut all, &FULL, &big, 4), (W * H) as u32);
    assert!(all.0.iter().all(|&p| p == 4));
}

#[test]
fn the_clip_rectangle_is_the_same_as_painting_all_and_cutting_out() {
    let mut d = Dice(0xC11F);
    for _ in 0..800 {
        let p = [corner(&mut d), corner(&mut d), corner(&mut d)];
        let clip = Rect::new(
            d.range(0, 100),
            d.range(0, 80),
            d.range(1, 60),
            d.range(1, 40),
        );
        let mut cut = blank();
        fill_tri(&mut cut, &clip, &p, 5);
        let mut all = blank();
        fill_tri(&mut all, &FULL, &p, 5);
        let c = clip.on_screen();
        for y in 0..H as i32 {
            for x in 0..W as i32 {
                let inside = x >= c.x0 && x < c.x1 && y >= c.y0 && y < c.y1;
                let want = if inside {
                    all.0[y as usize * W + x as usize]
                } else {
                    0
                };
                assert_eq!(
                    cut.0[y as usize * W + x as usize],
                    want,
                    "({x},{y}) {p:?} {clip:?}"
                );
            }
        }
    }
}

/// Paints a fan of triangles round `mid` (a convex polygon) and checks:
/// no pixel is painted twice, and every pixel whose middle is strictly
/// inside the polygon is painted.
fn fan_has_no_gaps_or_overlaps(mid: [i32; 2], ring: &[[i32; 2]]) {
    let mut count = vec![0u8; W * H];
    for k in 0..ring.len() {
        let t = [mid, ring[k], ring[(k + 1) % ring.len()]];
        let mut f = blank();
        fill_tri(&mut f, &FULL, &t, 1);
        for (c, p) in count.iter_mut().zip(f.0.iter()) {
            *c += *p;
        }
    }
    assert!(
        count.iter().all(|&c| c <= 1),
        "a pixel painted twice: {ring:?}"
    );
    for y in 0..H as i64 {
        for x in 0..W as i64 {
            let (px, py) = (x * 16 + 8, y * 16 + 8);
            let mut inside = true;
            let mut sign = 0i64;
            for k in 0..ring.len() {
                let a = ring[k].map(i64::from);
                let b = ring[(k + 1) % ring.len()].map(i64::from);
                let e = (b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0]);
                if e == 0 {
                    inside = false;
                    break;
                }
                if sign == 0 {
                    sign = e.signum();
                } else if e.signum() != sign {
                    inside = false;
                    break;
                }
            }
            if inside {
                assert_eq!(
                    count[(y * W as i64 + x) as usize],
                    1,
                    "gap at ({x},{y}) in {ring:?}"
                );
            }
        }
    }
}

#[test]
fn triangles_sharing_edges_leave_no_gap_and_no_overlap() {
    // A regular-ish polygon, then random convex ones (points on an
    // ellipse in angle order), with the middle on a pixel middle too.
    let mut d = Dice(0xFA4);
    for n in 0..60 {
        let corners = 3 + (n % 9) as usize;
        let (cx, cy) = (
            d.range(30, 130) * 16 + (n % 2) * 8,
            d.range(20, 100) * 16 + 8,
        );
        let (rx, ry) = (d.range(80, 900), d.range(80, 700));
        let mut ring = Vec::new();
        let mut a = d.range(0, 359) as f64;
        for _ in 0..corners {
            a += 360.0 / corners as f64 * (0.6 + (d.next() % 80) as f64 / 100.0);
            let r = a.to_radians();
            ring.push([
                cx + (rx as f64 * r.cos()) as i32,
                cy + (ry as f64 * r.sin()) as i32,
            ]);
        }
        // Keep it convex: sort by angle round the middle.
        ring.sort_by(|p, q| {
            let ap = ((p[1] - cy) as f64).atan2((p[0] - cx) as f64);
            let aq = ((q[1] - cy) as f64).atan2((q[0] - cx) as f64);
            ap.partial_cmp(&aq).unwrap()
        });
        fan_has_no_gaps_or_overlaps([cx, cy], &ring);
    }
    // A square cut along its diagonal, with corners on pixel edges.
    let sq = [[160, 160], [960, 160], [960, 960], [160, 960]];
    let (mut a, mut b) = (blank(), blank());
    let na = fill_tri(&mut a, &FULL, &[sq[0], sq[1], sq[2]], 1);
    let nb = fill_tri(&mut b, &FULL, &[sq[0], sq[2], sq[3]], 1);
    assert_eq!(na + nb, 50 * 50, "a 50 x 50 square is 2,500 pixels");
    assert!(a.0.iter().zip(b.0.iter()).all(|(x, y)| x & y == 0));
}

#[test]
fn a_rectangle_is_filled_only_on_the_picture() {
    let mut f: Box<Frame> = blank();
    game_3d::rect_fill(&mut f, &Rect::new(-5, 110, 20, 30), 6);
    let n = f.0.iter().filter(|&&p| p == 6).count();
    assert_eq!(n, 15 * 10);
    assert_eq!(f.0[119 * W + 14], 6);
    assert_eq!(f.0[119 * W + 15], 0);
}

#[test]
fn the_counting_filler_paints_the_same_and_counts_the_rows() {
    let mut d = Dice(0xC0_0E7);
    let mut rows_total = 0u64;
    for n in 0..4000 {
        let p = [corner(&mut d), corner(&mut d), corner(&mut d)];
        let clip = if n % 3 == 0 {
            Rect::new(
                d.range(-10, 150),
                d.range(-10, 110),
                d.range(0, 80),
                d.range(0, 60),
            )
        } else {
            FULL
        };
        let (mut a, mut b) = (blank(), blank());
        let na = fill_tri_with::<Checked>(&mut a, &clip, &p, 5);
        let done = fill_tri_counted::<Checked>(&mut b, &clip, &p, 5);
        assert_eq!(done.pixels, na, "triangle {n}");
        assert!(a.0 == b.0, "triangle {n}: pixels differ");
        assert_eq!(
            fill_tri_counted::<Wrapping>(&mut blank(), &clip, &p, 5),
            done
        );
        // Every row painted was visited, and no row beyond the clip
        // rectangle on the picture.
        let painted = (0..H)
            .filter(|&y| b.0[y * W..(y + 1) * W].contains(&5))
            .count();
        let on = clip.on_screen();
        assert!(done.rows as usize >= painted, "triangle {n}: rows {done:?}");
        assert!(done.rows as i32 <= (on.y1 - on.y0).max(0), "triangle {n}");
        rows_total += u64::from(done.rows);
    }
    assert!(rows_total > 10_000);
    // A triangle covering the picture visits every row once.
    let big = [[-2000, -2000], [8000, -2000], [-2000, 8000]];
    let done = fill_tri_counted::<Checked>(&mut blank(), &FULL, &big, 4);
    assert_eq!(done.rows, H as u32);
    assert_eq!(done.pixels, (W * H) as u32);
}
