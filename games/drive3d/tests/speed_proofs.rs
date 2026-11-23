//! The speed work's range proofs: the 32-bit sums that replaced
//! 64-bit ones in `src/scene.rs` give the very same numbers.
//!
//! * `cam_pt` (the corner's planes in 32 bits) against game-3d's
//!   `Pt::new::<D3>`, on corners up to its limit `CAM_MAX`, and the
//!   limit against the largest camera-space value the picture can make.
//! * `light_step` in 32 bits against the old 64-bit one (copied here),
//!   on every land triangle (fine and coarse), random triangles up to
//!   its limit `LIGHT_SIDE_MAX`, and the limit against the road pieces'
//!   and the land's sides.
//!
//! * The land's light tables against `light_step`, for every fine and
//!   coarse land triangle.
//! * `seen` (the walk's and the listing's visibility test, forward row
//!   first) against game-3d's `in_view::<D3>` on the TIGHT planes, for
//!   turned cameras, balls near every plane and the ends of the range.
//! * The block height table against the nine corners, for every block
//!   on the land and around it.
//! * The land walk's first test (`Ahead`, the camera's forward row
//!   bounded over every block height) never drops a block `seen` lets
//!   through, on random cameras over the land and round it.
//! * `nearest_point` (32-bit differences in its fast range) against the
//!   old one (copied here), on a grid over the land and round it, the
//!   road points and the middles between them (ties), and anywhere in
//!   the `i32` range (the slow way).
//! * The far hills written straight into the picture against the old
//!   way (one `vline` a column, copied here with the hills' shape), for
//!   every yaw and horizon.
//! * `no_rows` (polygons the picture counts but does not fill) against
//!   game-3d's filler: such a polygon's triangles paint no pixel and
//!   visit no row.
//!
//! `speed_baseline` shows that no picture changed on the drives; these
//! tests show why none can change elsewhere.

use drive3d::camera::VIEW;
use drive3d::draw::{col, hills};
use drive3d::land::{DIAG_BIT, FAST_REACH, LAND, nearest_point, nearest_point_slow, tri_corners};
use drive3d::order::{BLOCK, ORDER, REACH};
use drive3d::road::{POINTS, ROAD, Road, half_width};
use drive3d::scene::{
    Ahead, CAM_MAX, D3, LIFT, LIGHT_SIDE_MAX, PLANES, TIGHT, block_lo_hi, block_lo_hi_slow, cam_pt,
    coarse_light, fine_light, light_step, no_rows, seen,
};
use drive3d::tune::{CELL, CELLS_X, CELLS_Z, CORNERS_X, CORNERS_Z, M, SUB_PER_UNIT};
use game_3d::{Camera, Checked, Mat3, Pt, Stats, cos_fine, fill_tri_counted, in_view, sin_fine};
use game_kit::{Frame, H, W};

/// The light step as it was before the speed work (64-bit sums).
fn light_step_old(a: [i32; 3], b: [i32; 3], c: [i32; 3]) -> u8 {
    let u = [
        i64::from(b[0] - a[0]),
        i64::from(b[1] - a[1]),
        i64::from(b[2] - a[2]),
    ];
    let v = [
        i64::from(c[0] - a[0]),
        i64::from(c[1] - a[1]),
        i64::from(c[2] - a[2]),
    ];
    let mut n = [
        (u[1] * v[2] - u[2] * v[1]) >> 8,
        (u[2] * v[0] - u[0] * v[2]) >> 8,
        (u[0] * v[1] - u[1] * v[0]) >> 8,
    ];
    if n[1] < 0 {
        n = [-n[0], -n[1], -n[2]];
    }
    let dot = 40 * n[0] + 80 * n[1] - 45 * n[2];
    let n2 = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    if dot > 0 && dot * dot > 6806 * n2 {
        0
    } else if dot < 0 || dot * dot < 6006 * n2 {
        2
    } else {
        1
    }
}

/// A small random number source (xorshift), the same every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// A number in -(lim - 1)..lim, often near the ends and near 0.
    fn within(&mut self, lim: i32) -> i32 {
        let r = self.next();
        let lim = i64::from(lim);
        let v = match r % 4 {
            0 => (r >> 8) as i64 % lim,
            1 => lim - 1 - (r >> 8) as i64 % 64,
            2 => (r >> 8) as i64 % 1024,
            _ => (r >> 8) as i64 % (lim / 256).max(1),
        };
        (if r & (1 << 40) != 0 { -v } else { v }) as i32
    }
}

/// A corner's world place as `scene.rs` makes it (game-3d units).
fn corner_world(ci: i32, cj: i32) -> [i32; 3] {
    let i = ci.clamp(0, CORNERS_X as i32 - 1) as usize;
    let j = cj.clamp(0, CORNERS_Z as i32 - 1) as usize;
    [ci * 2048, i32::from(LAND.corner(i, j)) * 64, cj * 2048]
}

#[test]
fn the_corner_planes_in_32_bits_are_game_3ds() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut tried = 0u32;
    let mut inside = 0u32;
    let check = |c: [i32; 3], tried: &mut u32, inside: &mut u32| {
        let (mut a, mut b) = (Stats::default(), Stats::default());
        let want = Pt::new::<D3>(c, &VIEW, &PLANES, &mut a);
        let got = cam_pt(c, &mut b);
        assert_eq!(got, want, "corner {c:?}");
        assert_eq!(a, b, "counts at {c:?}");
        *tried += 1;
        *inside += u32::from(got.out == 0);
    };
    // The ends of the range, and every sign.
    let ends = [
        -(CAM_MAX - 1),
        -(CAM_MAX / 2),
        -1,
        0,
        1,
        64,
        CAM_MAX / 2,
        CAM_MAX - 1,
    ];
    for x in ends {
        for y in ends {
            for z in ends {
                check([x, y, z], &mut tried, &mut inside);
            }
        }
    }
    // Near the planes (on them, one either side), where a sign flips.
    for _ in 0..200_000 {
        let z = rng.within(CAM_MAX).abs();
        let x = rng.within(CAM_MAX);
        let y = rng.within(CAM_MAX);
        check([x, y, z], &mut tried, &mut inside);
        // The side planes: f * x = +-g * z, give or take one.
        let gx = 160;
        let edge = (i64::from(gx) * i64::from(z) / 100) as i32;
        for d in -1..=1 {
            if (edge + d).abs() < CAM_MAX {
                check([edge + d, y.clamp(-z, z), z], &mut tried, &mut inside);
                check([-(edge + d), y.clamp(-z, z), z], &mut tried, &mut inside);
            }
        }
        let gy = 120;
        let edge = (i64::from(gy) * i64::from(z) / 100) as i32;
        for d in -1..=1 {
            if (edge + d).abs() < CAM_MAX {
                check([x.clamp(-z, z), edge + d, z], &mut tried, &mut inside);
            }
        }
        check([x / 4096, y / 4096, 63 + (z & 3)], &mut tried, &mut inside);
    }
    assert!(inside > 10_000, "{inside} of {tried} corners inside");
}

/// The largest camera-space value (each way) a picture can make, from
/// the world's sizes: a drawn corner lies within the order table's
/// reach of the camera's block, a road corner or a thing on the land,
/// the eye above the land; turning keeps lengths (each matrix row is at
/// most 16384 * (1 + 1/4096) long), so each camera-space value is at
/// most |dx| + |dy| + |dz|.
#[test]
fn the_picture_stays_far_inside_the_corner_limit() {
    let each_way = i64::from((REACH + 2) * BLOCK);
    let land = i64::from(CELLS_X.max(CELLS_Z) as i32 * 2048);
    let flat = each_way.max(land);
    let up = 2 * (255 * 64 + 64 * 256);
    let most = 2 * flat + up;
    assert!(
        most * 4 < i64::from(CAM_MAX),
        "camera values up to {most}, limit {CAM_MAX}"
    );
}

#[test]
fn the_light_step_in_32_bits_is_the_old_one() {
    let mut n = [0u32; 3];
    let mut both = |a: [i32; 3], b: [i32; 3], c: [i32; 3]| {
        let want = light_step_old(a, b, c);
        assert_eq!(light_step(a, b, c), want, "{a:?} {b:?} {c:?}");
        n[usize::from(want)] += 1;
    };
    // Every land triangle as the picture draws it: fine cells and
    // coarse blocks, on the land and past its edges (heights clamped).
    for cj in -4..CELLS_Z as i32 + 4 {
        for ci in -4..CELLS_X as i32 + 4 {
            let inside = (0..CELLS_X as i32).contains(&ci) && (0..CELLS_Z as i32).contains(&cj);
            let byte = inside.then(|| LAND.cell_byte(ci as usize, cj as usize));
            let diag = byte.is_some_and(|b| b & DIAG_BIT != 0);
            for t in 0..2 {
                for step in [1, 2] {
                    if step == 2 && (ci % 2 != 0 || cj % 2 != 0) {
                        continue;
                    }
                    let w = tri_corners(diag, t)
                        .map(|(dx, dz)| corner_world(ci + step * dx as i32, cj + step * dz as i32));
                    both(w[0], w[1], w[2]);
                    both(w[0], w[2], w[1]);
                }
            }
        }
    }
    // Random triangles with every side inside the limit.
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..300_000 {
        let a = [
            rng.within(1 << 20),
            rng.within(1 << 14),
            rng.within(1 << 20),
        ];
        let half = LIGHT_SIDE_MAX / 2;
        let mut side = || [rng.within(half), rng.within(half), rng.within(half)];
        let (u, v) = (side(), side());
        let b = [a[0] + u[0], a[1] + u[1], a[2] + u[2]];
        let c = [a[0] + v[0], a[1] + v[1], a[2] + v[2]];
        both(a, b, c);
    }
    assert!(n.iter().all(|&k| k > 100), "light steps seen {n:?}");
}

/// Every side of a drawn triangle is inside the light step's limit:
/// land triangles by the land's sizes, road pieces from the road.
#[test]
fn the_light_tables_are_light_step() {
    // In the picture's corner order: `tri_corners(diag, t)` from the
    // cell's corner (fine), or twice as far from the block's (coarse).
    let mut steps = [0u32; 3];
    for size in [1usize, 2] {
        let (nx, nz) = (CELLS_X / size, CELLS_Z / size);
        for q in 0..nx * nz {
            let (bi, bj) = (q % nx, q / nx);
            let (i0, j0) = (bi * size, bj * size);
            let diag = LAND.cell_byte(i0, j0) & DIAG_BIT != 0;
            for t in 0..2 {
                let w = tri_corners(diag, t)
                    .map(|(dx, dz)| corner_world((i0 + size * dx) as i32, (j0 + size * dz) as i32));
                let want = light_step(w[0], w[1], w[2]);
                let got = if size == 1 {
                    fine_light(bi, bj, t)
                } else {
                    coarse_light(bi, bj, t)
                };
                assert_eq!(got, want, "size {size} at {bi} {bj} triangle {t}");
                steps[usize::from(want)] += 1;
            }
        }
    }
    // All three steps occur, so a table stuck at one value would fail.
    assert!(steps.iter().all(|&n| n > 0), "{steps:?}");
}

#[test]
fn every_drawn_side_is_inside_the_light_limit() {
    // Land: at most two cells across and 255 height steps of 64 up.
    const { assert!(2 * 2048 < LIGHT_SIDE_MAX && 255 * 64 < LIGHT_SIDE_MAX) };
    // Road pieces (`ribbon_corners`, units): x and z from the road
    // alone; y is the road's or the land's height (0 to 255 steps)
    // plus the lift.
    let mul = |a: i32, b: i32| (i64::from(a) * i64::from(b) / 16_384) as i32;
    let edge = |i: usize, side: i32| {
        let a = ROAD.heading[i];
        let hw = half_width(i) * side;
        [
            (ROAD.x[i] + mul(hw, cos_fine(a))) / SUB_PER_UNIT,
            (ROAD.z[i] - mul(hw, sin_fine(a))) / SUB_PER_UNIT,
        ]
    };
    let mut most = 0;
    for join in [1usize, 2, 3, 4] {
        for q in (0..(POINTS - 1) / 2).step_by(join) {
            let (i0, i1) = (2 * q, (2 * (q + join)).min(POINTS - 1));
            let c = [edge(i0, -1), edge(i0, 1), edge(i1, 1), edge(i1, -1)];
            for p in c {
                for r in c {
                    most = most.max((p[0] - r[0]).abs()).max((p[1] - r[1]).abs());
                }
            }
        }
    }
    assert!(most * 2 < LIGHT_SIDE_MAX, "road piece side {most}");
    let (lo, hi) = (ROAD.y.iter().min().copied(), ROAD.y.iter().max().copied());
    let (lo, hi) = (lo.unwrap_or(0).min(0), hi.unwrap_or(0).max(255 * 1024));
    let rise = (hi + LIFT) / SUB_PER_UNIT - lo / SUB_PER_UNIT;
    assert!(rise * 2 < LIGHT_SIDE_MAX, "road rise {rise}");
}

#[test]
fn seen_is_game_3ds_in_view() {
    let mut rng = Rng(0x0bad_cafe_dead_beef);
    let mut yes = 0u32;
    let mut tried = 0u32;
    for _ in 0..4_000 {
        // A turned camera somewhere over (or beyond) the land.
        let rot = Mat3::rotation_fine(
            rng.next() as u16,
            rng.next() as u16,
            (rng.next() as u16) >> 4,
        );
        let cam = Camera {
            pos: [
                rng.within(1 << 18),
                rng.within(1 << 15),
                rng.within(1 << 18),
            ],
            rot,
        };
        let far = 64 + rng.within(1 << 16).abs();
        for _ in 0..60 {
            let r = rng.within(1 << 14).abs();
            // A ball near the camera, often near a plane: built in
            // camera space, then turned back into the world.
            let z = rng.within(far + 4 * r + 256);
            let x = match rng.next() % 3 {
                0 => rng.within(1 << 16),
                _ => z * 4 / 5 + rng.within(r + 64),
            };
            let y = match rng.next() % 3 {
                0 => rng.within(1 << 16),
                1 => -(z * 3 / 5) + rng.within(r + 64),
                _ => z * 3 / 5 + rng.within(r + 64),
            };
            let x = if rng.next() & 1 == 0 { x } else { -x };
            let back = rot.transposed().apply_with::<game_3d::Checked>([x, y, z]);
            let c = [
                cam.pos[0] + back[0],
                cam.pos[1] + back[1],
                cam.pos[2] + back[2],
            ];
            let want = in_view::<D3>(&TIGHT, &cam, c, r, far).is_some();
            assert_eq!(seen(&cam, c, r, far), want, "{cam:?} {c:?} r {r} far {far}");
            yes += u32::from(want);
            tried += 1;
            // And anywhere at all within the range.
            let c = [
                rng.within(1 << 20),
                rng.within(1 << 17),
                rng.within(1 << 20),
            ];
            let want = in_view::<D3>(&TIGHT, &cam, c, r, far).is_some();
            assert_eq!(seen(&cam, c, r, far), want, "{cam:?} {c:?} r {r} far {far}");
        }
    }
    assert!(
        yes > tried / 10 && yes < tried * 9 / 10,
        "{yes} of {tried} seen"
    );
}

#[test]
fn the_block_table_is_the_corners() {
    // On the land, the ring round it, and far past it each way (the
    // order table reaches 15 blocks from the camera's block).
    let (bx, bz) = (CELLS_X as i32 / 2, CELLS_Z as i32 / 2);
    for bj in -60..bz + 60 {
        for bi in -60..bx + 60 {
            assert_eq!(
                block_lo_hi(bi, bj),
                block_lo_hi_slow(bi, bj),
                "block {bi} {bj}"
            );
        }
    }
}

#[test]
fn the_walks_first_test_never_drops_a_seen_block() {
    let mut rng = Rng(0x0f0e_0d0c_0b0a_0908);
    let (mut out, mut tried) = (0u32, 0u32);
    for n in 0..3_000u32 {
        // Half the cameras turned any way, half like the chase camera
        // (little pitch and roll).
        let rot = if n % 2 == 0 {
            Mat3::rotation_fine(rng.next() as u16, rng.next() as u16, rng.next() as u16)
        } else {
            Mat3::rotation_fine(
                rng.next() as u16,
                (rng.within(1 << 12)) as u16,
                (rng.within(1 << 9)) as u16,
            )
        };
        // Over the land (32 x 48 cells of 2048) and round it, from
        // under the lowest land to far above the highest.
        let pos = [
            (1 << 15) + rng.within(1 << 17),
            rng.within(1 << 15),
            3 * (1 << 14) + rng.within(1 << 17),
        ];
        let cam = Camera { pos, rot };
        // The three levels' far edges, and now and then any other.
        let far = match n % 4 {
            0 => 64 * 256,
            1 => 144 * 256,
            2 => 240 * 256,
            _ => 64 + rng.within(1 << 16).abs(),
        };
        let cb = [pos[0].div_euclid(BLOCK), pos[2].div_euclid(BLOCK)];
        let ahead = Ahead::new(&cam, cb, far);
        for &[di, dj] in ORDER.iter() {
            let (di, dj) = (i32::from(di), i32::from(dj));
            assert!(di.abs() <= REACH && dj.abs() <= REACH);
            let (bi, bj) = (cb[0] + di, cb[1] + dj);
            let (lo, hi) = block_lo_hi(bi, bj);
            let c = [
                bi * BLOCK + BLOCK / 2,
                (lo + hi) / 2,
                bj * BLOCK + BLOCK / 2,
            ];
            let r = 2_897 + (hi - lo) / 2 + 1;
            tried += 1;
            if ahead.out(di, dj) {
                out += 1;
                assert!(
                    !seen(&cam, c, r, far),
                    "{cam:?} far {far} block {bi} {bj} dropped but seen"
                );
            }
        }
    }
    // And it is worth having: it drops a good share of the blocks.
    println!("the walk's first test dropped {out} of {tried} blocks");
    assert!(out > tried / 5, "{out} of {tried} dropped");
}

/// `nearest_point` as it was before the speed work (64-bit differences,
/// checked sums), copied.
fn nearest_point_old(road: &Road, x: i32, z: i32) -> (usize, i64) {
    let d2 = |k: usize| {
        let dx = x as i64 - road.x[k] as i64;
        let dz = z as i64 - road.z[k] as i64;
        dx * dx + dz * dz
    };
    let mut best = 0usize;
    let mut best_d = i64::MAX;
    let mut k = 0usize;
    while k < POINTS {
        let d = d2(k);
        if d < best_d {
            best_d = d;
            best = k;
        }
        k += 4;
    }
    let lo = best.saturating_sub(4);
    let hi = if best + 4 < POINTS {
        best + 4
    } else {
        POINTS - 1
    };
    let mut k = lo;
    while k <= hi {
        let d = d2(k);
        if d < best_d {
            best_d = d;
            best = k;
        }
        k += 1;
    }
    (best, best_d)
}

/// The old scan in 128 bits: None where the old one's checked 64-bit
/// sums run over (it stops the game there) before it ends.
fn nearest_point_wide(road: &Road, x: i32, z: i32) -> Option<(usize, i64)> {
    let d2 = |k: usize| {
        let dx = i128::from(x) - i128::from(road.x[k]);
        let dz = i128::from(z) - i128::from(road.z[k]);
        i64::try_from(dx * dx).ok()?;
        i64::try_from(dx * dx + dz * dz).ok()
    };
    let mut best = 0usize;
    let mut best_d = i64::MAX;
    let mut k = 0usize;
    while k < POINTS {
        let d = d2(k)?;
        if d < best_d {
            best_d = d;
            best = k;
        }
        k += 4;
    }
    let lo = best.saturating_sub(4);
    let hi = (best + 4).min(POINTS - 1);
    for k in lo..=hi {
        let d = d2(k)?;
        if d < best_d {
            best_d = d;
            best = k;
        }
    }
    Some((best, best_d))
}

#[test]
fn the_fast_nearest_point_is_the_old_one() {
    let mut overflows = 0u32;
    let mut check = |x: i32, z: i32| {
        let Some(want) = nearest_point_wide(&ROAD, x, z) else {
            // The old sums run over: the old code stops the game. The
            // new one takes the very same code there (the slow way,
            // unchanged), since the fast way never runs over.
            assert!(
                !(-FAST_REACH..=FAST_REACH).contains(&x)
                    || !(-FAST_REACH..=FAST_REACH).contains(&z),
                "overflow inside the fast reach at {x} {z}"
            );
            overflows += 1;
            return;
        };
        assert_eq!(nearest_point_old(&ROAD, x, z), want, "old at {x} {z}");
        assert_eq!(nearest_point(&ROAD, x, z), want, "at {x} {z}");
        assert_eq!(nearest_point_slow(&ROAD, x, z), want, "slow at {x} {z}");
    };
    // A half-metre grid over the land and 64 m round it (odd offsets,
    // so it does not sit on the road's own grid).
    let (w, d) = (CELLS_X as i32 * CELL, CELLS_Z as i32 * CELL);
    let mut z = -64 * M + 7;
    while z < d + 64 * M {
        let mut x = -64 * M + 3;
        while x < w + 64 * M {
            check(x, z);
            x += M / 2;
        }
        z += M / 2;
    }
    // Every road point, and the middle of every two points less than 9
    // apart (equal distances: the first one found must win).
    for a in 0..POINTS {
        check(ROAD.x[a], ROAD.z[a]);
        for b in a + 1..POINTS.min(a + 9) {
            let (sx, sz) = (ROAD.x[a] + ROAD.x[b], ROAD.z[a] + ROAD.z[b]);
            for (ox, oz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                check((sx + ox).div_euclid(2), (sz + oz).div_euclid(2));
            }
        }
    }
    // Anywhere in the `i32` range, the fast reach's edges and the
    // corners.
    let mut rng = Rng(0x7777_1234_abcd_0001);
    for _ in 0..100_000 {
        let r = rng.next();
        check((r >> 32) as i32, r as i32);
    }
    for v in [
        FAST_REACH - 1,
        FAST_REACH,
        FAST_REACH + 1,
        i32::MAX,
        i32::MIN,
    ] {
        for u in [0, v.saturating_neg(), v] {
            check(v, u);
            check(u, v);
            check(v.saturating_neg(), u);
        }
    }
    println!("{overflows} points where the old sums run over (the slow way)");
}

/// The far hills as `draw.rs` drew them before the speed work: the
/// column angles, the hill shape and one `vline` a column, copied.
fn hills_old(f: &mut Frame, yaw: u16, yh: i32) {
    let tri_wave = |a: u16| 256 - (i32::from(a) - 32_768).abs() / 128;
    let hill = |a: u16| {
        let h1 = tri_wave(a.wrapping_mul(2).wrapping_add(9_000));
        let h2 = tri_wave(a.wrapping_mul(5).wrapping_add(21_000));
        let h3 = tri_wave(a.wrapping_mul(11).wrapping_add(4_000));
        1 + (h1 * 6 + h2 * 3 + h3) / 256
    };
    if yh > 0 {
        for x in 0..W {
            let t = ((2 * x as i64 + 1 - 160) * 65_536) / 200;
            let t2 = t * t / 65_536;
            let a = t * (63_728 - 12_576 * t2 / 65_536) / 65_536;
            let da = (a * 10_430 / 65_536) as i16;
            let a = yaw.wrapping_add(da as u16);
            let h = hill(a).min(yh);
            f.vline(x as i16, (yh - h) as i16, h as u16, col::HILLS);
        }
    }
}

#[test]
fn the_hills_written_straight_are_the_vline_ones() {
    let mut a = Box::new(Frame([7; W * H]));
    let mut b = Box::new(Frame([7; W * H]));
    // Every yaw at three horizons, and every horizon at many yaws.
    let mut tried = 0u32;
    for yaw in 0..=u16::MAX {
        let some = if yaw % 97 == 0 {
            (0..=H as i32).collect::<Vec<_>>()
        } else {
            vec![1, H as i32 / 2, H as i32]
        };
        for yh in some {
            a.0.fill(7);
            b.0.fill(7);
            hills(&mut a, yaw, yh);
            hills_old(&mut b, yaw, yh);
            assert!(a.0 == b.0, "yaw {yaw} horizon {yh}");
            tried += 1;
        }
    }
    assert!(tried > 3 * 65_536);
}

#[test]
fn polygons_with_no_rows_paint_nothing() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut f = Box::new(Frame([7; W * H]));
    let area = VIEW.area;
    let (mut empty, mut tried) = (0u32, 0u32);
    for _ in 0..300_000 {
        // Corners on and around the picture, often near one row's
        // middle (rows sit at 16 * row + 8 in 1/16 pixel).
        let row = rng.within(H as i32 + 4).abs() - 2;
        let n = 3 + (rng.next() % 4) as usize;
        let mut pts = [Pt {
            c: [0, 0, 100],
            out: 0,
            s: [0, 0],
        }; 6];
        for p in pts.iter_mut().take(n) {
            let y = match rng.next() % 3 {
                0 => 16 * row + 8 + rng.within(10),
                1 => 16 * row + rng.within(17).abs(),
                _ => rng.within(16 * (H as i32 + 8)),
            };
            p.s = [rng.within(16 * (W as i32 + 40)), y];
        }
        if rng.next().is_multiple_of(50) {
            pts[0].out = 1;
        }
        let refs: Vec<&Pt> = pts[..n].iter().collect();
        tried += 1;
        if !no_rows(&refs) {
            continue;
        }
        empty += 1;
        assert!(refs.iter().all(|p| p.out == 0));
        for k in 1..n - 1 {
            let tri = [refs[0].s, refs[k].s, refs[k + 1].s];
            let done = fill_tri_counted::<Checked>(&mut f, &area, &tri, 9);
            assert_eq!((done.pixels, done.rows), (0, 0), "{tri:?}");
        }
    }
    assert!(f.0.iter().all(|&b| b == 7), "a pixel was painted");
    assert!(
        empty > tried / 20,
        "{empty} of {tried} polygons with no rows"
    );
}
