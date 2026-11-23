//! Step F2: the auto-dock's way in is about as long as before step M2
//! (its speed falls with the way left instead of 420 all the way in),
//! and a raider is hit about as often as before M2 (its hit size is 72
//! m, smaller than it is drawn, but well over its body).
//!
//! The measures behind the numbers are in `f2_measure.rs` (ignored).

use mercator::app::seed_of;
use mercator::fixed::{Mat3, ONE14, scale14, sub};
use mercator::world::{self, Career, Event, FIGHTER_X, KM, Kind, M, Mode, Pad, State, World};

/// Flying in the start system of galaxy 1, nothing else in space.
fn flying() -> Box<World> {
    let mut w = Box::new(World::empty());
    w.begin(Career::new(seed_of(1)));
    w.launch();
    w.ships = [None; world::SHIPS];
    w
}

/// From `out` on the station's axis (and `side` across it), facing the
/// station at half speed, the auto-dock flies in through the mouth: no
/// crash, at most 300 a tick at the dock, the speed below cruise never
/// changes by more than 24 a tick, and the time it took (s).
fn dock_from(out: i32, side: i32) -> f64 {
    let mut w = flying();
    let b = w.bodies;
    w.me.pos = world::add(
        world::add(b.station_pos, scale14(b.station_rot.col(2), out)),
        scale14(b.station_rot.col(0), side),
    );
    w.me.rot = Mat3::looking(sub(b.station_pos, w.me.pos), [0, ONE14, 0]);
    w.me.speed = world::MAX_SPEED / 2;
    let mut t = 0;
    loop {
        let (v, cruise) = (w.me.speed, w.me.cruise);
        w.tick(&Pad {
            auto: !w.me.auto,
            ..Pad::default()
        });
        t += 1;
        // (Cruise speeds change on their own rule; cruise ends 6 km out.)
        if !cruise && !w.me.cruise && v <= world::MAX_SPEED && w.state == State::Flying {
            assert!(
                (w.me.speed - v).abs() <= 24,
                "a snap from {v} to {}",
                w.me.speed
            );
        }
        while let Some(e) = w.take_event() {
            assert_ne!(
                e,
                Event::Crash,
                "from {} m out, {} m aside",
                out / M,
                side / M
            );
            if e == Event::Docked {
                assert!(v <= 300, "docked at {v}");
            }
        }
        w.ships = [None; world::SHIPS];
        if w.state == State::Docked {
            return f64::from(t) / 60.0;
        }
        assert!(t < 60 * 120, "from {} m: no dock after 2 minutes", out / M);
    }
}

#[test]
fn the_auto_dock_flies_in_about_as_fast_as_before_m2() {
    // MEASURED ON THE PC with this same test: after step F1 (420 all the
    // way in from up to 6 km out) 69.0 s from 12 km, 58.7 s from 6 km
    // and 41.8 s from 5 km out 2 km aside; after F2 48.0, 37.2 and 35.5.
    let far = dock_from(12 * KM, 0);
    let near = dock_from(6 * KM, 0);
    let aside = dock_from(5 * KM, 2 * KM);
    println!(
        "auto-dock: from 12 km {far:.1} s, from 6 km {near:.1} s, from 5 km out 2 km aside {aside:.1} s"
    );
    assert!(far < 55.0, "{far}");
    assert!(near < 45.0, "{near}");
    assert!(aside < 39.0, "{aside}");
}

/// Whether a shot now would hit a raider `dist` m ahead, `off` m to the
/// side of the nose line.
fn hits(dist: i32, off: i32) -> bool {
    let mut w = flying();
    let (pos, ahead, right) = (w.me.pos, w.me.rot.col(2), w.me.rot.col(0));
    let p = world::add(
        world::add(pos, scale14(ahead, dist * M)),
        scale14(right, off * M),
    );
    let i = w.add_ship(Kind::Raider, p, Mode::Travel).unwrap();
    w.first_hit().is_some_and(|(j, _)| j == i)
}

#[test]
fn a_shot_through_a_raiders_hull_hits_and_one_past_its_claws_misses() {
    // READ FROM THE CODE (scene models, biggest size step): the body ring
    // is about 45 m round the middle, the claw tips about 150 m out.
    assert_eq!(Kind::Raider.size(), 24 * M * FIGHTER_X);
    let size = Kind::Raider.size() / M;
    for dist in [300, 600, 1000, 1600, 2300] {
        // Through the middle and through the body: a hit.
        assert!(hits(dist, 0), "middle at {dist} m");
        assert!(hits(dist, 45), "body at {dist} m");
        // The hit size plus 1/40 of the distance, and a little past it.
        let reach = size + dist / 40;
        assert!(hits(dist, reach - 2), "edge at {dist} m");
        assert!(!hits(dist, reach + 2), "past the edge at {dist} m");
        // Past the claw tips: a miss.
        assert!(!hits(dist, 155 + dist / 40), "past the claws at {dist} m");
    }
}
