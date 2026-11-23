//! Things to collect (PROTOTYPE-SPEC.md, section 4, tests).

mod common;

use common::*;
use game_kit::button;
use proto3d::collect::Coins;
use proto3d::island::{COIN_GAP, COIN_ISLET, ISLAND, ISLET, MAIN};
use proto3d::maths::cm;
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::{BOB, BOB_STEPS, SPIN};

#[test]
fn each_collectible_is_taken_once_and_only_once() {
    for (d, total) in [(Detail::Low, 4), (Detail::Mid, 6), (Detail::High, 6)] {
        let mut play = Play::on(&ISLAND, d);
        let mut took = [0u32; 6];
        follow(&mut play, &ROUTE, 400, |p| {
            if let Some(i) = p.events.took {
                took[usize::from(i)] += 1;
            }
            // The counter matches the taken bits at every step.
            let (n, t) = p.count();
            let bits = (p.taken() & ((1u16 << t) - 1) as u8).count_ones();
            assert_eq!(u32::from(n), bits);
            assert_eq!(t, total);
        });
        // Run the route back and forth once more: nothing is taken again.
        let back = [
            leg([0, 1100], 0.5, None, ISLET),
            leg([0, 1300], 0.5, None, ISLET),
        ];
        follow(&mut play, &back, 400, |p| assert_eq!(p.events.took, None));
        let want: Vec<u32> = (0..6).map(|i| u32::from(i < total)).collect();
        assert_eq!(took.to_vec(), want, "{d:?}");
        assert_eq!(play.count(), (total, total));
    }
}

#[test]
fn at_low_only_the_first_four_exist() {
    let play = Play::on(&ISLAND, Detail::Low);
    assert_eq!(play.count(), (0, 4));
    assert!(play.coin_exists(3) && !play.coin_exists(4) && !play.coin_exists(5));
}

#[test]
fn the_gap_collectible_is_taken_by_walking_off_never() {
    // Walk off the main island's edge at full speed (no jump), anywhere
    // along it, at the camera's start yaw.
    for x in (-180..=180).step_by(30) {
        let mut play = Play::new();
        play.hero = hero_at(x, 0, 500, MAIN);
        play.cam.snap(&play.hero, ISLAND.blocks, ISLAND.props, 1);
        for _ in 0..150 {
            play.step(&pad(stick_towards(0.0, 1.0, play.cam.yaw, 1.0), 0));
            assert_eq!(play.taken() & (1 << COIN_GAP), 0, "x {x}");
            if play.hero.frozen > 0 {
                break;
            }
        }
        assert_eq!(play.hero.falls, 1);
    }
}

#[test]
fn the_gap_collectible_is_not_taken_from_either_edge() {
    // A full jump straight up at the main island's edge and at the
    // islet's edge, the square right at each edge.
    for (z, top) in [(700 + 24, MAIN), (950 - 24, ISLET)] {
        let mut play = Play::new();
        play.hero = hero_at(0, 0, z, top);
        play.cam.snap(&play.hero, ISLAND.blocks, ISLAND.props, 1);
        play.step(&jump_pad((0, 0), 0));
        for _ in 0..30 {
            play.step(&pad((0, 0), button::A));
        }
        assert_eq!(play.hero.ground, Some(top));
        assert_eq!(play.taken() & (1 << COIN_GAP), 0, "from z {z}");
    }
}

#[test]
fn the_gap_collectible_is_taken_in_the_jump_across() {
    // Within its reach on x (0.25 + 0.4 m of its middle).
    for x in [-60, 0, 60] {
        let mut play = Play::new();
        play.hero = hero_at(x, 0, 450, MAIN);
        play.cam.snap(&play.hero, ISLAND.blocks, ISLAND.props, 1);
        let mut pressed = false;
        for _ in 0..60 {
            let s = stick_towards(0.0, 1.0, play.cam.yaw, 1.0);
            let p = if !pressed && play.hero.pos[2] >= cm(700) {
                pressed = true;
                jump_pad(s, 0)
            } else {
                pad(s, button::A)
            };
            play.step(&p);
            if play.hero.ground == Some(ISLET) {
                break;
            }
        }
        assert_eq!(play.hero.ground, Some(ISLET));
        assert_ne!(play.taken() & (1 << COIN_GAP), 0, "x {x}");
        // The islet's own is further on, not taken yet.
        assert_eq!(play.taken() & (1 << COIN_ISLET), 0);
    }
}

#[test]
fn collectibles_spin_and_bob_5_cm() {
    let mut c = Coins::new();
    let h = hero_at(0, 0, 0, MAIN);
    let mut lo = 0;
    let mut hi = 0;
    for _ in 0..BOB_STEPS {
        c.step(&h, &ISLAND, 1);
        lo = lo.min(c.bob());
        hi = hi.max(c.bob());
    }
    assert_eq!(c.spin, (SPIN as u32 * u32::from(BOB_STEPS)) as u16);
    assert_eq!(c.bob_step, 0, "one whole bob in BOB_STEPS");
    assert!(hi <= BOB && lo >= -BOB);
    assert!(hi >= BOB - 2 && lo <= -BOB + 2, "{lo} {hi}");
}

#[test]
fn a_frozen_hero_takes_nothing() {
    let mut play = Play::new();
    play.hero = hero_at(0, 0, 1150, ISLET);
    play.hero.frozen = 5;
    play.step(&Pad::NONE);
    assert_eq!(play.taken(), 0);
}
