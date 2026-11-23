//! Measurements for the report (MEASURED, PC): run with
//! `cargo test --offline --test measure -- --ignored --nocapture`.
//! They check nothing; the checking tests are in the other files.

mod common;

use common::{COAST, Flat, GAS, mps, rolling};
use drive3d::car::{Car, Controls};
use drive3d::land::{Ground, Stretch};
use drive3d::road::ROAD;
use drive3d::tune::{FORD, GRASS, GRAVEL, HANDLING, M, SURFACE_NAMES};

#[test]
#[ignore]
fn top_speed_and_time_per_surface() {
    for s in [GRAVEL, GRASS, FORD] {
        let g = Flat {
            surface: s,
            height: 0,
        };
        let mut c = common::car_on(&g);
        let top = HANDLING[usize::from(s)].top;
        let mut t25 = None;
        let mut t90 = None;
        let mut t98 = None;
        for n in 1..=1800 {
            c.step(&GAS, &g);
            let v = c.speed();
            if t25.is_none() && v >= 25 * M / 30 {
                t25 = Some(n);
            }
            if t90.is_none() && v * 10 >= top * 9 {
                t90 = Some(n);
            }
            if t98.is_none() && v * 100 >= top * 98 {
                t98 = Some(n);
            }
        }
        println!(
            "{}: top {} ({:.2} m/s) after 60 s: {} ({:.2} m/s); to 25 m/s {:?} steps; 90 % {:?}; 98 % {:?}",
            SURFACE_NAMES[usize::from(s)],
            top,
            mps(top),
            c.speed(),
            mps(c.speed()),
            t25,
            t90,
            t98
        );
    }
}

#[test]
#[ignore]
fn braking_distance_from_30_mps() {
    for s in [GRAVEL, GRASS, FORD] {
        let g = Flat {
            surface: s,
            height: 0,
        };
        let v0 = HANDLING[usize::from(s)].top.min(4096);
        let mut c = rolling(&g, v0);
        let z0 = c.pos[2];
        let brake = Controls {
            brake: 256,
            ..COAST
        };
        let mut n = 0;
        while c.vel[1] > 0 && n < 1000 {
            c.step(&brake, &g);
            n += 1;
        }
        println!(
            "{} from {:.2} m/s: {} steps, {:.2} m",
            SURFACE_NAMES[usize::from(s)],
            mps(v0),
            n,
            f64::from(c.pos[2] - z0) / f64::from(M)
        );
    }
}

fn slide(v: i32, hb: bool, steps: u32) -> (f64, f64, f64) {
    let g = Flat {
        surface: GRAVEL,
        height: 0,
    };
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
    let ang = f64::from(s).atan2(f64::from(f)).to_degrees();
    (ang, mps(f), mps(s))
}

#[test]
#[ignore]
fn slide_angles() {
    for v in [10, 15, 20, 25, 30] {
        for hb in [false, true] {
            for steps in [10, 15, 30] {
                let (a, f, s) = slide(v * M / 30, hb, steps);
                println!(
                    "{v} m/s hb={hb} after {steps} steps: angle {a:.1} deg, fwd {f:.2} side {s:.2}"
                );
            }
        }
    }
}

#[test]
#[ignore]
fn crest_flights() {
    let g = Stretch;
    for v in [8, 10, 11, 12, 14, 16, 17, 18, 20, 22, 25, 30] {
        let k = 44;
        let mut c = Car::standing(&g, ROAD.x[k], ROAD.z[k], 0);
        c.vel = [0, v * M / 30];
        let mut air_total = 0;
        let mut longest = 0;
        let mut cur = 0;
        let mut peak = 0;
        let mut first_off = None;
        for n in 0..90 {
            let keep = Controls {
                throttle: 0,
                ..COAST
            };
            c.step(&keep, &g);
            // keep the speed as given
            let sp = c.speed();
            if sp > 0 {
                let want = v * M / 30;
                c.vel[1] = want;
                c.vel[0] = 0;
            }
            if c.air {
                air_total += 1;
                cur += 1;
                longest = longest.max(cur);
                let over = c.pos[1] - g.at(c.pos[0], c.pos[2]).height;
                peak = peak.max(over);
                if first_off.is_none() {
                    first_off = Some((n, c.pos[2] / M));
                }
            } else {
                cur = 0;
            }
        }
        println!(
            "{v} m/s: air steps total {air_total}, longest flight {longest}, peak {:.2} m, first off at {:?}",
            f64::from(peak) / f64::from(M),
            first_off
        );
    }
}

#[test]
#[ignore]
fn sizes_of_the_state_and_the_baked_stretch() {
    use core::mem::size_of_val;
    println!(
        "Drive {} B, Play {} B, Car {} B, Chase {} B",
        core::mem::size_of::<drive3d::Drive>(),
        core::mem::size_of::<drive3d::play::Play>(),
        core::mem::size_of::<Car>(),
        core::mem::size_of::<drive3d::camera::Chase>()
    );
    println!(
        "baked: ROAD {} B, LAND {} B, THINGS {} B",
        size_of_val(&drive3d::road::ROAD),
        size_of_val(&drive3d::land::LAND),
        size_of_val(&drive3d::things::THINGS)
    );
}

#[test]
#[ignore]
fn scripted_drives_at_three_speeds() {
    use drive3d::play::Play;
    for (cruise, corner) in [(14, 10), (20, 14), (25, 16)] {
        let mut p = Play::new();
        let mut finish = None;
        let mut most_air = 0u16;
        for _ in 0..3_000 {
            let input = common::autopilot(&p, cruise * M / 30, corner * M / 30);
            p.step(&input);
            most_air = most_air.max(p.car.air_steps);
            if finish.is_none() {
                finish = p.finished;
            }
            if p.finished.is_some() && p.car.speed() == 0 {
                break;
            }
        }
        println!(
            "test driver {cruise} m/s, corners {corner}: finish {:?} steps, resets {}, hits {}, flights {}, longest air {most_air} steps",
            finish, p.resets, p.hits, p.flights
        );
    }
}
