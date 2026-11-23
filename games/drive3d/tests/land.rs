//! The stretch (the spec's section 3): road walk, bake, things.

use drive3d::land::{Ground, LAND, Stretch, bake};
use drive3d::road::{PIECES, POINTS, ROAD, walk};
use drive3d::things::{self, THINGS, gentle, kind};
use drive3d::tune::{CELL, CELLS_X, CELLS_Z, CORNERS_X, CORNERS_Z, GRAVEL, HEIGHT_STEP, M};

#[test]
fn the_const_bake_and_the_plain_run_give_the_same_bytes() {
    // The statics were made while compiling; the same functions run
    // now as plain code.
    assert_eq!(walk(), ROAD);
    assert_eq!(bake(), LAND);
    assert_eq!(things::make(&walk()), THINGS);
    // 33 * 49 = 1,617 bytes of heights and 32 * 48 = 1,536 cell bytes
    // (the spec's 3,153 B).
    assert_eq!(LAND.h.len() + LAND.cell.len(), 3_153);
    assert_eq!((CORNERS_X, CORNERS_Z, CELLS_X, CELLS_Z), (33, 49, 32, 48));
}

#[test]
fn the_road_walk_matches_the_records() {
    // 344 m in 2 m steps: 172 steps, 173 points.
    assert_eq!(POINTS, 173);
    let len: u32 = PIECES.iter().map(|p| u32::from(p.len)).sum();
    assert_eq!(len * 2, 344);
    // Heights: up 6 m on the hill, -0.75 m over the crest, -5 m on the
    // drop, +1 m in the first corner: the end is 8 + 6 - 0.75 - 5 + 1 =
    // 9.25 m.
    assert_eq!(ROAD.y[0], 8 * M);
    assert_eq!(ROAD.y[POINTS - 1], 8 * M + 6 * M - 3 * M / 4 - 5 * M + M);
    // Turns: +43 and -64 of 256 turns: the end heading is -21 / 256 of
    // a turn (-29.5 degrees): -21 * 256 = -5,376.
    assert_eq!(i32::from(ROAD.heading[POINTS - 1] as i16), -5_376);
    // Every step is 2 m long (within rounding: 1/16 sub sums).
    for k in 1..POINTS {
        let dx = i64::from(ROAD.x[k] - ROAD.x[k - 1]);
        let dz = i64::from(ROAD.z[k] - ROAD.z[k - 1]);
        let d = drive3d::maths::len2(dx as i32, dz as i32);
        assert!((d - 2 * M).abs() <= 4, "step {k}: {d}");
    }
    // The road stays inside the block, 8 m from its edges.
    for k in 0..POINTS {
        assert!((8 * M..=CELLS_X as i32 * CELL - 8 * M).contains(&ROAD.x[k]));
        assert!((8 * M..=CELLS_Z as i32 * CELL - 8 * M).contains(&ROAD.z[k]));
    }
}

#[test]
fn the_road_line_is_its_pieces_surface_all_along_gravel_but_the_ford() {
    // Looked at every 1/4 m along the line between the points (the
    // bake marks every 1/8 m, so no triangle the line crosses is left).
    let mut gravel = 0;
    for k in 0..POINTS - 1 {
        let want = PIECES[usize::from(ROAD.piece[k + 1])].surface;
        for t in 0..8 {
            let x = ROAD.x[k] + (ROAD.x[k + 1] - ROAD.x[k]) * t / 8;
            let z = ROAD.z[k] + (ROAD.z[k + 1] - ROAD.z[k]) * t / 8;
            let s = Stretch.at(x, z).surface;
            if s != want {
                // A point exactly on a piece's first point belongs to
                // both pieces.
                let other = PIECES[usize::from(ROAD.piece[k])].surface;
                assert!(t == 0 && s == other, "point {k} + {t}/8: {s} != {want}");
            }
            if s == GRAVEL {
                gravel += 1;
            }
        }
    }
    // Only the ford piece (24 m = 12 steps = 96 looks) is not gravel.
    assert!(gravel >= 172 * 8 - 96 - 1, "{gravel}");
    for p in PIECES.iter() {
        if p.note != drive3d::road::note::FORD {
            assert_eq!(p.surface, GRAVEL);
        }
    }
}

#[test]
fn no_road_point_is_more_than_a_quarter_metre_from_the_land() {
    let mut worst = 0;
    for k in 0..POINTS - 1 {
        for t in 0..4 {
            let x = ROAD.x[k] + (ROAD.x[k + 1] - ROAD.x[k]) * t / 4;
            let z = ROAD.z[k] + (ROAD.z[k + 1] - ROAD.z[k]) * t / 4;
            let y = ROAD.y[k] + (ROAD.y[k + 1] - ROAD.y[k]) * t / 4;
            let gap = (Stretch.at(x, z).height - y).abs();
            worst = worst.max(gap);
        }
    }
    assert!(worst <= M / 4, "worst gap {worst} subs");
    println!(
        "worst road-land gap: {worst} subs ({} mm)",
        worst * 1000 / M
    );
}

#[test]
fn the_land_is_the_corner_heights_at_the_corners_and_has_no_steps() {
    // At a corner the plane is the corner's height (1/4 m * 1,024).
    for j in 0..CELLS_Z {
        for i in 0..CELLS_X {
            let s = LAND.at(i as i32 * CELL, j as i32 * CELL);
            assert_eq!(s.height, i32::from(LAND.corner(i, j)) * HEIGHT_STEP);
        }
    }
    // One sub apart anywhere (along x and along z, across cell edges and
    // diagonals), the height changes by at most the steepest slope: a
    // whole corner range (255 steps) over a cell is 255 * 1024 / 32768
    // = 8 subs a sub; the land here is far gentler. 1 sub is enough to
    // show a step (a crack).
    let step = M / 2;
    let mut z = 0;
    while z < CELLS_Z as i32 * CELL - 1 {
        let mut x = 0;
        while x < CELLS_X as i32 * CELL - 1 {
            let h = LAND.at(x, z).height;
            for (dx, dz) in [(1, 0), (0, 1)] {
                let h2 = LAND.at(x + dx, z + dz).height;
                assert!((h2 - h).abs() <= 2, "step at ({x}, {z})");
            }
            x += step;
        }
        z += step;
    }
}

#[test]
fn rocks_stand_only_on_gentle_cells() {
    let mut rocks = 0;
    for t in THINGS.iter() {
        if t.kind == kind::ROCK {
            rocks += 1;
            assert!(
                gentle(&LAND, usize::from(t.cx), usize::from(t.cz)),
                "rock in cell ({}, {}): range {:?}",
                t.cx,
                t.cz,
                LAND.cell_range(usize::from(t.cx), usize::from(t.cz))
            );
        }
    }
    assert!(rocks >= 3);
}

#[test]
fn no_solid_thing_stands_on_the_road() {
    // Solid things keep their circle off the road's width (3 m each
    // side of the line) so a car on the road only touches them at the
    // edge. The banner is not solid.
    for t in THINGS.iter() {
        let r = t.radius();
        if r == 0 {
            assert_eq!(t.kind, kind::BANNER);
            continue;
        }
        let (x, z) = t.at();
        let mut near = i64::MAX;
        for k in 0..POINTS {
            near = near.min(drive3d::maths::dist2((x, z), (ROAD.x[k], ROAD.z[k])));
        }
        let d = game_3d::isqrt(near as u64) as i32;
        assert!(
            d - r >= 3 * M,
            "thing at ({x}, {z}) {} m from the road",
            d / M
        );
    }
    assert!((25..=35).contains(&THINGS.len()));
}
