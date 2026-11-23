//! The follow camera (PROTOTYPE-SPEC.md, section 2, tests).

mod common;

use common::*;
use game_kit::button;
use proto3d::camera::{Cam, FULL};
use proto3d::island::{BLOCKS, ISLAND, MAIN, dist2_to_segment, inside_any};
use proto3d::maths::{sub, unit};
use proto3d::play::{Detail, Play};
use proto3d::step::Pad;
use proto3d::tune::*;

fn still() -> Play {
    let mut play = Play::new();
    play.hero = hero_at(0, 0, -300, MAIN);
    play.cam.snap(&play.hero, &BLOCKS, ISLAND.props, 1);
    play
}

#[test]
fn the_aim_height_does_not_change_during_a_full_jump_on_flat_ground() {
    let mut play = still();
    play.step(&Pad::NONE);
    let y = play.aim()[1];
    assert_eq!(y, sub(AIM_UP));
    play.step(&jump_pad((0, 0), 0));
    let mut steps = 1;
    while play.hero.ground.is_none() || steps < 3 {
        play.step(&pad((0, 0), button::A));
        assert_eq!(play.aim()[1], y, "step {steps}");
        steps += 1;
        assert!(steps < 40);
    }
    assert!(steps >= 19);
}

#[test]
fn the_aim_follows_a_fall_past_hold() {
    let mut play = still();
    play.hero = hero_at(0, 0, 650, MAIN);
    play.cam.snap(&play.hero, &BLOCKS, ISLAND.props, 1);
    let y0 = play.aim()[1];
    // Walk off into the gap and fall.
    for _ in 0..25 {
        play.step(&pad(FWD, 0));
    }
    assert!(play.hero.pos[1] < -sub(HOLD));
    assert!(play.aim()[1] < y0);
}

/// Checks the eye after every step of a long random run.
#[test]
fn the_eye_is_never_inside_a_box_nor_nearer_than_dist_min() {
    let mut rng = Lcg(3);
    for d in [Detail::Low, Detail::High] {
        let mut play = Play::on(&ISLAND, d);
        let mut last = Pad::NONE;
        for n in 0..9_000 {
            last = rng.pad(&last);
            play.step(&last);
            let eye = play.eye();
            assert_eq!(inside_any(&BLOCKS, eye), None, "step {n}: eye {eye:?}");
            assert!(
                play.cam.dist() >= i64::from(sub(DIST_MIN)),
                "step {n}: distance {}",
                play.cam.dist()
            );
        }
    }
}

#[test]
fn walls_pull_the_eye_in() {
    // The hero stands against the terrace's side (z 1.5 m, 1 m high),
    // facing away from it, with the camera behind, over the terrace.
    let mut play = still();
    play.hero = hero_at(-600, 0, 125, MAIN);
    play.hero.yaw = 32_768;
    play.cam = Cam::new(32_768, 0);
    play.cam.snap(&play.hero, &BLOCKS, ISLAND.props, 1);
    let full = i64::from(sub(DIST));
    // Turn the pitch flat so the ray hits the terrace's side.
    for _ in 0..30 {
        play.step(&Pad {
            right: (0, -32_767),
            ..Pad::NONE
        });
    }
    assert_eq!(play.cam.pitch, PITCH_MIN);
    assert!(play.cam.dist() < full - i64::from(sub(WALL_GAP)) / 2);
    assert_eq!(inside_any(&BLOCKS, play.eye()), None);
}

#[test]
fn l_and_r_act_on_release_only() {
    let mut play = still();
    let y0 = play.cam.yaw;
    for _ in 0..20 {
        play.step(&pad((0, 0), button::L));
        assert_eq!(play.cam.yaw, y0, "a held L does nothing");
    }
    for _ in 0..SHOULDER_STEPS {
        play.step(&Pad::NONE);
    }
    assert_eq!(play.cam.yaw, y0.wrapping_sub(SHOULDER_TURN));
    for _ in 0..10 {
        play.step(&pad((0, 0), button::R));
    }
    assert_eq!(play.cam.yaw, y0.wrapping_sub(SHOULDER_TURN));
    for _ in 0..SHOULDER_STEPS {
        play.step(&Pad::NONE);
    }
    assert_eq!(play.cam.yaw, y0);
}

#[test]
fn l_and_r_together_put_the_camera_behind_the_hero() {
    let mut play = still();
    play.hero.yaw = 20_000;
    play.step(&pad((0, 0), button::L));
    play.step(&pad((0, 0), button::L | button::R));
    play.step(&pad((0, 0), button::R));
    for _ in 0..SHOULDER_STEPS {
        play.step(&Pad::NONE);
    }
    assert_eq!(play.cam.yaw, 20_000);
}

#[test]
fn a_full_turn_of_the_right_stick_returns_the_same_yaw() {
    // 32,768 steps of 546 is 273 whole turns.
    let mut cam = Cam::new(1_234, 0);
    let hero = hero_at(0, 0, 0, MAIN);
    let p = Pad {
        right: (32_767, 0),
        ..Pad::NONE
    };
    let mut seen_other = false;
    for _ in 0..32_768u32 {
        cam.step(&p, &hero, &BLOCKS, &[], 0);
        seen_other |= cam.yaw != 1_234;
    }
    assert!(seen_other);
    assert_eq!(cam.yaw, 1_234);
    // And right then left by the same amount, from any yaw.
    let mut cam = Cam::new(65_000, 0);
    for _ in 0..77 {
        cam.step(&p, &hero, &BLOCKS, &[], 0);
    }
    let left = Pad {
        right: (-32_767, 0),
        ..Pad::NONE
    };
    for _ in 0..77 {
        cam.step(&left, &hero, &BLOCKS, &[], 0);
    }
    assert_eq!(cam.yaw, 65_000);
}

#[test]
fn the_pitch_stays_between_its_limits() {
    let mut play = still();
    for _ in 0..60 {
        play.step(&Pad {
            right: (0, 32_767),
            ..Pad::NONE
        });
    }
    assert_eq!(play.cam.pitch, PITCH_MAX);
    for _ in 0..60 {
        play.step(&Pad {
            right: (0, -32_767),
            ..Pad::NONE
        });
    }
    assert_eq!(play.cam.pitch, PITCH_MIN);
}

#[test]
fn after_a_second_of_running_the_camera_turns_towards_the_run() {
    // Run 20 degrees to the right of the camera's view (inside AUTO_SPAN,
    // 30 degrees): nothing for AUTO_WAIT steps, then turns, at most
    // AUTO_MAX a step. (Was a quarter turn off; since the review a run
    // more than AUTO_SPAN off the view does not turn the camera, see
    // running_sideways_does_not_turn_the_camera.) On a big flat top, so
    // no box turns the run.
    static FLAT_BIG: [proto3d::island::Block; 1] = [flat([-3000, -50, -3000], [3000, 0, 3000])];
    let mut play = Play::on(world(&FLAT_BIG, [0, 0, 0]), Detail::Mid);
    let right = (11_207, -30_791);
    let y0 = play.cam.yaw;
    for _ in 0..AUTO_WAIT {
        play.step(&pad(right, 0));
        assert_eq!(play.cam.yaw, y0);
    }
    let mut last = play.cam.yaw;
    for _ in 0..20 {
        play.step(&pad(right, 0));
        let d = proto3d::maths::angle_diff(last, play.cam.yaw);
        assert!(d > 0 && d <= AUTO_MAX, "turn {d}");
        last = play.cam.yaw;
    }
}

#[test]
fn running_sideways_does_not_turn_the_camera() {
    let mut play = still();
    let y0 = play.cam.yaw;
    for _ in 0..150 {
        play.step(&pad((32_767, 0), 0));
    }
    assert_eq!(play.cam.yaw, y0);
}

#[test]
fn running_at_the_camera_does_not_spin_it() {
    let mut play = still();
    let y0 = play.cam.yaw;
    for _ in 0..50 {
        play.step(&pad((0, 32_767), 0));
    }
    assert_eq!(play.cam.yaw, y0);
}

#[test]
fn the_sky_tilt_comes_in_over_open_sky() {
    let mut play = still();
    play.hero = hero_at(0, 0, 650, MAIN);
    play.cam.snap(&play.hero, &BLOCKS, ISLAND.props, 1);
    assert_eq!(play.cam.tilt, 0);
    for _ in 0..40 {
        play.step(&pad(FWD, 0));
        if play.hero.frozen > 0 {
            break;
        }
    }
    assert_eq!(play.cam.tilt, SKY_TILT);
}

/// No prop that is drawn (scale above 0) has its ball across the line
/// from the eye to the aim or to the hero's middle.
#[test]
fn in_a_sweep_no_prop_is_drawn_between_the_eye_and_the_hero() {
    let mut play = Play::on(&ISLAND, Detail::High);
    let mut shrunk = 0;
    // Walk a loop round the island while the camera turns.
    let points: [[i16; 2]; 6] = [
        [500, -400],
        [450, 450],
        [-50, 550],
        [-600, -100],
        [-300, -600],
        [500, -550],
    ];
    for round in 0..2 {
        for (k, to) in points.iter().enumerate() {
            for _ in 0..90 {
                let h = &play.hero;
                let dx = f64::from(proto3d::maths::cm(to[0]) - h.pos[0]);
                let dz = f64::from(proto3d::maths::cm(to[1]) - h.pos[2]);
                let mut p = pad(stick_towards(dx, dz, play.cam.yaw, 0.6), 0);
                if (k + round) % 2 == 0 {
                    p.right = (20_000, 0);
                }
                play.step(&p);
                let eye = play.eye().map(unit);
                let aim = play.aim().map(unit);
                let hp = play.hero.pos;
                let chest = [hp[0], hp[1] + HERO_H / 2, hp[2]].map(unit);
                for (i, pr) in ISLAND.props.iter().enumerate() {
                    let Some((c, r)) = pr.ball() else { continue };
                    if !play.prop_shown(i) {
                        continue;
                    }
                    let (c, r) = (c.map(unit), i64::from(unit(r)));
                    let d2 = dist2_to_segment(c, aim, eye).min(dist2_to_segment(c, chest, eye));
                    if d2 <= r * r {
                        assert_eq!(play.cam.scale[i], 0, "prop {i} across the line");
                    }
                    if play.cam.scale[i] < FULL {
                        shrunk += 1;
                    }
                }
            }
        }
    }
    assert!(shrunk > 0, "the sweep never came near a prop");
}

/// The ready game-3d camera looks at the aim: the aim is straight ahead
/// (x and y near 0, z the distance), and the hero's feet are below it.
#[test]
fn the_game_3d_camera_looks_at_the_aim() {
    let mut play = still();
    for _ in 0..5 {
        play.step(&Pad::NONE);
    }
    let cam = play.cam.camera();
    let a = cam.to_cam(play.aim().map(unit));
    assert!(a[0].abs() <= 2 && a[1].abs() <= 2, "{a:?}");
    assert!((i64::from(a[2]) - i64::from(DIST)).abs() <= 4, "{a:?}");
    let f = cam.to_cam(play.hero.pos.map(unit));
    assert!(f[1] < 0 && f[2] > 0, "{f:?}");
}
