//! Shared corners (CAMBER's path): a floor of many quads whose corners
//! are turned once and shared, reaching behind the eye and past the
//! view's sides. With seam-proof cuts every pixel below the floor's far
//! edge is painted exactly once: no crack, no overlap.

mod common;

use common::blank;
use game_3d::{
    Camera, Checked, Lens, M8, Mat3, Pt, Rect, Setup, Stats, View, Wrapping, deg, draw_poly,
};
use game_kit::{H, W};

/// CAMBER's choices.
struct Track;
impl Setup for Track {
    type Sums = Checked;
    const NEAR: i32 = M8 / 4;
    const LENS: Lens = Lens::Fast;
    const SEAM_PROOF: bool = true;
}

/// The same with wrapping sums.
struct TrackWrapping;
impl Setup for TrackWrapping {
    type Sums = Wrapping;
    const NEAR: i32 = M8 / 4;
    const LENS: Lens = Lens::Fast;
    const SEAM_PROOF: bool = true;
}

/// Grid size: N x N quads, each 1.5 m, from 6 m behind the eye.
const N: usize = 14;
const STEP: i32 = 3 * M8 / 2;

/// Draws the floor seen from `cam` and returns, per pixel, how many
/// quads painted it, and the stats.
fn floor_counts<C: Setup>(cam: &Camera, view: &View) -> (Vec<u8>, Stats) {
    let planes = view.clip_planes_at(C::NEAR);
    let mut st = Stats::default();
    let mut pts = [[Pt::default(); N + 1]; N + 1];
    for (i, row) in pts.iter_mut().enumerate() {
        for (j, p) in row.iter_mut().enumerate() {
            let w = [
                (j as i32 - N as i32 / 2) * STEP,
                0,
                (i as i32) * STEP - 6 * M8,
            ];
            *p = Pt::new::<C>(cam.to_cam(w), view, &planes, &mut st);
        }
    }
    let mut count = vec![0u8; W * H];
    for i in 0..N {
        for j in 0..N {
            let quad = [
                &pts[i][j],
                &pts[i + 1][j],
                &pts[i + 1][j + 1],
                &pts[i][j + 1],
            ];
            let mut f = blank();
            draw_poly::<C, 9>(&mut f, view, &planes, &quad, 1, &mut st);
            for (c, p) in count.iter_mut().zip(f.0.iter()) {
                *c += *p;
            }
        }
    }
    (count, st)
}

fn check_floor<C: Setup>(cam: &Camera, view: &View) -> Stats {
    let (count, st) = floor_counts::<C>(cam, view);
    let a = view.area;
    for x in a.x0..a.x1 {
        // From the bottom of the view up: painted once, then (past the
        // floor's far edge) never again.
        let mut floor = true;
        for y in (a.y0..a.y1).rev() {
            let c = count[y as usize * W + x as usize];
            assert!(c <= 1, "pixel ({x},{y}) painted {c} times");
            if c == 0 {
                floor = false;
            } else {
                assert!(floor, "a crack below pixel ({x},{y})");
            }
        }
    }
    // Nothing outside the view.
    for (i, &c) in count.iter().enumerate() {
        let (x, y) = ((i % W) as i32, (i / W) as i32);
        if !(x >= a.x0 && x < a.x1 && y >= a.y0 && y < a.y1) {
            assert_eq!(c, 0);
        }
    }
    // The bottom row is all floor.
    let y = a.y1 - 1;
    assert!((a.x0..a.x1).all(|x| count[y as usize * W + x as usize] == 1));
    st
}

#[test]
fn a_floor_of_shared_corners_has_no_cracks_and_no_overlaps() {
    let view = View::new(Rect::new(4, 8, 152, 104), 70);
    let mut clipped = 0;
    for (k, (yaw, pitch, roll)) in [
        (0, deg(-10), 0),
        (deg(17), deg(-14), deg(5)),
        (deg(-33), deg(-8), deg(-9)),
        (deg(71), deg(-20), deg(12)),
        (deg(-128), deg(-6), 0),
    ]
    .into_iter()
    .enumerate()
    {
        let cam = Camera {
            pos: [37 * k as i32, M8 + 23 * k as i32, 11 * k as i32],
            rot: Mat3::rotation(yaw, pitch, roll).transposed(),
        };
        let st = check_floor::<Track>(&cam, &view);
        check_floor::<TrackWrapping>(&cam, &view);
        assert!(st.front > 0);
        clipped += st.clipped;
        // Same pixels with wrapping sums.
        assert_eq!(
            floor_counts::<Track>(&cam, &view).0,
            floor_counts::<TrackWrapping>(&cam, &view).0
        );
    }
    assert!(clipped > 10, "the floor was hardly ever cut");
}

#[test]
fn a_polygon_wholly_outside_one_plane_is_not_drawn() {
    let view = View::new(Rect::new(0, 0, 160, 120), 70);
    let planes = view.clip_planes_at(Track::NEAR);
    let mut st = Stats::default();
    // All four corners behind the eye.
    let p: Vec<Pt> = [[0, 0, -100], [100, 0, -100], [100, 100, -50], [0, 100, -50]]
        .iter()
        .map(|&c| Pt::new::<Track>(c, &view, &planes, &mut st))
        .collect();
    let refs: Vec<&Pt> = p.iter().collect();
    let mut f = blank();
    assert!(!draw_poly::<Track, 9>(
        &mut f, &view, &planes, &refs, 1, &mut st
    ));
    assert!(f.0.iter().all(|&c| c == 0));
    assert_eq!(st.front, 0);
}
