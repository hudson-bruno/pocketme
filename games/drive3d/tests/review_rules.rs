//! An independent review of DRIVE3D's rules and the feel of driving
//! (2026-11-22). Every expected value has its hand sum beside it.
//!
//! Tests marked `FINDING` fail on purpose: each shows a fault the
//! review found (its findings are kept outside the repository). They
//! stay failing until the fault is fixed. The `feel_*` tests measure
//! scripted drives and print their numbers (run with `--nocapture`).
//!
//! The scripted drivers here look only at what a player can see: the
//! road ahead (its points and their headings, as drawn), where the car
//! is and where its nose points, and its speed. They never read the
//! rules' own state (`last_road`, the grip, the surface table).

mod common;

use common::{COAST, Flat, GAS, Slope, car_on, car_on_screen, rolling};
use drive3d::camera::Chase;
use drive3d::car::{Car, Controls};
use drive3d::clock::Clock;
use drive3d::land::{Ground, Stretch, side_of};
use drive3d::maths::{angle_diff, dir, yaw_of};
use drive3d::play::{Input, Play};
use drive3d::road::{POINTS, ROAD};
use drive3d::tune::{
    ACC, CELL, CELLS_X, CELLS_Z, EDGE, FORD, GRASS, GRAVEL, HANDLING, M, REV_TOP, STEP_US, TURN,
};
use std::f64::consts::TAU;

const LAST: usize = POINTS - 1;

fn mps(v: i32) -> f64 {
    f64::from(v) * 30.0 / f64::from(M)
}

fn subs(v_mps: f64) -> i32 {
    (v_mps * f64::from(M) / 30.0).round() as i32
}

fn deg(a: i32) -> f64 {
    f64::from(a) * 360.0 / 65_536.0
}

/// The road point nearest to (x, z), looked for over the whole road
/// (what a player sees: where on the road the car is).
fn nearest_k(x: i32, z: i32) -> usize {
    (0..POINTS)
        .min_by_key(|&k| {
            let dx = i64::from(x) - i64::from(ROAD.x[k]);
            let dz = i64::from(z) - i64::from(ROAD.z[k]);
            dx * dx + dz * dz
        })
        .unwrap()
}

/// How far (m) the point is from the road's line, plus to the right.
fn offset_m(x: i32, z: i32) -> f64 {
    let k = nearest_k(x, z);
    f64::from(side_of(&ROAD, k, x, z)) / f64::from(M)
}

/// The road's bend (radians a metre) around point j: the heading
/// change over 5 points (10 m).
fn bend(j: usize) -> f64 {
    let a = j.saturating_sub(2);
    let b = (j + 3).min(LAST);
    if b <= a {
        return 0.0;
    }
    let d = f64::from(angle_diff(ROAD.heading[a], ROAD.heading[b])) * TAU / 65_536.0;
    d.abs() / ((b - a) as f64 * 2.0)
}

/// A car standing at road point k, along the road, rolling at `v` m/s.
fn play_at(k: usize, v: f64) -> Play {
    let mut p = Play::new();
    let h = ROAD.heading[k];
    let mut car = Car::standing(&Stretch, ROAD.x[k], ROAD.z[k], h);
    let (fx, fz) = dir(h);
    let s = i64::from(subs(v));
    car.vel = [
        (s * i64::from(fx) / 16_384) as i32,
        (s * i64::from(fz) / 16_384) as i32,
    ];
    p.car = car;
    p.last_road = k as u16;
    p.cam = Chase::behind(&p.car, &Stretch);
    p.running = true;
    p
}

/// Steering toward a road point `look_m` ahead (at least 6 m), full
/// lock at `full_deg` off the nose.
fn pursue(p: &Play, look_s: f64, full_deg: f64) -> i32 {
    let c = &p.car;
    let k0 = nearest_k(c.pos[0], c.pos[2]);
    let v = mps(c.speed());
    let n = ((v * look_s).max(6.0) / 2.0).ceil() as usize;
    let (tx, tz) = if k0 + n <= LAST {
        (ROAD.x[k0 + n], ROAD.z[k0 + n])
    } else {
        let (s, co) = dir(ROAD.heading[LAST]);
        let ahead = ((k0 + n - LAST) * 2) as i64 * i64::from(M);
        (
            ROAD.x[LAST] + (ahead * i64::from(s) / 16_384) as i32,
            ROAD.z[LAST] + (ahead * i64::from(co) / 16_384) as i32,
        )
    };
    let want = yaw_of(tx - c.pos[0], tz - c.pos[2]).unwrap_or(c.yaw);
    let off = deg(angle_diff(c.yaw, want));
    ((off / full_deg) * 256.0).round().clamp(-256.0, 256.0) as i32
}

// ---------------------------------------------------------------------
// 1. Breaking the car's rules
// ---------------------------------------------------------------------

#[test]
fn full_lock_at_top_speed_on_each_surface_circles_and_recovers() {
    // Full lock and full throttle held for 300 steps (10 s) from TOP on
    // each flat surface. The nose turns up to TURN = 546 a step once
    // the steering is in (6 ramp steps: 43, 86, .., 256): at most
    // 546 * 297 = 162,162 in 300 steps, 2.47 turns (ESTIMATED by
    // hand, an upper bound). The speed never passes TOP. Then the wheel
    // straight (full throttle on): the nose stops turning once the
    // steering is back at 0 (4 steps), and the slide dies (the grip
    // takes up to CAP a step).
    for s in [GRAVEL, GRASS, FORD] {
        let g = Flat {
            surface: s,
            height: 0,
        };
        let h = HANDLING[usize::from(s)];
        let mut c = rolling(&g, h.top);
        let lock = Controls { aim: 256, ..GAS };
        let mut turned: i64 = 0;
        let mut worst_slide = 0.0f64;
        let mut lowest = i32::MAX;
        for _ in 0..300 {
            let y0 = c.yaw;
            c.step(&lock, &g);
            turned += i64::from(angle_diff(y0, c.yaw));
            assert!(c.speed() <= h.top + 2, "surface {s}: {}", c.speed());
            let (f, sd) = c.fwd_side();
            let a = deg(i32::from(yaw_of(sd.abs(), f).unwrap_or(0)));
            let a = if a > 180.0 { 360.0 - a } else { a };
            worst_slide = worst_slide.max(a);
            lowest = lowest.min(c.speed());
        }
        let turns = turned as f64 / 65_536.0;
        // MEASURED about 1.5 turns, not the 2.47 of the hand sum: the
        // nose turns TURN * min(fwd, VT) / VT, and in the spin the
        // forward part of the speed drops under VT for long stretches.
        // Above 1 turn in 10 s: the nose never stops coming round.
        assert!(turns > 1.0, "surface {s}: {turns} turns");
        let v_lock = c.speed();
        // Straight again.
        let mut still = 0;
        let mut n_side = None;
        for n in 1..=150 {
            let y0 = c.yaw;
            c.step(&GAS, &g);
            if c.yaw == y0 {
                still += 1;
            } else {
                assert!(n <= 4, "surface {s}: still turning at step {n}");
            }
            let (_, sd) = c.fwd_side();
            if n_side.is_none() && sd.abs() * 50 <= c.speed().max(1) {
                n_side = Some(n);
            }
        }
        assert!(still >= 146);
        let n_side = n_side.expect("the slide dies within 5 s");
        println!(
            "surface {s}: full lock 10 s from {:.1} m/s: {turns:.2} turns of the nose, \
             slide angle up to {worst_slide:.0} deg, speed {:.1} m/s at the end (lowest \
             {:.1}); straight again: slide under 2 % after {n_side} steps",
            mps(h.top),
            mps(v_lock),
            mps(lowest)
        );
    }
}

/// FINDING: full brake and full throttle together drive a standing car
/// forward to 7.3 m/s on gravel.
///
/// Hand sum: at a standstill the engine adds ceil(ACC * 256 * TOP /
/// (256 * TOP)) = 41 a step; the brake takes min(BRAKE * 256 / 256,
/// CAP) = min(36, 31) = 31. Net +10 a step. It settles where the
/// engine's step equals the brake's: 41 (TOP - f) / TOP = 31, f =
/// 4096 x 10 / 41 = 999 subs a step (7.3 m/s). With the spec's ACC of 23
/// the brake (31) beat the engine (23) and the car stood still; ACC
/// was raised to 41 and the brake was not.
#[test]
fn finding_full_brake_and_full_throttle_together_keep_the_car_still() {
    let g = Flat {
        surface: GRAVEL,
        height: 0,
    };
    let mut c = car_on(&g);
    let both = Controls { brake: 256, ..GAS };
    for _ in 0..300 {
        c.step(&both, &g);
    }
    println!("both pedals for 10 s: {:.1} m/s", mps(c.speed()));
    assert_eq!(c.speed(), 0, "the car creeps at {} subs a step", c.speed());
}

#[test]
fn brake_and_full_lock_together_have_no_shared_grip() {
    // The brake (step 5) and the grip (step 6) each take up to CAP a
    // step, on their own: braking costs no cornering grip. Measured:
    // a car at 20 m/s in a full-lock turn, with and without the brake.
    // The sideways change a step is the same (31, the CAP), so braking
    // in a corner is free: the car brakes at 0.7 g and turns at 0.7 g
    // together, 0.99 g in all (31 * sqrt 2 = 43.8 subs a step a step).
    let g = Flat {
        surface: GRAVEL,
        height: 0,
    };
    let start = |brake: i32| {
        let mut c = rolling(&g, 2_731);
        c.steer = 256;
        let ctl = Controls {
            aim: 256,
            throttle: 0,
            brake,
            handbrake: false,
        };
        // Into the slide first.
        for _ in 0..6 {
            c.step(&ctl, &g);
        }
        let v0 = c.vel;
        c.step(&ctl, &g);
        let dv = [c.vel[0] - v0[0], c.vel[1] - v0[1]];
        ((dv[0] as f64).hypot(dv[1] as f64), c)
    };
    let (free, _) = start(0);
    let (braking, _) = start(256);
    println!(
        "change of speed in one step, full lock at about 20 m/s: {free:.1} without the brake, \
         {braking:.1} with it (subs a step a step; CAP 31 = 0.7 g)"
    );
    assert!(braking > 40.0, "{braking}");
    // Braking with full lock from 25 m/s stops the car: count the steps.
    let mut c = rolling(&g, 3_413);
    let ctl = Controls {
        aim: 256,
        throttle: 0,
        brake: 256,
        handbrake: false,
    };
    let mut n = 0;
    while c.fwd_side().0 > 0 {
        c.step(&ctl, &g);
        n += 1;
        assert!(n < 200);
    }
    let mut s = rolling(&g, 3_413);
    let mut n_straight = 0;
    while s.fwd_side().0 > 0 {
        s.step(
            &Controls {
                brake: 256,
                ..COAST
            },
            &g,
        );
        n_straight += 1;
    }
    println!(
        "from 25 m/s: forward speed gone after {n} steps with full lock, {n_straight} straight"
    );
}

#[test]
fn reverse_tops_out_steers_the_other_way_and_the_throttle_stops_it() {
    // B held at a standstill drives backwards. Hand sum (report): with
    // the coasting drag of 1/256 against it, 20 (1092 - u) / 1092 =
    // u / 256 gives u = 900; the step rounded up gives a little more
    // (the report's 929). Never past REV_TOP.
    let g = Flat {
        surface: GRAVEL,
        height: 0,
    };
    let mut c = car_on(&g);
    let back = Controls {
        brake: 256,
        ..COAST
    };
    for _ in 0..600 {
        c.step(&back, &g);
        assert!(c.fwd_side().0 >= -REV_TOP);
    }
    let rev = -c.fwd_side().0;
    assert!((880..=960).contains(&rev), "{rev}");
    // Full right lock while reversing turns the nose left (minus).
    let y0 = c.yaw;
    for _ in 0..10 {
        c.step(&Controls { aim: 256, ..back }, &g);
    }
    assert!(angle_diff(y0, c.yaw) < 0);
    // Throttle stops it: the engine adds ACC (TOP - fwd) / TOP, over 41
    // a step while fwd < 0: 929 / 41 = about 23 steps.
    let mut n = 0;
    while c.fwd_side().0 < 0 {
        c.step(&GAS, &g);
        n += 1;
        assert!(n < 40, "{n}");
    }
    // With nothing pressed it rolls backwards for a long time: the drag
    // is fwd / 256 (at least 1) a step: 929 -> 768 at 3 (54 steps),
    // -> 512 at 2 (128), -> 0 at 1 (511): about 690 steps, 23 s.
    let mut c = car_on(&g);
    for _ in 0..600 {
        c.step(&back, &g);
    }
    let mut n = 0;
    while c.speed() > 0 {
        c.step(&COAST, &g);
        n += 1;
        assert!(n < 2_000);
    }
    println!(
        "reverse top {rev} subs ({:.1} m/s); coasting from it: {n} steps to a stop",
        mps(rev)
    );
    assert!((600..=760).contains(&n), "{n}");
}

#[test]
fn standing_on_a_steep_slope_the_car_stays_and_climbing_costs_nothing() {
    // The rules have no pull down a slope on the ground (gravity acts
    // only in the air). A car standing on a 1 in 2 slope stays where it
    // is; a car climbing it at full throttle gains speed exactly as on
    // the flat.
    let steep = Slope { num: 1, den: 2 };
    let mut c = car_on(&steep);
    let p0 = c.pos;
    for _ in 0..300 {
        c.step(&COAST, &steep);
    }
    assert_eq!(c.pos, p0, "it stands");
    let flat = Flat {
        surface: GRAVEL,
        height: 0,
    };
    let (mut a, mut b) = (car_on(&flat), car_on(&steep));
    for _ in 0..120 {
        a.step(&GAS, &flat);
        b.step(&GAS, &steep);
    }
    println!(
        "after 4 s of full throttle: flat {:.2} m/s, up a 1 in 2 slope {:.2} m/s (climbed {:.1} m)",
        mps(a.speed()),
        mps(b.speed()),
        f64::from(b.pos[1] - p0[1]) / f64::from(M)
    );
    assert_eq!(a.speed(), b.speed(), "no slope pull (spec: none)");
}

#[test]
fn over_the_top_of_a_steep_climb_the_car_is_thrown_high() {
    // vy on the ground is the land's change a step: up a 1 in 2 slope at
    // 20 m/s (2,731) it is 1,365 subs a step (10 m/s upward). Where the
    // slope ends the car flies, keeping it: up for 1365 / 45 = 30 steps,
    // about 1365^2 / (2 * 45) = 20,700 subs (5.1 m) high, 2 s in the
    // air (ESTIMATED). Nothing caps the throw.
    struct Ramp;
    impl Ground for Ramp {
        fn at(&self, _x: i32, z: i32) -> drive3d::land::Spot {
            let top = common::MID + 8 * M;
            let h = if z < common::MID {
                0
            } else if z < top {
                (z - common::MID) / 2
            } else {
                4 * M
            };
            drive3d::land::Spot {
                height: h,
                surface: GRAVEL,
            }
        }
        fn size(&self) -> (i32, i32) {
            (1 << 29, 1 << 29)
        }
    }
    let g = Ramp;
    let mut c = rolling(&g, 2_731);
    let mut air = 0;
    let mut high = 0;
    for _ in 0..200 {
        c.step(&GAS, &g);
        if c.air {
            air += 1;
            high = high.max(c.pos[1] - g.at(c.pos[0], c.pos[2]).height);
        }
    }
    println!(
        "off a 4 m ramp of 1 in 2 at 20 m/s: {air} steps in the air, {:.1} m over the land",
        f64::from(high) / f64::from(M)
    );
    assert!(air > 30);
}

#[test]
fn the_steepest_land_on_the_stretch() {
    // The largest change between two neighbouring corners (8 m apart),
    // and the vy a car at 30 m/s meets there (4096 / 8 m * rise).
    let land = &drive3d::land::LAND;
    let mut worst = 0i32;
    for j in 0..CELLS_Z {
        for i in 0..CELLS_X {
            let a = i32::from(land.corner(i, j));
            let b = i32::from(land.corner(i + 1, j));
            let c = i32::from(land.corner(i, j + 1));
            worst = worst.max((a - b).abs()).max((a - c).abs());
        }
    }
    let rise_m = f64::from(worst) / 4.0;
    println!(
        "steepest step between corners: {rise_m} m over 8 m (1 in {:.1}); \
         at 30 m/s across it vy is {:.0} subs a step",
        8.0 / rise_m,
        4096.0 * rise_m / 8.0
    );
    assert!(worst <= 16, "{worst}");
}

#[test]
fn turning_through_yaw_zero_is_smooth_for_the_car_and_the_camera() {
    // Nose at -11 degrees (65,536 - 2,000), full right lock at 20 m/s:
    // the nose moves TURN (546) a step once the steering is in, through
    // 0 without a jump; the speed's size changes by at most CAP + drag
    // a step; the camera's yaw moves by at most a fifth of its gap.
    let g = Flat {
        surface: GRAVEL,
        height: 0,
    };
    let mut p = Play::new();
    let mut c = rolling(&g, 2_731);
    c.yaw = (65_536 - 2_000) as u16;
    c.vel = [0, 2_731];
    p.car = c;
    let mut cam = Chase::behind(&p.car, &g);
    let lock = Controls { aim: 256, ..GAS };
    let mut crossed = false;
    for n in 0..30 {
        let (y0, v0, cy0) = (p.car.yaw, p.car.speed(), cam.yaw);
        p.car.step(&lock, &g);
        cam.step(&p.car, &g, 0);
        let dy = angle_diff(y0, p.car.yaw);
        assert!((0..=TURN).contains(&dy), "step {n}: {dy}");
        if n >= 6 {
            assert_eq!(dy, TURN);
        }
        assert!((p.car.speed() - v0).abs() <= 45, "step {n}");
        assert!(angle_diff(cy0, cam.yaw).abs() <= 2_000, "step {n}");
        if y0 > 60_000 && p.car.yaw < 5_000 {
            crossed = true;
        }
    }
    assert!(crossed);
}

#[test]
fn yaw_of_is_within_a_quarter_degree_all_round() {
    // atan approximated by t * 8192 + 2847 t (1 - t): the module says
    // within about 0.25 degrees (45.5 angle units). Checked against
    // atan2 on 3,600 directions and a few lengths, both short and long.
    let mut worst = 0.0f64;
    for i in 0..3_600 {
        let a = f64::from(i) * TAU / 3_600.0;
        for r in [7.0, 1_000.0, 2_000_000.0] {
            let (x, z) = ((a.sin() * r).round() as i32, (a.cos() * r).round() as i32);
            let got = f64::from(yaw_of(x, z).unwrap());
            let want = f64::from(x).atan2(f64::from(z)).rem_euclid(TAU) * 65_536.0 / TAU;
            let mut e = (got - want).abs();
            if e > 32_768.0 {
                e = 65_536.0 - e;
            }
            if r > 100.0 {
                worst = worst.max(e);
            }
        }
    }
    println!(
        "yaw_of worst error {worst:.1} angle units ({:.3} deg)",
        worst * 360.0 / 65_536.0
    );
    assert!(worst <= 46.0, "{worst}");
    assert_eq!(angle_diff(65_535, 1), 2);
    assert_eq!(angle_diff(1, 65_535), -2);
    assert_eq!(angle_diff(0, 32_768), -32_768);
}

#[test]
fn driving_into_every_edge_and_corner_stays_inside_and_gets_out() {
    // From the block's middle at full throttle, wheel straight, toward 8
    // directions (the four edges and the four corners) until the edge
    // first pushes the car (at most 40 s), then 3 s more pressed into
    // it: the car stays EDGE inside the block all the time. Then the
    // way out a player would take: B (reverse) with full lock for
    // 1.5 s, then the throttle with the nose pointed at the middle:
    // within 20 s the car is 24 m from every edge.
    //
    // The way out: B (reverse), wheel straight, until 14 m from every
    // edge (the slowest full-lock circle has a radius of VT * 65,536 /
    // (TURN * 2 pi) = 26,075 subs, 6.4 m), then the throttle with the
    // nose pointed at the middle: within 20 s the car is 24 m from
    // every edge.
    let (sx, sz) = (CELLS_X as i32 * CELL, CELLS_Z as i32 * CELL);
    let inside = |c: &Car| {
        assert!((EDGE..=sx - EDGE).contains(&c.pos[0]));
        assert!((EDGE..=sz - EDGE).contains(&c.pos[2]));
    };
    for dirn in 0..8u16 {
        let yaw = dirn * 8_192;
        let mut p = play_at(0, 0.0);
        p.car = Car::standing(&Stretch, sx / 2, sz / 2, yaw);
        p.cam = Chase::behind(&p.car, &Stretch);
        let gas = Input {
            controls: GAS,
            ..Input::default()
        };
        let mut reached = None;
        for n in 0..1_200 {
            p.step(&gas);
            inside(&p.car);
            if p.events.car.edge {
                reached = Some(n);
                break;
            }
        }
        let hits_on_way = p.hits;
        let v_at = mps(p.car.speed());
        for _ in 0..90 {
            p.step(&gas);
            inside(&p.car);
        }
        let v_pressed = mps(p.car.speed());
        let gap = |c: &Car| c.pos[0].min(sx - c.pos[0]).min(c.pos[2]).min(sz - c.pos[2]);
        let mut backed = 0;
        while gap(&p.car) < 14 * M && backed < 300 {
            p.step(&Input {
                controls: Controls {
                    brake: 256,
                    ..COAST
                },
                ..Input::default()
            });
            inside(&p.car);
            backed += 1;
        }
        let mut out = None;
        for n in 0..600 {
            let c = &p.car;
            let want = yaw_of(sx / 2 - c.pos[0], sz / 2 - c.pos[2]).unwrap_or(c.yaw);
            let aim = (angle_diff(c.yaw, want) * 256 / 2_000).clamp(-256, 256);
            p.step(&Input {
                controls: Controls { aim, ..GAS },
                ..Input::default()
            });
            inside(&p.car);
            let c = &p.car;
            if gap(c) > 24 * M {
                out = Some(n);
                break;
            }
        }
        // When the throttle got stuck on the edge again (see
        // `finding_a_car_pressed_against_the_edge_can_drive_away_along_it`),
        // the sure way: B until 24 m from every edge.
        let mut backed_out = None;
        if out.is_none() {
            for n in 0..900 {
                p.step(&Input {
                    controls: Controls {
                        brake: 256,
                        ..COAST
                    },
                    ..Input::default()
                });
                inside(&p.car);
                if gap(&p.car) > 24 * M {
                    backed_out = Some(n);
                    break;
                }
            }
        }
        println!(
            "direction {dirn}: edge reached after {reached:?} steps at {v_at:.1} m/s \
             ({hits_on_way} things hit on the way); pressed into it 3 s: {v_pressed:.1} m/s; \
             reversed {backed} steps; with the throttle 24 m out after {out:?} steps; \
             else in reverse after {backed_out:?}"
        );
        assert!(
            reached.is_some(),
            "direction {dirn}: never reached the edge"
        );
        assert!(
            out.is_some() || backed_out.is_some(),
            "direction {dirn}: stuck at the edge"
        );
    }
}

/// FINDING: a car pressed against the block's edge with the throttle
/// held cannot turn away from it: it sticks there until the player
/// reverses (B) or asks to be put back (Y).
///
/// Hand sum: nose 10 degrees off straight into the edge at x = max
/// (yaw 80 degrees), full throttle, full left lock (away from the
/// edge, toward yaw 0). Each step the engine adds about 41 along the
/// nose; the edge removes the outward x part and then multiplies what
/// is left by 224 / 256; the grip takes the sideways part. The
/// forward speed stays at a few subs a step, and the nose turns TURN *
/// fwd / VT = 546 * 3 / 1365 = about 1 angle unit a step (0.006
/// degrees): 70 degrees would take over 10,000 steps (6 minutes).
/// MEASURED in a drive from the block's middle (direction 3): 0.1 m/s
/// and 1.3 degrees in 250 steps. A real wall at a slant lets the car
/// slide along it; here every step on the edge takes an eighth of all
/// the speed, along the edge too.
#[test]
fn finding_a_car_pressed_against_the_edge_can_drive_away_along_it() {
    let (sx, sz) = (CELLS_X as i32 * CELL, CELLS_Z as i32 * CELL);
    let mut p = play_at(0, 0.0);
    p.car = Car::standing(&Stretch, sx - EDGE, sz / 2, 14_563);
    p.cam = Chase::behind(&p.car, &Stretch);
    let y0 = p.car.yaw;
    for _ in 0..300 {
        p.step(&Input {
            controls: Controls { aim: -256, ..GAS },
            ..Input::default()
        });
    }
    let c = &p.car;
    let off_wall = f64::from(sx - EDGE - c.pos[0]) / f64::from(M);
    let along = f64::from(c.pos[2] - sz / 2) / f64::from(M);
    let turned = deg(angle_diff(y0, c.yaw));
    println!(
        "10 s of full throttle and full lock away from the edge: {off_wall:.2} m off it, \
         {along:.2} m along it, nose turned {turned:.1} deg, {:.2} m/s",
        mps(c.speed())
    );
    assert!(off_wall > 4.0, "stuck at the edge: {off_wall:.2} m off it");
}

#[test]
fn flat_out_at_the_far_corners_of_the_block_does_not_overflow() {
    // Overflow checks are on in tests: every sum at the block's far
    // corners, at top speed, spinning with the handbrake, for 20 s.
    let (sx, sz) = (CELLS_X as i32 * CELL, CELLS_Z as i32 * CELL);
    for (x, z) in [
        (EDGE, EDGE),
        (sx - EDGE, EDGE),
        (EDGE, sz - EDGE),
        (sx - EDGE, sz - EDGE),
    ] {
        for yaw in [0u16, 16_384, 32_768, 49_152, 8_000] {
            let mut p = Play::new();
            p.car = Car::standing(&Stretch, x, z, yaw);
            let (fx, fz) = dir(yaw);
            p.car.vel = [4_096 * fx / 16_384, 4_096 * fz / 16_384];
            p.cam = Chase::behind(&p.car, &Stretch);
            for n in 0..600 {
                let ctl = Controls {
                    aim: if n % 90 < 45 { 256 } else { -256 },
                    handbrake: n % 30 < 10,
                    ..GAS
                };
                p.step(&Input {
                    controls: ctl,
                    reset: n == 300,
                    ..Input::default()
                });
                let land = Stretch.at(p.car.pos[0], p.car.pos[2]).height;
                assert!(p.car.pos[1] >= land);
            }
        }
    }
}

#[test]
fn past_the_finish_the_car_stops_before_the_blocks_edge() {
    // Once past the finish the rules brake fully (min(36, CAP 31) a step
    // plus the coasting drag). The finish line is 51.1 m from the
    // block's edge (MEASURED below), so 43.1 m of run-off before the
    // edge's 8 m. From the speed a fast driver brings to the line
    // (corner exit at 20.5 m/s at point 152, the cut's MEASURED best,
    // then 40 m of full throttle: about 25 m/s) the car stops in time.
    // From the top speed of 30 m/s (not reachable here; ESTIMATED by
    // hand: 4096 / 31 = 132 steps without drag, about 50 m) it would
    // not: printed, not asserted.
    let (sx, sz) = (CELLS_X as i32 * CELL, CELLS_Z as i32 * CELL);
    let (lx, lz) = (ROAD.x[LAST], ROAD.z[LAST]);
    let fin_gap = f64::from(lx.min(sx - lx).min(lz).min(sz - lz)) / 4096.0;
    let run = |k: usize, v: f64| {
        let mut p = play_at(k, v);
        let mut edge = false;
        let mut n = 0;
        let mut v_line = 0.0;
        while !(p.finished.is_some() && p.car.speed() == 0) {
            p.step(&Input {
                controls: GAS,
                ..Input::default()
            });
            if p.events.finished.is_some() {
                v_line = mps(p.car.speed());
            }
            edge |= p.events.car.edge;
            n += 1;
            assert!(n < 600);
        }
        let c = &p.car;
        let past = f64::from(drive3d::land::along_of(&ROAD, LAST, c.pos[0], c.pos[2])) / 4096.0;
        (v_line, past, edge)
    };
    let (v_fast, past_fast, edge_fast) = run(152, 20.5);
    let (v_top, past_top, edge_top) = run(LAST - 6, 30.0);
    println!(
        "finish line {fin_gap:.1} m from the block's edge. From the corner's exit at 20.5 m/s: \
         {v_fast:.1} m/s at the line, stopped {past_fast:.1} m past it, edge: {edge_fast}. \
         At 30 m/s: stopped {past_top:.1} m past it ({v_top:.1} at the line), edge: {edge_top}"
    );
    assert!(
        !edge_fast,
        "a fast driver runs into the block's edge after the finish"
    );
}

// ---------------------------------------------------------------------
// 2. The clock, presses and the fingerprint
// ---------------------------------------------------------------------

/// FINDING: after one long update (a stall: a slow picture, the first
/// frames after loading) the clock's store is left at a whole number of
/// steps, so the half-step start is lost, and updates that come a
/// little early then run 0 and 2 steps in turn for ever.
///
/// Hand sum: start 16,666. A stall of 1 s: 1,016,666, two steps run
/// (950,000 left), held to 2 * 33,333 = 66,666. Next update of 33,000:
/// 99,666, two steps, 33,000 left. 33,666: 66,666, two steps, 0 left.
/// 33,000: 33,000, NO step. 33,666: 66,666, two steps. And so on.
/// Before the stall the same deltas ran one step each (16,666 + 33,000
/// = 49,666: one, 16,333 left; + 33,666 = 49,999: one, 16,666 left).
/// The stall also runs 2 + 2 + 2 steps in its first three updates (a
/// catch-up of 3 steps), though the module says it does not race.
#[test]
fn finding_after_a_stall_jittered_updates_still_run_one_step_each() {
    let mut c = Clock::new();
    let before: Vec<u8> = (0..8)
        .map(|i| c.add(if i % 2 == 0 { 33_000 } else { 33_666 }))
        .collect();
    assert_eq!(before, vec![1; 8], "before the stall");
    assert_eq!(c.add(1_000_000), 2);
    let after: Vec<u8> = (0..8)
        .map(|i| c.add(if i % 2 == 0 { 33_000 } else { 33_666 }))
        .collect();
    println!("steps per update after a 1 s stall, deltas 33,000 / 33,666: {after:?}");
    assert_eq!(&after[2..], &[1; 6], "0, 2, 0, 2 after the stall");
}

#[test]
fn zero_tiny_and_huge_deltas() {
    // 1,000 updates of 0: nothing. Updates of 1 us: the first step
    // after 33,333 - 16,666 = 16,667 of them. One of u32::MAX: two
    // steps, no overflow (the store saturates).
    let mut c = Clock::new();
    for _ in 0..1_000 {
        assert_eq!(c.add(0), 0);
    }
    let mut n = 0;
    let mut first = None;
    for i in 1..=20_000u32 {
        let s = c.add(1);
        if s > 0 && first.is_none() {
            first = Some(i);
        }
        n += u32::from(s);
    }
    assert_eq!(first, Some(16_667));
    assert_eq!(n, 1);
    assert_eq!(c.add(u32::MAX), 2);
    assert!(c.store <= 2 * STEP_US);
}

#[test]
fn a_one_frame_tap_of_y_or_x_is_never_lost() {
    // Y or X down for exactly one console frame (33,333 us), at each of
    // the first frames (the first update runs no step: its delta is 0)
    // and later: the press reaches a step. Y: the car is put back after
    // 1.5 s; X: the level changes (from HIGH, the start, to MAX).
    use game_kit::button;
    use game_kit::testing::TestConsole;
    for at in 0..5 {
        for (b, want) in [(button::Y, "back on the road"), (button::X, "detail MAX")] {
            // SAFETY: `game_entry` is made by `game_kit::game!`.
            let mut c = unsafe { TestConsole::new(drive3d::game_entry) };
            for _ in 0..at {
                c.step();
            }
            c.hold(b);
            c.step();
            c.hold(0);
            c.run_ms(2_000);
            let logs: Vec<String> = c.log_lines().iter().map(|l| l.to_string()).collect();
            assert!(
                logs.iter().any(|l| l.contains(want)),
                "tap of {b:#x} at frame {at}: {logs:?}"
            );
        }
    }
}

/// FINDING: the fingerprint leaves out state that changes later steps.
///
/// `Play::print` takes the car's place, speeds, yaw, steering, air,
/// `last_road` and the camera. It leaves out `reset_in` (a Y waiting:
/// the car is put back later), `running` and `run_steps` (the run's
/// time, so the finish time), `finished` (the brakes go on by
/// themselves) and the car's `air_steps` (the landing's loss).
/// MEASURED: a waiting Y gives the same fingerprint for 3 steps, then
/// the run parts (finish 549 against 599 steps); 100 steps more on the
/// run's clock give the same fingerprint for the whole run while the
/// finish time differs by 100 steps. `finished` shows within a step
/// (the brakes change the car).
#[test]
fn finding_the_fingerprint_sees_every_field_that_changes_a_later_step() {
    // Two plays at road point 30, 15 m/s, that differ in one hidden
    // field, both driven by the same driver who looks at the road. After
    // one step their fingerprints are equal; later they part (the car's
    // place, or the finish time).
    let base = play_at(30, 15.0);
    let s = Sight::steady();
    let mut blind = Vec::new();
    type Tweak = (&'static str, fn(&mut Play));
    let tweaks: [Tweak; 4] = [
        ("reset_in (a Y waiting)", |p| p.reset_in = 3),
        ("running/run_steps (the run's time)", |p| {
            p.running = false;
            p.run_steps = 0;
        }),
        ("finished (the brakes on by themselves)", |p| {
            p.finished = Some(1);
        }),
        ("run_steps (100 steps more on the clock)", |p| {
            p.run_steps = 100
        }),
    ];
    for (name, tweak) in tweaks {
        let (mut a, mut b) = (base, base);
        tweak(&mut b);
        let (ca, cb) = (sight(&a, &s), sight(&b, &s));
        a.step(&Input {
            controls: ca,
            ..Input::default()
        });
        b.step(&Input {
            controls: cb,
            ..Input::default()
        });
        let same_print = a.print == b.print;
        let mut differ = false;
        for _ in 0..3_000 {
            let (ca, cb) = (sight(&a, &s), sight(&b, &s));
            a.step(&Input {
                controls: ca,
                ..Input::default()
            });
            b.step(&Input {
                controls: cb,
                ..Input::default()
            });
            differ |= a.car != b.car || a.finished != b.finished;
            if a.finished.is_some() && b.finished.is_some() && a.car.speed() == 0 {
                break;
            }
        }
        differ |= a.finished != b.finished;
        let same_end = a.print == b.print;
        println!(
            "{name}: same fingerprint after the step: {same_print}; the plays part later: \
             {differ} (finish {:?} against {:?}); same fingerprint at the end: {same_end}",
            a.finished, b.finished
        );
        if (same_print || same_end) && differ {
            blind.push(name);
        }
    }
    assert!(blind.is_empty(), "left out of the fingerprint: {blind:?}");
}

#[test]
fn the_same_drive_twice_is_the_same_to_the_last_field() {
    let s = Sight::steady();
    let (a, ra) = drive_sight(&s);
    let (b, rb) = drive_sight(&s);
    assert_eq!(a, b);
    assert_eq!(ra.steps, rb.steps);
    assert_eq!(a.print, b.print);
}

// ---------------------------------------------------------------------
// 3. Feel: the whole stretch with a driver who only looks at the road
// ---------------------------------------------------------------------

/// A driver who looks at the road: steers at a point `look_s` seconds
/// ahead, picks a speed from the bends it can see (no faster than
/// sqrt(a_lat / bend), with room to brake at `a_brake`), throttle under
/// it and brake 0.5 m/s over it.
#[derive(Clone, Copy, Debug)]
struct Sight {
    a_lat: f64,
    a_brake: f64,
    look_s: f64,
    full_deg: f64,
}

impl Sight {
    fn steady() -> Sight {
        Sight {
            a_lat: 4.0,
            a_brake: 5.0,
            look_s: 0.5,
            full_deg: 10.0,
        }
    }
}

fn speed_target(k0: usize, s: &Sight) -> f64 {
    let mut t = 40.0f64;
    for j in k0..=(k0 + 40).min(LAST) {
        let b = bend(j);
        let vl = if b > 1e-4 { (s.a_lat / b).sqrt() } else { 40.0 };
        let d = (j - k0) as f64 * 2.0;
        t = t.min((vl * vl + 2.0 * s.a_brake * d).sqrt());
    }
    t
}

fn sight(p: &Play, s: &Sight) -> Controls {
    let c = &p.car;
    let k0 = nearest_k(c.pos[0], c.pos[2]);
    let v = mps(c.speed());
    let target = speed_target(k0, s);
    let fwd = c.fwd_side().0;
    let (throttle, brake) = if v < target {
        (256, 0)
    } else if v > target + 0.5 && fwd > 0 {
        (0, 256)
    } else {
        (0, 0)
    };
    Controls {
        aim: pursue(p, s.look_s, s.full_deg),
        throttle,
        brake,
        handbrake: false,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Run {
    finish: Option<u32>,
    steps: u32,
    max_off: f64,
    off_steps: u32,
    grass_steps: u32,
    stops: u32,
    max_air: u16,
    flights: u16,
    hits: u16,
    edge: u32,
    cam_jump: f64,
    cam_close: f64,
    out_of_picture: u32,
    px_low: i32,
    px_high: i32,
}

fn watch(p: &Play, prev_cam_y: i32, r: &mut Run) {
    let c = &p.car;
    let k = nearest_k(c.pos[0], c.pos[2]);
    if k < LAST {
        let off = offset_m(c.pos[0], c.pos[2]).abs();
        r.max_off = r.max_off.max(off);
        if off > 3.0 {
            r.off_steps += 1;
        }
    }
    if !c.air && c.surface == GRASS {
        r.grass_steps += 1;
    }
    if p.running && c.speed() == 0 && p.finished.is_none() {
        r.stops += 1;
    }
    r.max_air = r.max_air.max(c.air_steps);
    if p.events.car.edge {
        r.edge += 1;
    }
    r.cam_jump = r
        .cam_jump
        .max(f64::from((p.cam.pos[1] - prev_cam_y).abs()) / f64::from(M));
    let d =
        f64::from(p.cam.pos[0] - c.pos[0]).hypot(f64::from(p.cam.pos[2] - c.pos[2])) / f64::from(M);
    r.cam_close = if r.cam_close == 0.0 {
        d
    } else {
        r.cam_close.min(d)
    };
    let on = car_on_screen(p);
    if !(0..160).contains(&on.mid.0) || !(0..120).contains(&on.mid.1) || on.nearest <= 0 {
        r.out_of_picture += 1;
    }
    r.px_low = if r.px_low == 0 {
        on.height_px
    } else {
        r.px_low.min(on.height_px)
    };
    r.px_high = r.px_high.max(on.height_px);
}

fn drive_sight(s: &Sight) -> (Play, Run) {
    let mut p = Play::new();
    let mut r = Run::default();
    for n in 0..3_000u32 {
        let ctl = sight(&p, s);
        let cy = p.cam.pos[1];
        p.step(&Input {
            controls: ctl,
            ..Input::default()
        });
        watch(&p, cy, &mut r);
        r.steps = n + 1;
        if p.finished.is_some() {
            break;
        }
    }
    r.finish = p.finished;
    r.flights = p.flights;
    r.hits = p.hits;
    (p, r)
}

#[test]
fn feel_the_whole_stretch_with_a_driver_who_looks_at_the_road() {
    // Five drivers from careful (3 m/s/s in bends) to bold (7, over the
    // gravel's 0.7 g = 6.8 m/s/s). For each: the time to the finish,
    // the farthest the car's middle got from the road's line (the road
    // is 6 m wide: over 3 m is off it), steps off it, on grass, in the
    // air, things hit, the camera's biggest jump in height in one step
    // and how near it came to the car.
    println!(
        "a_lat  finish(s)  max_off(m)  off  grass  stops  flights  max_air  hits  edge  \
         cam_jump(m)  cam_near(m)  car_out_of_picture  car_px"
    );
    let mut clean = Vec::new();
    for a_lat in [3.0, 4.0, 5.0, 6.0, 7.0] {
        let s = Sight {
            a_lat,
            ..Sight::steady()
        };
        let (_, r) = drive_sight(&s);
        println!(
            "{a_lat:>5}  {:>9}  {:>10.2}  {:>3}  {:>5}  {:>5}  {:>7}  {:>7}  {:>4}  {:>4}  \
             {:>11.2}  {:>11.2}  {:>3}  {}..{}",
            r.finish
                .map_or("-".to_string(), |f| format!("{:.2}", f64::from(f) / 30.0)),
            r.max_off,
            r.off_steps,
            r.grass_steps,
            r.stops,
            r.flights,
            r.max_air,
            r.hits,
            r.edge,
            r.cam_jump,
            r.cam_close,
            r.out_of_picture,
            r.px_low,
            r.px_high
        );
        if let (Some(f), 0) = (r.finish, r.off_steps) {
            clean.push((a_lat, f));
        }
        assert_eq!(r.stops, 0, "the car stopped dead on the way");
    }
    println!("clean runs (a_lat, steps): {clean:?}");
    assert!(!clean.is_empty(), "no driver got round on the road");
}

// ---------------------------------------------------------------------
// 4. Feel: the tightest corner (piece 8, left 90 degrees in 40 m)
// ---------------------------------------------------------------------

/// One way through the corner.
#[derive(Clone, Copy, Debug)]
struct Plan {
    /// Speed held at the corner's entry (m/s).
    v_entry: f64,
    /// Handbrake pulled this far before the corner's first point (m).
    hb_before: f64,
    /// For this many steps (0: none), with full left lock.
    hb_steps: u32,
    /// In the corner: full throttle (a power slide) instead of holding
    /// `v_entry` to the corner's last third.
    power: bool,
    /// Full lock at this angle off the nose.
    full_deg: f64,
    /// Look this many seconds ahead (a longer look cuts the corner).
    look_s: f64,
    /// 0: steer the nose at the point ahead. Above 0: steer like a
    /// player holding a slide: point the nose up to this many degrees
    /// inside the way the car travels (counter-steering when the tail
    /// swings out too far).
    drift: f64,
}

#[derive(Clone, Copy, Debug)]
struct CornerRun {
    steps: u32,
    max_off: f64,
    /// Farthest to the left of the line (the corner's inside) and to
    /// the right (m; added in the final round, for the cut).
    max_in: f64,
    max_out: f64,
    hits: u16,
    exit_v: f64,
    max_slide: f64,
    /// Steps with the car on grass or the low grass (R1, 2026-11-22).
    grass: u32,
}

const START_K: usize = 118;
const GATE: usize = 162;

/// Steering for a held slide: the wanted travel is toward the point
/// ahead; the nose is put up to `max_slip` degrees inside it.
fn drift_aim(p: &Play, look_s: f64, max_slip: f64) -> i32 {
    let c = &p.car;
    if c.speed() < 300 {
        return pursue(p, look_s, 10.0);
    }
    let travel = yaw_of(c.vel[0], c.vel[1]).unwrap_or(c.yaw);
    let k0 = nearest_k(c.pos[0], c.pos[2]);
    let v = mps(c.speed());
    let n = ((v * look_s).max(6.0) / 2.0).ceil() as usize;
    let k = (k0 + n).min(LAST);
    let target = yaw_of(ROAD.x[k] - c.pos[0], ROAD.z[k] - c.pos[2]).unwrap_or(travel);
    let off = deg(angle_diff(travel, target));
    let slip = (off * 3.0).clamp(-max_slip, max_slip);
    let want = f64::from(travel) + slip * 65_536.0 / 360.0;
    let want = want.rem_euclid(65_536.0) as u16;
    let e = deg(angle_diff(c.yaw, want));
    ((e / 6.0) * 256.0).round().clamp(-256.0, 256.0) as i32
}

fn corner_run(plan: &Plan, v_start: f64) -> CornerRun {
    let (entry, end) = ROAD.piece_points(7);
    let mut p = play_at(START_K, v_start);
    let mut max_off = 0.0f64;
    let mut max_in = 0.0f64;
    let mut max_out = 0.0f64;
    let mut max_slide = 0.0f64;
    let mut hb_left: Option<u32> = None;
    let mut steps = 0;
    let mut grass = 0;
    while steps < 600 {
        let c = &p.car;
        let k0 = nearest_k(c.pos[0], c.pos[2]);
        if k0 >= GATE {
            break;
        }
        let v = mps(c.speed());
        let fwd = c.fwd_side().0;
        let to_entry = (entry as f64 - k0 as f64) * 2.0;
        if hb_left.is_none() && plan.hb_steps > 0 && to_entry <= plan.hb_before {
            hb_left = Some(plan.hb_steps);
        }
        let hold = |vt: f64| {
            if v < vt {
                (256, 0)
            } else if v > vt + 0.5 && fwd > 0 {
                (0, 256)
            } else {
                (0, 0)
            }
        };
        let aim = if plan.drift > 0.0 && (hb_left.is_some() || to_entry <= plan.hb_before) {
            drift_aim(&p, plan.look_s, plan.drift)
        } else {
            pursue(&p, plan.look_s, plan.full_deg)
        };
        let mut ctl = Controls { aim, ..COAST };
        match hb_left {
            Some(h) if h > 0 => {
                ctl.aim = -256;
                ctl.handbrake = true;
                hb_left = Some(h - 1);
            }
            _ => {
                let in_corner = hb_left.is_some() || to_entry <= 0.0;
                let (t, b) = if !in_corner {
                    // Brake in time for v_entry at the entry (5.5 m/s/s).
                    let d = (to_entry - plan.hb_before).max(0.0);
                    if v * v - plan.v_entry * plan.v_entry >= 2.0 * 5.5 * d {
                        hold(plan.v_entry)
                    } else {
                        (256, 0)
                    }
                } else if plan.power || k0 >= entry + (end - entry) * 2 / 3 {
                    (256, 0)
                } else {
                    hold(plan.v_entry)
                };
                ctl.throttle = t;
                ctl.brake = b;
            }
        }
        p.step(&Input {
            controls: ctl,
            ..Input::default()
        });
        steps += 1;
        let c = &p.car;
        let k = nearest_k(c.pos[0], c.pos[2]);
        if (START_K..GATE).contains(&k) {
            if c.surface != GRAVEL && c.surface != FORD {
                grass += 1;
            }
            max_off = max_off.max(offset_m(c.pos[0], c.pos[2]).abs());
            max_in = max_in.max(-offset_m(c.pos[0], c.pos[2]));
            max_out = max_out.max(offset_m(c.pos[0], c.pos[2]));
        }
        if c.speed() > 300 {
            let travel = yaw_of(c.vel[0], c.vel[1]).unwrap_or(c.yaw);
            max_slide = max_slide.max(deg(angle_diff(travel, c.yaw)).abs());
        }
    }
    CornerRun {
        steps,
        max_off,
        max_in,
        max_out,
        hits: p.hits,
        exit_v: mps(p.car.speed()),
        max_slide,
        grass,
    }
}

#[test]
fn feel_the_tightest_corner_grip_against_slide() {
    // From road point 118 (the split straight) at 22 m/s to point 162
    // (20 m into the finish straight), 88 m of road. "On the road": the
    // car's middle never more than 3 m from the line. ESTIMATED limit:
    // the corner's radius is 40 m / (pi / 2) = 25.5 m; at the gravel's
    // CAP (0.7 g, 6.8 m/s/s) sqrt(6.8 * 25.5) = 13.2 m/s.
    let v0 = 22.0;
    let speeds: Vec<f64> = (16..=44).map(|i| f64::from(i) / 2.0).collect();
    let mut best = std::collections::BTreeMap::new();
    let mut tried = std::collections::BTreeMap::new();
    let mut fastest_anywhere: Option<(u32, Plan, CornerRun)> = None;
    let mut note = |family: &'static str, plan: Plan, r: CornerRun| {
        if r.hits == 0 && fastest_anywhere.is_none_or(|f| r.steps < f.0) {
            fastest_anywhere = Some((r.steps, plan, r));
        }
        let t = tried.entry(family).or_insert((0u32, 0u32));
        t.0 += 1;
        if r.max_off <= 3.0 {
            t.1 += 1;
            let e = best.entry(family).or_insert((u32::MAX, plan, r));
            if r.steps < e.0 {
                *e = (r.steps, plan, r);
            }
        }
    };
    for &ve in &speeds {
        for full_deg in [6.0, 10.0] {
            let grip = Plan {
                v_entry: ve,
                hb_before: 0.0,
                hb_steps: 0,
                power: false,
                full_deg,
                look_s: 0.5,
                drift: 0.0,
            };
            note("a grip: brake to v, hold v", grip, corner_run(&grip, v0));
            let power = Plan {
                power: true,
                ..grip
            };
            note("b grip, full throttle in", power, corner_run(&power, v0));
            for (fam, pl) in [
                ("c long look (cuts), hold v", grip),
                ("d long look (cuts), full", power),
            ] {
                let cut = Plan { look_s: 1.2, ..pl };
                note(fam, cut, corner_run(&cut, v0));
            }
            for hb_before in [0.0, 4.0, 8.0, 12.0] {
                for hb_steps in [3, 6, 9, 12] {
                    for (fam, pl) in [
                        ("e handbrake, then hold v", grip),
                        ("f handbrake, then full", power),
                    ] {
                        let slide = Plan {
                            hb_before,
                            hb_steps,
                            ..pl
                        };
                        note(fam, slide, corner_run(&slide, v0));
                    }
                }
            }
            for drift in [15.0, 30.0] {
                for (fam, pl) in [
                    ("g held slide, hold v", grip),
                    ("h held slide, full", power),
                ] {
                    for (hb_before, hb_steps) in [(0.0, 0), (4.0, 6)] {
                        let d = Plan {
                            drift,
                            hb_before,
                            hb_steps,
                            ..pl
                        };
                        note(fam, d, corner_run(&d, v0));
                    }
                }
            }
        }
    }
    println!("best of each way (on the road all through), 88 m from 22 m/s:");
    for (family, (steps, plan, r)) in &best {
        let (n, on) = tried[family];
        println!(
            "  {family:<28} {steps} steps ({:.2} s), entry {:.1} m/s, hb {} m / {} steps, \
             lock at {} deg, drift {} deg, max off {:.2} m, slide up to {:.0} deg, exit \
             {:.1} m/s, hits {}; {on} of {n} tries on the road",
            f64::from(*steps) / 30.0,
            plan.v_entry,
            plan.hb_before,
            plan.hb_steps,
            plan.full_deg,
            plan.drift,
            r.max_off,
            r.max_slide,
            r.exit_v,
            r.hits
        );
    }
    for (family, (n, on)) in &tried {
        if !best.contains_key(family) {
            println!("  {family:<28} none of {n} tries stayed on the road ({on})");
        }
    }
    if let Some((steps, plan, r)) = fastest_anywhere {
        println!(
            "  fastest of all, off the road allowed, nothing hit: {steps} steps, {plan:?}, \
             max off {:.2} m, slide up to {:.0} deg, exit {:.1} m/s",
            r.max_off, r.max_slide, r.exit_v
        );
    }
    // A slide done badly: the best handbrake slide pulled 4 m early or
    // late, held 3 steps longer, or entered 2 m/s faster.
    for fam in ["e handbrake, then hold v", "g held slide, hold v"] {
        let Some((_, plan, r)) = best.get(fam) else {
            continue;
        };
        for (what, p2) in [
            (
                "pulled 4 m earlier",
                Plan {
                    hb_before: plan.hb_before + 4.0,
                    ..*plan
                },
            ),
            (
                "pulled 4 m later",
                Plan {
                    hb_before: plan.hb_before - 4.0,
                    ..*plan
                },
            ),
            (
                "held 3 steps longer",
                Plan {
                    hb_steps: plan.hb_steps + 3,
                    ..*plan
                },
            ),
            (
                "entered 2 m/s faster",
                Plan {
                    v_entry: plan.v_entry + 2.0,
                    ..*plan
                },
            ),
        ] {
            let r2 = corner_run(&p2, v0);
            println!(
                "  {fam} done badly ({what}): {} steps ({:+}), max off {:.2} m",
                r2.steps,
                i64::from(r2.steps) - i64::from(r.steps),
                r2.max_off
            );
        }
    }
    assert!(best.contains_key("a grip: brake to v, hold v"));
}

/// Review F4 (final round): the spec calls the cut inside the second
/// corner "shorter, slower". Every way through the corner that goes
/// onto the cut (the car's middle more than 6 m inside the line, so the
/// whole car is off the 6 m road) and hits nothing, without running
/// wide (more than 3 m right), must take at least as long as the best
/// way that stays on the road (never more than 3 m off). The plans are
/// the feel test's, plus longer looks ahead (2 to 4 s, aiming across
/// the corner), so some ways do take the cut. Ways between (3 to 6 m inside: a wheel or two on
/// the verge) and ways that run wide are printed, not judged: that is
/// the grip, not the cut (owner's questions).
#[test]
fn finding_the_cut_inside_the_tightest_corner_is_not_the_fastest_line() {
    // R1 (owner's rule, 2026-11-22): the road is the best way through the
    // corner. A run counts its steps to the gate plus the steps a full
    // throttle on gravel then needs for 100 m more (`run_on`), so a slow
    // exit off the grass is paid for. "On the road": the car's middle
    // never on grass or low grass (the land's surface under it); every
    // run that touches grass, runs wide, takes the verge or the cut must
    // be slower than the best run on the road.
    let v0 = 22.0;
    let score = |r: &CornerRun| r.steps + run_on(r.exit_v);
    let mut on_road = u32::MAX;
    let mut off_road: Option<(u32, Plan, CornerRun)> = None;
    let mut wide: Option<(u32, Plan, CornerRun)> = None;
    let mut verge: Option<(u32, Plan, CornerRun)> = None;
    let mut on_grass: Option<(u32, Plan, CornerRun)> = None;
    let mut note = |plan: Plan, r: CornerRun| {
        let n = score(&r);
        if r.hits > 0 {
            return;
        }
        if r.grass == 0 {
            on_road = on_road.min(n);
            return;
        }
        if on_grass.is_none_or(|f| n < f.0) {
            on_grass = Some((n, plan, r));
        }
        if r.max_in > 6.0 && r.max_out <= 3.0 {
            if off_road.is_none_or(|f| n < f.0) {
                off_road = Some((n, plan, r));
            }
        } else if r.max_in <= 6.0 && r.max_out <= 3.0 {
            if verge.is_none_or(|f| n < f.0) {
                verge = Some((n, plan, r));
            }
        } else if wide.is_none_or(|f| n < f.0) {
            wide = Some((n, plan, r));
        }
    };
    for ve in (16..=44).map(|i| f64::from(i) / 2.0) {
        for full_deg in [6.0, 10.0] {
            let grip = Plan {
                v_entry: ve,
                hb_before: 0.0,
                hb_steps: 0,
                power: false,
                full_deg,
                look_s: 0.5,
                drift: 0.0,
            };
            let power = Plan {
                power: true,
                ..grip
            };
            for pl in [grip, power] {
                note(pl, corner_run(&pl, v0));
                for look_s in [0.7, 0.85, 1.0, 1.1, 1.2, 1.3, 1.5, 2.0, 3.0, 4.0] {
                    let cut = Plan { look_s, ..pl };
                    note(cut, corner_run(&cut, v0));
                }
                for hb_before in [0.0, 4.0, 8.0, 12.0] {
                    for hb_steps in [3, 6, 9, 12] {
                        let slide = Plan {
                            hb_before,
                            hb_steps,
                            ..pl
                        };
                        note(slide, corner_run(&slide, v0));
                    }
                }
                for drift in [15.0, 30.0] {
                    for (hb_before, hb_steps) in [(0.0, 0), (4.0, 6)] {
                        let d = Plan {
                            drift,
                            hb_before,
                            hb_steps,
                            ..pl
                        };
                        note(d, corner_run(&d, v0));
                    }
                }
            }
        }
    }
    println!("best on the road (never on grass): {on_road} steps with the run-on");
    let mut beaten = Vec::new();
    for (name, best) in [
        ("touching grass", on_grass),
        ("running wide (more than 3 m right)", wide),
        ("3 to 6 m inside (on the verge)", verge),
        ("over the cut", off_road),
    ] {
        if let Some((n, plan, r)) = best {
            println!(
                "fastest {name}, nothing hit: {n} steps with the run-on ({} to the gate), \
                 {plan:?}, {} steps on grass, max off {:.2} m, inside up to {:.2} m, \
                 exit {:.1} m/s",
                r.steps, r.grass, r.max_off, r.max_in, r.exit_v
            );
            if n < on_road {
                beaten.push(format!("{name} ({n} steps)"));
            }
        }
    }
    assert!(on_road < u32::MAX, "no way stayed on the road");
    assert!(off_road.is_some(), "no way took the cut");
    assert!(
        beaten.is_empty(),
        "the road ({on_road} steps) is beaten by {beaten:?}"
    );
}

/// Steps a full throttle on gravel needs for 100 m from `v` m/s (the
/// engine's rule of `car.rs` on a flat straight, no drag under TOP).
fn run_on(v: f64) -> u32 {
    let h = HANDLING[usize::from(GRAVEL)];
    let mut fwd = subs(v);
    let (mut went, mut n) = (0i64, 0u32);
    while went < i64::from(100 * M) {
        if fwd < h.top {
            let num = i64::from(ACC) * 256 * i64::from(h.top - fwd);
            let den = 256 * i64::from(h.top);
            fwd += ((num + den - 1) / den) as i32;
        }
        went += i64::from(fwd);
        n += 1;
    }
    n
}
