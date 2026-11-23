//! The test island's records and collision (PROTOTYPE-SPEC.md, section 3,
//! the rule tests; the picture tests are the drawing's).

mod common;

use common::*;
use proto3d::island::*;
use proto3d::maths::cm;
use proto3d::play::{Detail, Play};
use proto3d::tune::{HALF, M};

#[test]
fn every_top_on_the_island_is_reached_in_a_scripted_run() {
    let mut play = Play::on(&ISLAND, Detail::High);
    let mut seen = 0u32;
    follow(&mut play, &ROUTE, 400, |p| {
        if let Some(g) = p.hero.ground {
            seen |= 1 << g;
        }
        assert_eq!(p.hero.frozen, 0, "fell off at {:?}", p.hero.pos);
    });
    assert_eq!(seen, (1 << BLOCKS.len()) - 1, "tops reached: {seen:b}");
    assert_eq!(play.hero.falls, 0);
}

#[test]
fn the_records_are_16_8_and_6_bytes() {
    assert_eq!(size_of::<Block>(), 16);
    assert_eq!(size_of::<Prop>(), 8);
    assert_eq!(size_of::<Coin>(), 6);
}

#[test]
fn the_layout_is_the_specs_in_whole_cm() {
    let b = |i: u8| BLOCKS[usize::from(i)];
    // Main island, x and z from -7 to 7 m, top 0, 0.6 m skirt.
    assert_eq!((b(MAIN).lo, b(MAIN).hi), ([-700, -60, -700], [700, 0, 700]));
    assert_eq!(
        (b(TERRACE).lo, b(TERRACE).hi),
        ([-700, 0, 150], [-150, 100, 700])
    );
    assert_eq!(
        (b(RAMP).lo, b(RAMP).hi),
        ([-450, 0, -150], [-250, 100, 150])
    );
    assert_eq!(b(RAMP).ramp, ramp::UP_Z);
    assert_eq!((b(STEP).lo, b(STEP).hi), ([100, 0, -100], [300, 50, 100]));
    assert_eq!(
        (b(FLOAT).lo, b(FLOAT).hi),
        ([200, 110, 300], [360, 150, 460])
    );
    assert_eq!(
        (b(ISLET).lo, b(ISLET).hi),
        ([-200, -40, 950], [200, 0, 1350])
    );
    // The gap: 2.5 m of open sky.
    assert_eq!(b(ISLET).lo[2] - b(MAIN).hi[2], 250);
    assert_eq!(ISLAND.start, [50, 0, -320]);
    // Every top can be stood on.
    assert!(BLOCKS.iter().all(|b| b.flags & flag::WALK != 0));
    assert!(b(FLOAT).flags & flag::PATCH != 0);
    // 1 m is 4096 subs; a cm is 40.96 (rounded down).
    assert_eq!(cm(100), M);
    assert_eq!(cm(-700), -7 * M);
}

#[test]
fn the_props_and_collectibles_per_detail_are_the_comparisons() {
    let count = |kind: u8, d: u8| {
        PROPS
            .iter()
            .filter(|p| p.what() == kind && p.min_detail() <= d)
            .count()
    };
    assert_eq!([0, 1, 2].map(|d| count(prop::TREE, d)), [5, 6, 8]);
    assert_eq!([0, 1, 2].map(|d| count(prop::BUSH, d)), [0, 5, 8]);
    assert_eq!([0, 1, 2].map(|d| count(prop::TUFTS, d)), [0, 0, 5]);
    assert_eq!([0, 1, 2].map(|d| count(prop::FAR_ISLET, d)), [0, 2, 3]);
    assert_eq!(COINS.len(), 6);
    assert_eq!(ISLAND.coins_low, 4);
    // Every collectible is over a top or the gap, within the island.
    for c in COINS {
        let p = c.pos();
        assert!(p[0].abs() <= 7 * M && p[2] >= -7 * M && p[2] <= cm(1350));
    }
}

#[test]
fn the_ramp_rises_along_z_from_0_to_1_m() {
    let r = &BLOCKS[usize::from(RAMP)];
    let x = cm(-350);
    assert_eq!(r.surface(x, cm(-150)), 0);
    assert_eq!(r.surface(x, cm(150)), M);
    assert_eq!(r.surface(x, 0), M / 2);
    // Beside the ramp: the height at its nearest edge.
    assert_eq!(r.surface(x, cm(-400)), 0);
}

#[test]
fn the_highest_top_under_a_point_is_found() {
    // Over the step: the step, not the main top under it.
    assert_eq!(top_under(&BLOCKS, cm(200), 0, M), Some((STEP, cm(50))));
    // Below the step's top: the main top.
    assert_eq!(top_under(&BLOCKS, cm(200), 0, cm(40)), Some((MAIN, 0)));
    // Over the gap: nothing.
    assert_eq!(top_under(&BLOCKS, 0, cm(800), M), None);
}

#[test]
fn a_ray_finds_the_first_block_it_enters() {
    // Straight down onto the step from 3 m.
    let a = [cm(200), cm(300), 0];
    let b = [cm(200), cm(-100), 0];
    let (f, i) = first_hit(&BLOCKS, a, b).unwrap();
    assert_eq!(i, STEP);
    // 2.5 m of the 4 m way down: 5/8 of 65536.
    assert!((i64::from(f) - 40_960).abs() <= 2, "f {f}");
    // Up the ramp's slope: the slope, not the box round it.
    let a = [cm(-350), cm(90), cm(-300)];
    let b = [cm(-350), cm(90), cm(300)];
    let (f, i) = first_hit(&BLOCKS, a, b).unwrap();
    assert_eq!(i, RAMP);
    // The slope is 0.9 m high at z = 1.2 m: 4.2 / 6 of the way (the
    // terrace's side, at z = 1.5 m, comes after it).
    assert!((i64::from(f) - 45_875).abs() <= 64, "f {f}");
    // Over everything: no hit.
    assert_eq!(first_hit(&BLOCKS, [0, 4 * M, 0], [M, 4 * M, M]), None);
}

#[test]
fn the_body_does_not_fit_under_the_floating_block() {
    let b = BLOCKS[usize::from(FLOAT)].bounds();
    let mid = [(b.lo[0] + b.hi[0]) / 2, 0, (b.lo[2] + b.hi[2]) / 2];
    assert!(!body_fits(&BLOCKS, mid));
    assert!(body_fits(&BLOCKS, [mid[0], b.hi[1], mid[2]]));
    // Next to it, a half width away, it fits.
    assert!(body_fits(&BLOCKS, [b.lo[0] - HALF, 0, mid[2]]));
}
