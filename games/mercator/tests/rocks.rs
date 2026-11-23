//! Step L4: drifting rocks far from the station. They are never
//! scooped; shot apart, each leaves a canister of ORE or ICE. Pictures
//! go to `.local/mercator/shots/` (names `70-rocks-*`).

mod common;

use common::*;
use game_kit::button;
use mercator::app::{Scene, seed_of};
use mercator::fixed::{Mat3, ONE14, V3, scale14, sub};
use mercator::galaxy;
use mercator::scene::Detail;
use mercator::world::{self, Career, FIGHT, KM, Kind, M, Pad, ROCK, SHIPS, World};

fn out_in(danger: u8) -> Driver {
    let mut d = Driver::new();
    d.start();
    let mut c = Career::new(seed_of(1));
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| galaxy::system(c.seed, i).danger == danger)
        .unwrap();
    c.target = c.system;
    d.app.world.begin(c);
    d.app.last_dock = c;
    d.idle(2);
    assert_eq!(d.app.scene, Scene::Docked);
    d.app.sel = 0;
    d.tap(button::A);
    d.app.opts.detail = Detail::High;
    d
}

fn hold(w: &mut World, pos: V3, at: V3) {
    w.me.pos = pos;
    w.me.rot = Mat3::looking(sub(at, pos), [0, ONE14, 0]);
    w.me.speed = 0;
    w.me.throttle = 0;
    w.car.hull = world::HULL_MAX as u8;
}

fn rocks(w: &World) -> Vec<usize> {
    (FIGHT..SHIPS)
        .filter(|&i| w.ships[i].is_some_and(|s| s.kind == Kind::Canister && s.load.0 == ROCK))
        .collect()
}

/// Ticks 60 km out until a rock field is there; the rocks' slots.
fn field(d: &mut Driver) -> Vec<usize> {
    let b = d.app.world.bodies;
    let far = world::add(b.station_pos, scale14(b.station_rot.col(2), 60 * KM));
    let at = world::add(far, [0, 0, 10 * KM]);
    for _ in 0..60 * 900 {
        let w = &mut d.app.world;
        hold(w, far, at);
        for s in w.ships[..FIGHT].iter_mut() {
            *s = None;
        }
        // Only rocks stay: the field rolls when three slots are free.
        for i in FIGHT..SHIPS {
            if w.ships[i].is_some_and(|s| s.load.0 != ROCK) {
                w.ships[i] = None;
            }
        }
        w.tick(&Pad::default());
        let r = rocks(w);
        if r.len() == 3 {
            return r;
        }
    }
    panic!("no rock field");
}

#[test]
fn a_rock_field_drifts_ahead_and_is_never_scooped() {
    let mut d = out_in(1);
    let r = field(&mut d);
    let w = &mut d.app.world;
    let me = w.me.pos;
    for &i in &r {
        let s = w.ships[i].unwrap();
        assert!(world::within(s.pos, me, 4 * KM), "a rock far off");
        assert!(matches!(s.load.1, 1 | 2), "ICE or ORE");
    }
    d.idle(1);
    shot(&mut d, "70-rocks-field");
    let st = d.app.render.stats;
    println!(
        "rock field view: {} triangles, {} pixels",
        st.tris, st.pixels
    );
    // Touching a rock scoops nothing.
    let w = &mut d.app.world;
    let i = r[0];
    let hold_before = w.car.hold;
    for _ in 0..30 {
        let p = w.ships[i].unwrap().pos;
        w.me.pos = p;
        w.tick(&Pad::default());
    }
    assert!(w.ships[i].is_some_and(|s| s.load.0 == ROCK));
    assert_eq!(w.car.hold, hold_before);
}

#[test]
fn a_rock_shot_apart_leaves_ore_or_ice_to_scoop() {
    let mut d = out_in(0);
    let r = field(&mut d);
    let i = r[0];
    let good = d.app.world.ships[i].unwrap().load.1;
    let w = &mut d.app.world;
    // Close in, 300 m, nose on it; fire until it breaks.
    let mut shots = 0;
    for _ in 0..60 * 20 {
        let s = w.ships[i].unwrap();
        if s.load.0 != ROCK {
            break;
        }
        let back = world::add(s.pos, [0, 0, -300 * M]);
        hold(w, back, s.pos);
        w.me.heat = 0;
        w.tick(&Pad {
            fire: true,
            ..Pad::default()
        });
        shots += 1;
    }
    let c = w.ships[i].expect("a canister in the rock's place");
    assert_eq!(c.kind, Kind::Canister);
    assert_eq!(c.load.0, good, "the rock's good");
    assert!((1..=3).contains(&c.load.1));
    assert!(!w.wanted, "breaking a rock is no crime");
    println!("rock broken after {shots} ticks of fire");
    d.idle(30);
    let w = &mut d.app.world;
    let p = w.ships[i].unwrap().pos;
    let back = world::add(p, [0, 0, -260 * M]);
    hold(w, back, p);
    d.idle(1);
    shot(&mut d, "71-rocks-broken");
    // Scooped like any cargo.
    let w = &mut d.app.world;
    let before = w.car.hold.0[usize::from(good)];
    w.me.pos = w.ships[i].unwrap().pos;
    w.tick(&Pad::default());
    assert!(w.car.hold.0[usize::from(good)] > before);
}

/// Flying straight on far from the station for ten minutes: rock
/// fields and convoys both come along, now and then.
#[test]
fn rock_fields_and_convoys_come_along() {
    let mut d = out_in(1);
    let w = &mut d.app.world;
    let b = w.bodies;
    let out = b.station_rot.col(2);
    w.me.pos = world::add(b.station_pos, scale14(out, 15 * KM));
    let (mut fields, mut convoys) = (0, 0);
    let mut seen = [false; SHIPS];
    for _ in 0..60 * 600 {
        w.me.rot = Mat3::looking(out, [0, ONE14, 0]);
        w.me.speed = 900;
        w.me.throttle = 900;
        w.car.hull = world::HULL_MAX as u8;
        // A long trip: back to 15 km out now and then.
        if !world::within(w.me.pos, b.station_pos, 200 * KM) {
            w.me.pos = world::add(b.station_pos, scale14(out, 15 * KM));
        }
        w.tick(&Pad::default());
        for (slot, was) in w.ships[FIGHT..].iter().zip(seen[FIGHT..].iter_mut()) {
            if let Some(s) = slot.filter(|_| !*was) {
                if s.load.0 == ROCK {
                    fields += 1;
                } else if s.kind == Kind::Hauler && s.lead == world::NOBODY {
                    convoys += 1;
                }
            }
            *was = slot.is_some();
        }
    }
    println!("ten minutes: {fields} rocks, {convoys} lead haulers");
    assert!(fields >= 6, "{fields} rocks");
    assert!(convoys >= 3, "{convoys} convoys");
}
