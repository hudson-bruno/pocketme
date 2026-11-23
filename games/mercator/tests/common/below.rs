//! Step G1: getting to the planet the game's own way (docked, launch,
//! the ship put over the ring nose down, the ticks run until it enters).
#![allow(dead_code)]

use super::Driver;
use game_kit::button;
use mercator::app::{Scene, seed_of};
use mercator::fixed::{Mat3, ONE14, scale14};
use mercator::galaxy;
use mercator::planet::space;
use mercator::world::{self, Career, M};

/// The first system (galaxy, number) with planet `look`, galaxies 1..=10.
pub fn system_with_look(look: u8) -> (u8, u8) {
    for g in 1..=10u8 {
        let seed = seed_of(g);
        for i in 0..galaxy::SYSTEMS as u8 {
            if galaxy::system(seed, i).look == look {
                return (g, i);
            }
        }
    }
    panic!("no system with look {look}");
}

/// A driver docked at system `id` of galaxy `g`.
pub fn docked_at(g: u8, id: u8) -> Driver {
    let mut d = Driver::new();
    d.start();
    let mut c = Career::new(seed_of(g));
    c.system = id;
    c.target = id;
    d.app.world.begin(c);
    d.app.last_dock = c;
    d.idle(2);
    d
}

/// From the dock: launch, then the ship put `up` over the ring's middle,
/// nose down, at 600 (no raiders), and flown until it is below.
pub fn dive(d: &mut Driver, up: i32) {
    assert_eq!(d.app.scene, Scene::Docked);
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    let s = space::site(&d.app.world).expect("a site");
    let w = &mut d.app.world;
    w.ships = [None; world::SHIPS];
    w.me.pos = world::add(s.ring, scale14(s.dir, up));
    w.me.rot = Mat3::looking(scale14(s.dir, -ONE14), space::across(s.dir).0);
    w.me.speed = 600;
    w.me.throttle = 600;
    for _ in 0..200 {
        if d.app.scene == Scene::Below {
            return;
        }
        d.idle(1);
    }
    panic!("the ship never went below");
}

/// A driver below at the first system with `look`.
pub fn below_in(look: u8) -> Driver {
    let (g, id) = system_with_look(look);
    let mut d = docked_at(g, id);
    dive(&mut d, 30 * M);
    d
}

// ---- Step G2: the landing pilot -----------------------------------------------------

use mercator::app::{DEAD_ZONE, Keys};
use mercator::fixed::{cos, sin};
use mercator::planet::Below;
use mercator::planet::lander::{self, In, MPS, Phase};

/// A scripted pilot's input for this tick (it measures the rules, not a
/// human's skill): fly to the pad's middle at a safe height with the
/// assist on, let the assist bring the ship down over it, and add a
/// touch of lift in the last metres. On the ground: nothing.
pub fn pilot(b: &Below) -> In {
    if b.phase != Phase::Fly {
        return In::default();
    }
    let at = lander::at(b);
    let alt = lander::alt(b);
    // Horizontal: a speed toward the pad that can still be braked.
    let (dx, dz) = (-i64::from(at[0]), -i64::from(at[2]));
    let dist = ((dx * dx + dz * dz) as f64).sqrt() / f64::from(M);
    let want = (dist * 0.8).sqrt().min(1.0) * (2.0 * 2.0 * dist).sqrt().min(9.0);
    let (wx, wz) = if dist > 0.05 {
        (
            dx as f64 / f64::from(M) / dist * want,
            dz as f64 / f64::from(M) / dist * want,
        )
    } else {
        (0.0, 0.0)
    };
    let mps = f64::from(MPS);
    let (vx, vz) = (f64::from(b.v[0]) / mps, f64::from(b.v[2]) / mps);
    let (ex, ez) = (wx - vx, wz - vz);
    // Into the camera's frame (right, ahead).
    let (s, c) = (
        f64::from(sin(b.cam_yaw)) / 16384.0,
        f64::from(cos(b.cam_yaw)) / 16384.0,
    );
    let gain = 256.0 / 1.5;
    let pr = ((ex * c - ez * s) * gain).clamp(-256.0, 256.0) as i32;
    let pf = ((ex * s + ez * c) * gain).clamp(-256.0, 256.0) as i32;
    // Vertical: keep 40 m or more while far out; in the last 3 m ask for
    // a slow touch; between, the assist.
    let alt_m = f64::from(alt) / f64::from(lander::FINE);
    let vy = f64::from(b.v[1]) / mps;
    let target = if dist > 12.0 && alt_m < 40.0 {
        Some(1.0)
    } else if dist > 12.0 {
        Some(0.0)
    } else if alt_m < 3.0 {
        Some(-0.6)
    } else {
        None
    };
    let lift = target.map_or(0, |t| ((t - vy) * 256.0).clamp(0.0, 256.0) as i32);
    In {
        held: 0,
        pressed: 0,
        push: [pr, pf],
        lift,
        finger: None,
        look: [0, 0],
    }
}

/// The App's keys that give the lander this input (the left stick for
/// the push, the right trigger for the lift).
pub fn keys_for(i: &In, held_before: u32) -> Keys {
    let stick = |p: i32| -> i16 {
        if p == 0 {
            0
        } else {
            let m = DEAD_ZONE + p.abs() * (32_767 - DEAD_ZONE) / 256 + 1;
            (p.signum() * m.min(32_767)) as i16
        }
    };
    Keys {
        held: i.held,
        pressed: i.held & !held_before,
        left: (stick(i.push[0]), -stick(i.push[1])),
        triggers: (0, (i.lift.clamp(0, 256) * 32_767 / 256) as u16),
        ..Keys::default()
    }
}

/// One call of the game with the pilot's keys.
pub fn fly_call(d: &mut Driver) {
    let i = pilot(&d.app.below);
    let k = keys_for(&i, d.held);
    d.held = i.held;
    d.call_keys(k);
}

/// Below, from the entry: the pilot flies until the ship is down (or
/// `max` calls). Returns the calls it took.
pub fn land(d: &mut Driver, max: u32) -> u32 {
    for n in 0..max {
        if d.app.below.phase == Phase::Landed || d.app.scene != Scene::Below {
            return n;
        }
        fly_call(d);
    }
    panic!("the pilot did not land in {max} calls");
}
