//! The extra traffic of step L2: ships leaving and arriving at the
//! station, a patrol, convoys with escorts or raiders on their tail.
//! Pictures go to `.local/mercator/shots/` (names `50-traffic-*`).

mod common;

use common::*;
use mercator::app::seed_of;
use mercator::fixed::{Mat3, ONE14, V3, scale14, sub};
use mercator::galaxy;
use mercator::scene::{Detail, Eye};
use mercator::world::{self, Career, FIGHT, HULL_MAX, KM, Kind, M, NOBODY, Pad, SHIPS, World};

fn docked_in(danger: u8) -> Driver {
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
    d.app.sel = 0;
    d.tap(game_kit::button::A);
    d.app.opts.detail = Detail::High;
    d.app.opts.eye = Eye::Chase;
    d
}

/// The player held still at `pos`, looking at `at`, and kept alive.
fn hold(w: &mut World, pos: V3, at: V3) {
    w.me.pos = pos;
    w.me.rot = Mat3::looking(sub(at, pos), [0, ONE14, 0]);
    w.me.speed = 0;
    w.me.throttle = 0;
    w.car.hull = HULL_MAX as u8;
}

fn traffic(w: &World) -> impl Iterator<Item = &world::Ship> {
    w.ships[FIGHT..].iter().flatten()
}

#[test]
fn ships_come_and_go_at_the_station() {
    let mut d = docked_in(0);
    let b = d.app.world.bodies;
    let axis = b.station_rot.col(2);
    let side = b.station_rot.col(0);
    let eye = world::add(
        world::add(b.station_pos, scale14(axis, 2500 * M)),
        scale14(side, 900 * M),
    );
    let (mut most, mut out, mut patrol) = (0, false, false);
    for _ in 0..60 * 90 {
        let w = &mut d.app.world;
        hold(w, eye, b.station_pos);
        w.tick(&Pad::default());
        most = most.max(traffic(w).count());
        out |= traffic(w).any(|s| s.kind == Kind::Hauler && s.timer > 0);
        patrol |= traffic(w).any(|s| s.kind == Kind::Warden);
    }
    println!("station traffic: at most {most} extra ships");
    assert!(most >= 2, "too little traffic: {most}");
    assert!(out, "no ship left the station");
    assert!(patrol, "no patrol");
    d.idle(1);
    shot(&mut d, "50-traffic-station");
    let s = d.app.render.stats;
    println!(
        "station traffic view: {} triangles, {} pixels",
        s.tris, s.pixels
    );
}

/// Ticks far from the station until a convoy has formed up; the
/// leader's slot.
fn convoy(d: &mut Driver) -> usize {
    let b = d.app.world.bodies;
    let far = world::add(b.station_pos, scale14(b.station_rot.col(2), 60 * KM));
    for _ in 0..60 * 600 {
        let w = &mut d.app.world;
        hold(w, far, b.station_pos);
        // The fights' own raiders are cleared: this is about the convoy.
        for s in w.ships[..FIGHT].iter_mut() {
            *s = None;
        }
        w.tick(&Pad::default());
        let formed = (FIGHT..SHIPS).find(|&i| {
            w.ships[i].is_some_and(|l| {
                l.kind == Kind::Hauler
                    && l.lead == NOBODY
                    && traffic(w).any(|s| usize::from(s.lead) == i)
                    && traffic(w)
                        .filter(|s| usize::from(s.lead) == i)
                        // Step M2: 450 m times the kind's growth.
                        .all(|s| world::within(s.pos, l.pos, 450 * M * s.kind.grown()))
            })
        });
        if let Some(i) = formed {
            return i;
        }
    }
    panic!("no convoy formed");
}

/// The view from beside the convoy's leader.
fn look_at(d: &mut Driver, lead: usize) {
    let w = &mut d.app.world;
    let l = w.ships[lead].unwrap();
    let eye = world::add(
        world::add(l.pos, scale14(l.rot.col(0), 700 * M)),
        scale14(l.rot.col(2), 500 * M),
    );
    hold(w, eye, l.pos);
    d.idle(1);
}

#[test]
fn a_convoy_crosses_with_its_escort() {
    let mut d = docked_in(1);
    let lead = convoy(&mut d);
    let w = &d.app.world;
    assert!(traffic(w).any(|s| s.kind == Kind::Warden && usize::from(s.lead) == lead));
    look_at(&mut d, lead);
    shot(&mut d, "51-traffic-convoy");
    let s = d.app.render.stats;
    println!("convoy view: {} triangles, {} pixels", s.tris, s.pixels);
}

#[test]
fn raiders_go_for_a_convoy_before_the_player() {
    let mut d = docked_in(3);
    let lead = convoy(&mut d);
    let w = &d.app.world;
    let raiders: Vec<usize> = (FIGHT..SHIPS)
        .filter(|&i| w.ships[i].is_some_and(|s| s.kind == Kind::Raider))
        .collect();
    assert!(!raiders.is_empty(), "no raiders on the convoy's tail");
    // Some seconds on: they fire at the haulers, not at the player.
    let start = d.app.world.ships[lead].unwrap().pos;
    let mut at_haulers = false;
    for _ in 0..60 * 40 {
        let w = &mut d.app.world;
        let me = w.me.pos;
        hold(w, me, start);
        w.tick(&Pad::default());
        at_haulers |= raiders.iter().any(|&i| {
            w.ships[i].is_some_and(|s| {
                s.kind == Kind::Raider && s.target != NOBODY && usize::from(s.target) < SHIPS
            })
        });
        if at_haulers {
            break;
        }
    }
    assert!(at_haulers, "the raiders never went for a hauler");
    let w = &d.app.world;
    if let Some(&r) = raiders.iter().find(|&&i| w.ships[i].is_some()) {
        look_at(&mut d, r);
        shot(&mut d, "52-traffic-raiders");
    }
}

#[test]
fn a_raider_pack_closes_in_as_a_v() {
    let mut d = docked_in(3);
    let w = &mut d.app.world;
    let b = w.bodies;
    let far = world::add(b.station_pos, scale14(b.station_rot.col(2), 60 * KM));
    hold(w, far, b.station_pos);
    let leader = w.add_ship(
        Kind::Raider,
        world::add(far, [0, 0, 7 * KM]),
        world::Mode::Hunt,
    );
    let leader = leader.unwrap();
    for k in 1..3u8 {
        // Step M2: 300 m times FIGHTER_X apart, as the game puts them.
        let p = world::add(far, [i32::from(k) * 300 * M * world::FIGHTER_X, 0, 7 * KM]);
        let j = w.add_ship(Kind::Raider, p, world::Mode::Hunt).unwrap();
        let s = w.ships[j].as_mut().unwrap();
        s.lead = leader as u8;
        s.wing = k;
    }
    // Ten seconds: still in formation, each near its place.
    for _ in 0..60 * 10 {
        hold(w, far, b.station_pos);
        w.tick(&Pad::default());
    }
    let l = w.ships[leader].unwrap();
    assert!(
        !world::within(l.pos, far, 2 * KM),
        "the leader is already close"
    );
    for s in w.ships.iter().flatten().filter(|s| s.lead == leader as u8) {
        let near = 450 * M * world::FIGHTER_X;
        assert!(world::within(s.pos, l.pos, near), "a wing fell behind");
    }
    // Close in: they break off and hunt on their own.
    for _ in 0..60 * 20 {
        hold(w, far, b.station_pos);
        w.tick(&Pad::default());
    }
    assert!(w.ships.iter().flatten().all(|s| s.lead == NOBODY));
}
