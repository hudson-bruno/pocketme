//! The car's rules (the spec's section 1), each expected value with
//! its hand sum. Speeds are in subs a step: 4,096 is 30 m/s.

mod common;

use common::{COAST, Flat, GAS, Slope, car_on, rolling};
use drive3d::car::{Car, Controls, ramp, split, stick_aim};
use drive3d::land::{Ground, Stretch};
use drive3d::road::ROAD;
use drive3d::tune::{
    ACC, CAR_HEIGHT, FORD, G, GRASS, GRAVEL, HANDLING, M, RAMP_IN, RAMP_OUT, REV_TOP, TURN, VT,
};

fn flat(surface: u8) -> Flat {
    Flat { surface, height: 0 }
}

/// Steps of full throttle from a standstill until the speed reaches
/// `v` (None within a minute), and the speed after a minute.
fn run_up(surface: u8, v: i32) -> (Option<u32>, i32) {
    let g = flat(surface);
    let mut c = car_on(&g);
    let mut at = None;
    for n in 1..=1800 {
        c.step(&GAS, &g);
        if at.is_none() && c.speed() >= v {
            at = Some(n);
        }
    }
    (at, c.speed())
}

#[test]
fn top_speed_settles_at_top_on_each_surface() {
    // Engine rounded up and forward drag scaled by (256 - throttle):
    // at full throttle the only pull below TOP is the engine, so the
    // speed settles on TOP exactly (the spec's test: within 2 %).
    for s in [GRAVEL, GRASS, FORD] {
        let top = HANDLING[usize::from(s)].top;
        let (_, v) = run_up(s, top);
        assert!(
            (v - top).abs() * 50 <= top,
            "surface {s}: {v} against {top}"
        );
        assert_eq!(v, top, "surface {s}");
    }
}

#[test]
fn time_to_speed_per_surface() {
    // The engine adds ACC * (TOP - v) / TOP a step (full throttle), so
    // v(n) = TOP (1 - e^(-n ACC / TOP)) and the steps to a share s of
    // TOP are n = TOP / ACC * ln(1 / (1 - s)).
    // Gravel, 0 to 25 m/s (3,413, s = 5/6): 4096 / 41 * ln 6 = 99.9 *
    // 1.792 = 179 steps (6.0 s); rounding the step up makes it a little
    // sooner. MEASURED 173.
    let (n, _) = run_up(GRAVEL, 3_413);
    let n = n.expect("gravel reaches 25 m/s");
    assert!((165..=181).contains(&n), "gravel 0-25 m/s: {n} steps");
    // To 90 % of TOP: n = TOP / ACC * ln 10 = 99.9 * 2.303 = 230 steps
    // on gravel; grass 1638 / 41 * 2.303 = 92 (R1, 2026-11-22: grass
    // is slower); ford 1843 / 41 * 2.303 = 104. MEASURED 219, 98.
    for (s, sum) in [(GRAVEL, 230), (GRASS, 92), (FORD, 104)] {
        let top = HANDLING[usize::from(s)].top;
        let (n, _) = run_up(s, top * 9 / 10 + 1);
        let n = n.expect("reaches 90 %") as i32;
        assert!(
            n <= sum && n >= sum * 9 / 10,
            "surface {s}: 90 % after {n} steps, sum {sum}"
        );
    }
    // The ford never reaches 25 m/s (its TOP is 13.5 m/s).
    assert_eq!(run_up(FORD, 3_413).0, None);
    let _ = ACC;
}

#[test]
fn braking_from_30_mps_on_gravel() {
    // Brake: min(BRAKE * 256 / 256, CAP) = min(36, 31) = 31 a step,
    // and the full drag fwd / 256 (no throttle). dv/dn = -31 - v / 256
    // gives v(n) = (v0 + 7936) e^(-n / 256) - 7936 (31 * 256 = 7936):
    // stops at n = 256 ln(12032 / 7936) = 106.5 steps (3.55 s) after
    // 256 * (12032 - 7936) - 7936 * 106.5 = 203,400 subs = 49.7 m.
    // MEASURED 108 steps, 49.74 m.
    let g = flat(GRAVEL);
    let mut c = rolling(&g, 4096);
    let z0 = c.pos[2];
    let brake = Controls {
        brake: 256,
        ..COAST
    };
    let mut n = 0;
    while c.vel[1] > 0 {
        c.step(&brake, &g);
        n += 1;
        assert!(n < 200);
    }
    let d = c.pos[2] - z0;
    assert!((104..=110).contains(&n), "{n} steps");
    assert!((48 * M..=51 * M).contains(&d), "{} m", d / M);
    // Then it stands: the brake held at a standstill only starts the
    // reverse (next test), it never drives forwards.
    assert!(c.vel[1] <= 0);
}

#[test]
fn the_brake_held_at_a_standstill_reverses_up_to_rev_top() {
    // DEFAULT rule. The reverse adds REV_ACC * (REV_TOP + fwd) / REV_TOP
    // backwards a step (rounded up), and the full drag fwd / 256 pulls
    // back (no throttle). They balance where 20 (1092 - u) / 1092 =
    // u / 256: u = 5120 * 1092 / (1092 + 5120) = 900 (6.6 m/s); the
    // roundings (up for the add, toward 0 for the drag) move it a
    // little out. MEASURED -929.
    let g = flat(GRAVEL);
    let mut c = car_on(&g);
    let brake = Controls {
        brake: 256,
        ..COAST
    };
    for _ in 0..600 {
        c.step(&brake, &g);
    }
    let (fwd, side) = c.fwd_side();
    assert_eq!(side, 0);
    assert!((-960..=-880).contains(&fwd), "{fwd}");
    assert!(fwd > -REV_TOP);
    // Throttle then stops it and drives forwards.
    for _ in 0..120 {
        c.step(&GAS, &g);
    }
    assert!(c.fwd_side().0 > 1_000);
}

#[test]
fn a_rolling_car_comes_to_a_stop() {
    // DEFAULT: at least 1 sub a step lost with no throttle. From 2 m/s
    // (273): the drag 273 / 256 = 1 a step until 255, then 1 a step:
    // about 273 steps.
    let g = flat(GRAVEL);
    let mut c = rolling(&g, 273);
    let mut n = 0;
    while c.speed() > 0 {
        c.step(&COAST, &g);
        n += 1;
        assert!(n < 400);
    }
    assert!((260..=280).contains(&n), "{n}");
}

#[test]
fn steering_at_low_and_high_speed() {
    // turn = steer * TURN * min(|fwd|, VT) / VT / 256.
    // At 5 m/s (683): 256 * 546 * 683 / 1365 / 256 = 273 (half of
    // TURN, 45 degrees a second). At 20 m/s (2,731 > VT): 546.
    // Standing: 0.
    let g = flat(GRAVEL);
    for (v, want) in [(683, 273), (2_731, TURN), (0, 0), (VT, TURN)] {
        let mut c = rolling(&g, v);
        c.steer = 256;
        let full = Controls { aim: 256, ..COAST };
        c.step(&full, &g);
        assert_eq!(i32::from(c.yaw as i16), want, "at {v}");
    }
    // The handbrake turns 3/2 as much: 546 * 3 / 2 = 819.
    let mut c = rolling(&g, 2_731);
    c.steer = 256;
    c.step(
        &Controls {
            aim: 256,
            handbrake: true,
            ..COAST
        },
        &g,
    );
    assert_eq!(i32::from(c.yaw as i16), 819);
}

#[test]
fn reversing_steers_the_other_way() {
    // fwd = -683: the same full right lock turns the nose -273.
    let g = flat(GRAVEL);
    let mut c = rolling(&g, -683);
    c.steer = 256;
    c.step(&Controls { aim: 256, ..COAST }, &g);
    assert_eq!(i32::from(c.yaw as i16), -273);
}

#[test]
fn the_steering_ramps_in_6_steps_and_out_in_4() {
    // 43 a step: 43 * 6 = 258 >= 256; 64 a step: 64 * 4 = 256.
    let mut s = 0;
    let mut n = 0;
    while s != 256 {
        s = ramp(s, 256);
        n += 1;
    }
    assert_eq!(n, 6);
    let mut n = 0;
    while s != 0 {
        s = ramp(s, 0);
        n += 1;
    }
    assert_eq!(n, 4);
    // Across: back to straight at RAMP_OUT, then out at RAMP_IN.
    assert_eq!(ramp(128, -256), 128 - RAMP_OUT);
    assert_eq!(ramp(0, -256), -RAMP_IN);
    // The stick: dead below 4,096; full at 32,767.
    assert_eq!(stick_aim(4_000), 0);
    assert_eq!(stick_aim(32_767), 256);
    assert_eq!(stick_aim(-32_767), -256);
    // Half way through the live part: (18431 - 4096) * 256 / 28671 = 127.
    assert_eq!(stick_aim(18_431), 127);
}

/// The slide angle (degrees, of the speed against the nose) after
/// `steps` of full right lock and full throttle from `v` straight on.
fn slide_after(v: i32, hb: bool, steps: u32) -> (Car, i32) {
    let g = flat(GRAVEL);
    let mut c = rolling(&g, v);
    let ctl = Controls {
        aim: 256,
        throttle: 256,
        brake: 0,
        handbrake: hb,
    };
    for _ in 0..steps {
        c.step(&ctl, &g);
    }
    let (f, s) = c.fwd_side();
    // atan in whole numbers through the angle helper (1/65536 turn).
    let a = drive3d::maths::yaw_of(s.abs(), f.max(1)).unwrap_or(0);
    (c, i32::from(a) * 360 / 65_536)
}

#[test]
fn the_slide_starts_grows_with_speed_and_the_handbrake() {
    // Start: the nose turns TURN a step (full lock over VT), while the
    // grip can turn the speed at most CAP a step: CAP / v radians, at
    // 25 m/s (3,413) 31 / 3413 = 0.0091 rad = 0.52 degrees against
    // the nose's 3.0 degrees a step. So the gap (the slide) opens by
    // about 2.5 degrees a step once the steering is in (6 steps).
    // At 10 m/s (1,365) the grip turns 1.3 degrees a step: the gap
    // opens slower. MEASURED after 15 steps: 29.8 and 21.3 degrees
    // before R2; since R2 the full throttle held in the slide gives grip
    // back (up to twice), and it is 25 and 14 (whole degrees). With the
    // handbrake (grip and cap 3/8, turn 3/2; no power grip) 52 and 45,
    // as before.
    let (_, at10) = slide_after(1_365, false, 15);
    let (_, at25) = slide_after(3_413, false, 15);
    let (_, hb10) = slide_after(1_365, true, 15);
    let (_, hb25) = slide_after(3_413, true, 15);
    assert!(at25 > at10, "{at25} > {at10}");
    assert!(hb10 > at10 && hb25 > at25);
    assert!((20..=30).contains(&at25), "{at25}");
    assert!((9..=19).contains(&at10), "{at10}");
    assert!((45..=58).contains(&hb25), "{hb25}");
    // How it starts: the first step turns the nose by 43 * 546 / 256 =
    // 91 (the steering's first ramp step), while the speed keeps its
    // way, so the next split sees a side speed of -3413 * sin(91 / 65536
    // of a turn) = -3413 * 0.00872 = -29.8.
    let (c, _) = slide_after(3_413, false, 1);
    assert_eq!(c.fwd_side().1, -30);
}

#[test]
fn the_slide_holds_then_dies_with_the_stick_straight() {
    // Hold: 15 steps of full lock at 20 m/s, then the wheel straight.
    // The grip takes min(|side| * g / 256, CAP) a step, g = 150 * 256 /
    // (256 + v / 64); at about 18 m/s g = 150 * 256 / 294 = 130, half
    // the side speed, but held to CAP = 31. The side speed after the
    // 15 steps is about 1,340 (9.8 m/s, MEASURED): 1340 / 31 = 43 steps
    // at the cap, a few more as it gets small: it ends near 45 steps.
    let (mut c, _) = slide_after(2_731, false, 15);
    let g = flat(GRAVEL);
    let side0 = c.fwd_side().1.abs();
    assert!(side0 > 1_000, "{side0}");
    let mut n = 0;
    let mut last = side0;
    while c.fwd_side().1 != 0 {
        c.step(&COAST, &g);
        n += 1;
        let s = c.fwd_side().1.abs();
        // It holds: once the wheel is straight (RAMP_OUT, 4 steps; while
        // it comes back the nose still turns and moves the split), never
        // more than the cap, the side drag (side / 256) and rounding a
        // step.
        if n > 4 {
            assert!(last - s <= 31 + last / 256 + 2, "step {n}: {last} -> {s}");
        }
        last = s;
        assert!(n < 120);
    }
    assert!((40..=70).contains(&n), "the slide ended after {n} steps");
}

#[test]
fn the_handbrake_cuts_the_grip_on_a_slide() {
    // Added in the final round (a mutation check found the handbrake's
    // grip change was caught only by the pictures). A sliding car with
    // the wheel straight (steer 0, so the nose does not turn): the grip
    // takes min(|side| * g / 256, cap) a step, plus the side drag
    // (side / 256) and rounding. Gravel's cap is 31; with the handbrake
    // grip and cap are times HB / 256 = 96 / 256, cap 31 * 96 / 256 = 11.
    let (mut c, _) = slide_after(2_731, false, 15);
    c.steer = 0;
    let g = flat(GRAVEL);
    let hb = Controls {
        handbrake: true,
        ..COAST
    };
    let (mut a, mut b) = (c, c);
    for n in 1..=5 {
        let (sa, sb) = (a.fwd_side().1.abs(), b.fwd_side().1.abs());
        a.step(&COAST, &g);
        b.step(&hb, &g);
        let (la, lb) = (sa - a.fwd_side().1.abs(), sb - b.fwd_side().1.abs());
        assert!(la >= 28, "step {n}: without the handbrake {la}");
        assert!(lb <= 11 + sb / 256 + 2, "step {n}: with the handbrake {lb}");
    }
}

#[test]
fn on_a_slope_the_car_follows_the_land() {
    // A 1 in 8 slope (1 m per 8 m): rolling at 10 m/s (1,365), each
    // step the land rises v / 8 = 170 subs; the car stays on it with vy
    // the land's change. The rules have no slope pull (DEFAULT, the
    // spec has none), so only the drag slows it: 1365 / 256 = 5 a step,
    // vy falls by about 5 / 8 a step.
    let g = Slope { num: 1, den: 8 };
    let mut c = rolling(&g, 1_365);
    for _ in 0..60 {
        let before = c.pos[1];
        c.step(&COAST, &g);
        assert!(!c.air);
        assert_eq!(c.pos[1], g.at(c.pos[0], c.pos[2]).height);
        assert_eq!(c.vy, c.pos[1] - before);
        let want = c.vel[1] / 8;
        assert!((c.vy - want).abs() <= 2, "{} {}", c.vy, want);
    }
}

/// The longest flight (steps in the air) going over the stretch's
/// crest at `v`, held at that speed.
fn crest_flight(v: i32) -> u16 {
    let g = Stretch;
    let k = 44;
    let mut c = Car::standing(&g, ROAD.x[k], ROAD.z[k], 0);
    c.vel = [0, v];
    let mut longest = 0;
    for _ in 0..120 {
        c.step(&COAST, &g);
        c.vel = [0, v];
        longest = longest.max(c.air_steps);
    }
    longest
}

#[test]
fn over_the_crest_at_20_mps_the_car_flies_at_12_it_does_not() {
    // The crest's land (section 3): the road climbs 1/4 m to row 17
    // and falls 1 m to row 18 (cells of 8 m), a change of slope of
    // (1 + 4) / 32 = 0.156 at row 17. The car leaves the ground where
    // v * 0.156 > G = 45 subs a step: v > 288 subs a step (2.1 m/s):
    // every car hops there (the land is made of flat triangles; see the
    // report). A real flight (the spec's "flies") is taken as at least
    // 15 steps (0.5 s) in the air: the time in the air is about
    // 2 v dS / G steps, 2 * 2731 * 0.156 / 45 = 19 at 20 m/s and
    // 2 * 1638 * 0.156 / 45 = 11 at 12 m/s; 15 steps needs v = 15 * 45
    // / (2 * 0.156) = 2,163 (15.8 m/s; the spec's estimate, from a
    // smooth curve: 17 m/s). MEASURED: 17 steps at 20 m/s, 10 at 12.
    let at20 = crest_flight(2_731);
    let at12 = crest_flight(1_638);
    assert!(at20 >= 15, "20 m/s: {at20} steps in the air");
    assert!(at12 < 15, "12 m/s: {at12} steps in the air");
}

#[test]
fn in_the_air_gravity_changes_the_speed_before_the_place() {
    // vy -= G, then y += vy: from 10 m up at rest, y = 40960 - 45, then
    // 40960 - 45 - 90 = 40825.
    let g = flat(GRAVEL);
    let mut c = car_on(&g);
    c.pos[1] = 10 * M;
    c.air = true;
    c.air_steps = 1;
    c.step(&COAST, &g);
    assert_eq!((c.vy, c.pos[1]), (-G, 10 * M - 45));
    c.step(&COAST, &g);
    assert_eq!((c.vy, c.pos[1]), (-2 * G, 10 * M - 135));
    // It lands when y + vy reaches the land: from 40960 the falls add
    // up to 45 n (n + 1) / 2 >= 40960 at n = 42 (45 * 42 * 43 / 2 =
    // 40,635 < 40,960; n = 43: 42,570).
    let mut n = 2;
    while c.air {
        c.step(&COAST, &g);
        n += 1;
    }
    assert_eq!(n, 43);
    assert_eq!(c.pos[1], 0);
}

#[test]
fn landing_costs_speed_by_the_spec_rule() {
    // A car landing at vy = -(LAND_FREE + 8 * 20) = -340 loses 20 of
    // 256; straight (no side speed), so nothing more. 2,048 * 236 / 256
    // = 1,888.
    let g = flat(GRAVEL);
    let mut c = rolling(&g, 2_048);
    c.pos[1] = 100;
    c.vy = -(180 + 160) + G;
    c.air = true;
    c.air_steps = 5;
    let ev = c.step(&COAST, &g);
    assert_eq!(ev.landed.map(|l| l.1), Some(20));
    // Before landing, drag in the air took 2048 / 256 = 8.
    assert_eq!(c.vel[1], (2_048 - 8) * 236 / 256);
    // Crossed up: side speed as large as forward adds 64 * 0.707 = 45.
    let mut c = rolling(&g, 0);
    c.vel = [1_448, 1_448];
    c.pos[1] = 100;
    c.vy = -180 + G;
    c.air = true;
    c.air_steps = 5;
    let ev = c.step(&COAST, &g);
    let loss = ev.landed.expect("landed").1;
    assert!((44..=46).contains(&loss), "{loss}");
}

#[test]
fn the_split_of_the_speed_is_along_the_nose_and_its_right() {
    // Yaw a quarter turn: the nose along +x, its right along -z.
    assert_eq!(split([1_000, 0], 16_384), (1_000, 0));
    assert_eq!(split([0, -1_000], 16_384), (0, 1_000));
    let _ = CAR_HEIGHT;
}

/// The path's turn and the slip after `n` steps from 15 m/s on flat
/// `surface` with the wheel at `aim` and the throttle at `t` (degrees).
fn turn_from_15(surface: u8, aim: i32, t: i32, n: u32) -> (f64, f64) {
    use drive3d::maths::{angle_diff, yaw_of};
    let g = flat(surface);
    let mut c = rolling(&g, 2_048);
    let y0 = yaw_of(c.vel[0], c.vel[1]).expect("moving");
    for _ in 0..n {
        c.step(
            &Controls {
                aim,
                throttle: t,
                brake: 0,
                handbrake: false,
            },
            &g,
        );
    }
    let path = yaw_of(c.vel[0], c.vel[1]).expect("moving");
    let deg = |a: i32| f64::from(a) * 360.0 / 65_536.0;
    (
        deg(angle_diff(path, y0)).abs(),
        deg(angle_diff(c.yaw, path)).abs(),
    )
}

#[test]
fn power_out_of_a_slide() {
    // R2 (owner's rule, 2026-11-22): the throttle held in a slide gives
    // grip back. From 15 m/s, half lock, 1 s on gravel (MEASURED, PC):
    // coasting the path turns 25.6 degrees with 17.9 of slip; full
    // throttle turned it 24.0 with 19.5 of slip before R2 (the soap),
    // and 32.4 with 11.1 since.
    let (coast, coast_slip) = turn_from_15(GRAVEL, 128, 0, 30);
    let (gas, gas_slip) = turn_from_15(GRAVEL, 128, 256, 30);
    assert!(
        gas > coast + 5.0,
        "throttle {gas:.1} against coasting {coast:.1}"
    );
    assert!(
        gas_slip < coast_slip,
        "slip {gas_slip:.1} against {coast_slip:.1}"
    );
    // Full lock: the path turns more with the throttle too.
    let (coast, _) = turn_from_15(GRAVEL, 256, 0, 30);
    let (gas, _) = turn_from_15(GRAVEL, 256, 256, 30);
    assert!(gas > coast + 10.0, "full lock: {gas:.1} against {coast:.1}");
    // The grass gives nothing back: with the throttle held the car still
    // slides there (MEASURED, PC: 30.7 degrees of slip, 12.8 of path;
    // the path turns more than coasting, 9.0, only because the engine
    // over the grass's top speed slows the car, as before R2).
    let (_, grass_slip) = turn_from_15(GRASS, 128, 256, 30);
    assert!(
        grass_slip > 2.0 * gas_slip,
        "grass slip {grass_slip:.1} against gravel {gas_slip:.1}"
    );
}
