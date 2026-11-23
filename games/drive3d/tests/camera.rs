//! The chase camera (the spec's section 2).

mod common;

use common::{Flat, GAS, MID, autopilot, car_on, car_on_screen, rolling, roof_hidden_by};
use drive3d::camera::{Chase, hide_rise};
use drive3d::car::Controls;
use drive3d::land::{Ground, Spot, Stretch};
use drive3d::maths::{angle_diff, len2};
use drive3d::play::Play;
use drive3d::tune::{BACK, CAR_HEIGHT, CLEAR, EASE_POS, GRAVEL, M, UP};

fn flat() -> Flat {
    Flat {
        surface: GRAVEL,
        height: 0,
    }
}

#[test]
fn behind_a_standing_car_the_camera_is_back_and_up() {
    // BACK (9.5 m) behind along the yaw, UP (2.8 m) above.
    let g = flat();
    let car = car_on(&g);
    let c = Chase::behind(&car, &g);
    assert_eq!(c.pos, [MID, UP, MID - BACK]);
    // It looks down at the point 6 m ahead and 0.6 m up: the drop
    // 2.8 - 0.6 = 2.2 m over 9.5 + 6 = 15.5 m, atan(0.142) = 8.1
    // degrees = 1,472 of 65,536 (the atan helper is within 0.25
    // degrees: 45).
    assert!((c.pitch as i32 - 1_472).abs() <= 45, "{}", c.pitch);
}

#[test]
fn the_camera_lags_three_steps_of_speed_behind() {
    // The place moves a quarter of the way each step (EASE_POS 64).
    // With the car at a steady v a step, the gap e between wanted and
    // real place settles where e = (e + v) * 3 / 4: e = 3 v. The car is
    // set to 2,731 (20 m/s) before each step and the drag takes 2731 /
    // 256 = 10 before it moves, so v = 2,721 and the camera is BACK +
    // 3 * 2721 = 38912 + 8163 = 47,075 subs (11.5 m) behind.
    assert_eq!(EASE_POS, 64);
    let g = flat();
    let mut car = rolling(&g, 2_731);
    let mut cam = Chase::behind(&car, &g);
    let hold = Controls { throttle: 0, ..GAS };
    for _ in 0..90 {
        car.step(&hold, &g);
        car.vel = [0, 2_731];
        cam.step(&car, &g, 0);
    }
    let back = car.pos[2] - cam.pos[2];
    assert!((back - 47_075).abs() <= 8, "{back}");
    // Stopping, it closes in to BACK: after 60 steps 3 * 2731 * 0.75^60
    // = 0.003 subs left (plus rounding: the quarter rounds down).
    car.vel = [0, 0];
    for _ in 0..60 {
        cam.step(&car, &g, 0);
    }
    let back = car.pos[2] - cam.pos[2];
    assert!((back - BACK).abs() <= 4, "{back}");
}

#[test]
fn the_angle_gap_wraps_through_yaw_zero_without_a_jump() {
    // A car turning steadily from -10 degrees through 0 to +10: the
    // camera's yaw moves a little each step, never round the long way.
    let g = flat();
    let mut car = car_on(&g);
    car.yaw = (65_536 - 1_820) as u16;
    let mut cam = Chase::behind(&car, &g);
    for _ in 0..40 {
        car.yaw = car.yaw.wrapping_add(91);
        let before = cam.yaw;
        cam.step(&car, &g, 0);
        let moved = angle_diff(before, cam.yaw);
        assert!(moved.abs() <= 400, "jump of {moved}");
    }
    let off = angle_diff(car.yaw, cam.yaw);
    assert!(off.abs() < 2_000, "{off}");
}

/// A land with a wall of earth 3 m high from z = MID - 4 m to MID -
/// 2 m (behind a car standing at MID), flat elsewhere.
struct Bank;
impl Ground for Bank {
    fn at(&self, _x: i32, z: i32) -> Spot {
        let h = if (MID - 4 * M..MID - 2 * M).contains(&z) {
            3 * M
        } else {
            0
        };
        Spot {
            height: h,
            surface: GRAVEL,
        }
    }
    fn size(&self) -> (i32, i32) {
        (1 << 29, 1 << 29)
    }
}

#[test]
fn the_crest_check_lifts_the_camera_over_land_between_it_and_the_car() {
    // Camera 9.5 m back, 2.8 m up; the roof 1.4 m up. Of the four
    // points (1/5 .. 4/5 of the way), the one at 4/5 is at z = MID -
    // 1.9 m (outside the bank), at 3/5 MID - 3.8 m (on it): its line
    // height 2.8 - (2.8 - 1.4) * 3 / 5 = 1.96 m, the bank 3 m: a gap of
    // 1.04 m, so the camera rises 1.04 * 5 / 2 = 2.6 m (under the 3 m
    // cap).
    let g = Bank;
    let car = car_on(&g);
    let cam = [MID, UP, MID - BACK];
    let rise = hide_rise(cam, &car, &g);
    let sum = (3 * M - (UP - (UP - CAR_HEIGHT) * 3 / 5)) * 5 / 2;
    assert!((rise - sum).abs() <= 4, "{rise} against {sum}");
    // And with it, the roof is in sight at every one of 64 points but
    // those the 4-point check cannot see (the bank's edge between two
    // points): the full camera's floor step uses it.
    let c = Chase::behind(&car, &g);
    assert!(c.pos[1] >= UP + rise - 4);
}

#[test]
fn over_the_whole_stretch_the_car_stays_in_the_picture_and_in_sight() {
    // The scripted drive (the test driver at 20 m/s, 14 in corners)
    // from the start to standing past the finish. Every step:
    // * the car's middle projects inside the 160 by 120 picture;
    // * its box (4 by 1.8 by 1.4 m) is 12 to 40 px tall on screen
    //   (ESTIMATED by hand: 1.4 m tall seen from 9.8 m with lens 100 is
    //   14 px, plus the top's depth seen from above, about 24 px;
    //   MEASURED 17 to 25);
    // * the camera is at least CLEAR above the land under it;
    // * the land never rises over the line from the camera to the roof
    //   (looked at in 32 points, the land's planes, not pixels).
    let mut p = Play::new();
    let mut lo = i32::MAX;
    let mut hi = 0;
    for _ in 0..1_200 {
        let input = autopilot(&p, 20 * M / 30, 14 * M / 30);
        p.step(&input);
        let s = car_on_screen(&p);
        assert!((0..160).contains(&s.mid.0) && (0..120).contains(&s.mid.1));
        assert!(s.nearest > 0, "the car is behind the camera");
        lo = lo.min(s.height_px);
        hi = hi.max(s.height_px);
        let clear = p.cam.pos[1] - Stretch.at(p.cam.pos[0], p.cam.pos[2]).height;
        assert!(clear >= CLEAR, "clear {clear}");
        assert_eq!(roof_hidden_by(&p, &Stretch, 32), 0, "step {}", p.steps);
        if p.finished.is_some() && p.car.speed() == 0 {
            break;
        }
    }
    assert!(p.finished.is_some());
    assert!(lo >= 12 && hi <= 40, "{lo}..{hi} px");
    println!("car height on screen: {lo}..{hi} px");
}

#[test]
fn at_the_crest_the_roof_is_never_hidden() {
    // Over the crest at 12, 20 and 30 m/s, the car held at speed.
    for v in [12, 20, 30] {
        let mut p = Play::new();
        let k = 40usize;
        p.last_road = k as u16;
        p.put_back();
        p.car.vel = [0, v * M / 30];
        for _ in 0..90 {
            p.step(&drive3d::play::Input::default());
            p.car.vel[1] = v * M / 30;
            p.car.vel[0] = 0;
            assert_eq!(roof_hidden_by(&p, &Stretch, 32), 0, "{v} m/s");
            let d = len2(p.cam.pos[0] - p.car.pos[0], p.cam.pos[2] - p.car.pos[2]);
            assert!(d > 0);
        }
    }
}
