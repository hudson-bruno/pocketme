//! Step M3: the live docked scene. While docked the station turns and
//! traffic passes behind the menus, but no rule time passes: these tests
//! idle at the dock (DOCKED, MARKET, OUTFIT) and check that the career,
//! the ship, the prices, the world's clock and the flight after launch
//! are exactly what an immediate launch gives. Pictures of the docked
//! screens go to `.local/mercator/shots/` (names `50-dock-*`).

mod common;

use common::*;
use game_kit::button;
use mercator::app::{Scene, seed_of};
use mercator::fx;
use mercator::galaxy;
use mercator::scene;
use mercator::world::{self, Career, Kind, Pad, State};

/// Docked at the first system of `danger`, with a TAG unpaid and a
/// dented hull and some cargo, so every rule value is set.
fn docked_in(danger: u8) -> Driver {
    let mut d = Driver::new();
    d.start();
    let mut c = Career::new(seed_of(1));
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| galaxy::system(c.seed, i).danger == danger)
        .unwrap();
    c.target = c.system;
    c.hull = 63;
    c.hold.0[2] = 4;
    d.app.world.begin(c);
    d.app.world.car.tags = 1;
    d.app.world.car.owed = 40;
    d.app.last_dock = d.app.world.car;
    d.idle(2);
    assert_eq!(d.app.scene, Scene::Docked);
    d
}

/// Everything the rules read, as text (the world's own fields).
fn rules(d: &Driver) -> String {
    let w = &d.app.world;
    let rules_ships: Vec<_> = if w.state == State::Docked {
        // Docked, the ships are the scenery's (looks only).
        Vec::new()
    } else {
        w.ships.to_vec()
    };
    format!(
        "{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{} {} {:?}\n{:?}",
        w.car,
        w.sys,
        w.me,
        w.stock,
        w.state,
        w.ticks,
        w.wanted,
        (0..market_goods())
            .map(|g| (w.price(g), w.sell_price(g)))
            .collect::<Vec<_>>(),
        rules_ships,
    )
}

fn market_goods() -> usize {
    mercator::market::GOODS
}

/// From the dock: MARKET, OUTFIT, back, each idled for `calls`; then
/// LAUNCH.
fn visit_and_launch(d: &mut Driver, calls: u32) {
    d.idle(calls);
    d.app.sel = 1;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Market);
    d.idle(calls);
    d.tap(button::B);
    d.app.sel = 2;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Outfit);
    d.idle(calls);
    d.tap(button::B);
    assert_eq!(d.app.scene, Scene::Docked);
    d.idle(calls);
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
}

/// A fixed flight: turning, firing now and then (world level).
fn fly(d: &mut Driver, ticks: u32) -> Vec<world::Event> {
    let mut ev = Vec::new();
    for t in 0..ticks {
        let w = &mut d.app.world;
        if w.state != State::Flying {
            break;
        }
        let pad = Pad {
            steer: ((t as i32 % 300) - 150, 40),
            throttle: 80,
            fire: t % 50 < 10,
            ..Pad::default()
        };
        w.tick(&pad);
        while let Some(e) = w.take_event() {
            ev.push(e);
        }
    }
    ev
}

#[test]
fn docked_time_changes_no_rule_value() {
    for danger in [0u8, 3] {
        let mut idle = docked_in(danger);
        let mut quick = docked_in(danger);
        let at_dock = rules(&idle);
        // 4 x 30 s at the dock against 4 x 2 calls.
        let mut seen_ships = 0;
        for k in 0..4 {
            for _ in 0..1800 {
                idle.idle(1);
                let w = &idle.app.world;
                // No raider comes near a docked player, nobody fires.
                for s in w.ships.iter().flatten() {
                    assert!(matches!(s.kind, Kind::Hauler | Kind::Warden));
                    seen_ships += 1;
                }
                assert!(w.beams.iter().all(|b| b.age == 0));
            }
            assert_eq!(rules(&idle), at_dock, "danger {danger}: rule time passed");
            if idle.app.scene == Scene::Docked {
                idle.app.sel = if k == 0 { 1 } else { 2 };
                idle.tap(button::A);
            } else {
                idle.tap(button::B);
            }
        }
        if fx::DOCK_LIVE_ON {
            assert!(seen_ships > 0, "no traffic passed");
            assert!(idle.app.world.show > 0);
        }
        // Back to the dock, then out: the same flight as a quick launch.
        if idle.app.scene != Scene::Docked {
            idle.tap(button::B);
        }
        idle.app.sel = 0;
        idle.tap(button::A);
        visit_and_launch(&mut quick, 2);
        assert_eq!(idle.app.scene, Scene::Flight);
        assert_eq!(
            rules(&idle),
            rules(&quick),
            "danger {danger}: launch differs"
        );
        assert_eq!(idle.app.world.show, 0);
        assert_eq!(idle.app.world.ships, quick.app.world.ships);
        let (a, b) = (fly(&mut idle, 3600), fly(&mut quick, 3600));
        assert_eq!(a, b, "danger {danger}: the flights' events differ");
        assert_eq!(
            rules(&idle),
            rules(&quick),
            "danger {danger}: the flights differ"
        );
    }
}

#[test]
fn the_docked_scene_moves_and_the_pause_rests() {
    let mut d = docked_in(1);
    for scene in [Scene::Docked, Scene::Market, Scene::Outfit] {
        if scene != Scene::Docked {
            d.app.sel = if scene == Scene::Market { 1 } else { 2 };
            d.tap(button::A);
        }
        assert_eq!(d.app.scene, scene);
        assert!(d.app.docked_view());
        assert_eq!(d.app.still(), !fx::DOCK_LIVE_ON, "{scene:?}");
        let a = d.picture_hash();
        d.idle(30);
        if fx::DOCK_LIVE_ON {
            assert_ne!(d.picture_hash(), a, "{scene:?}: the scene stood still");
        }
        if scene != Scene::Docked {
            // START leaves MARKET and OUTFIT (as before); B too.
            d.tap(button::B);
            assert_eq!(d.app.scene, Scene::Docked);
            continue;
        }
        // Paused (from the dock): still, and nothing moves.
        d.tap(button::START);
        assert!(d.app.pause.is_some());
        assert!(d.app.still());
        let show = d.app.world.show;
        d.idle(1);
        let p = d.picture_hash();
        d.idle(60);
        assert_eq!(d.picture_hash(), p, "the pause moved");
        assert_eq!(d.app.world.show, show);
        d.tap(button::START);
        assert!(d.app.pause.is_none());
    }
    // The chart from the dock stays a still picture.
    d.tap(button::SELECT);
    assert_eq!(d.app.scene, Scene::Chart);
    assert!(d.app.still());
}

#[test]
fn the_docked_scene_is_the_same_every_run() {
    let run = || {
        let mut d = docked_in(3);
        let mut h = Vec::new();
        for k in 0..40 {
            d.idle(45);
            if k == 20 {
                d.app.sel = 1;
                d.tap(button::A);
            }
            h.push(d.picture_hash());
        }
        h
    };
    assert_eq!(run(), run());
}

#[test]
fn the_docked_screens_have_pictures() {
    for danger in [0u8, 3] {
        let mut d = docked_in(danger);
        shot(&mut d, &format!("50-dock-{danger}-0s"));
        for (s, calls) in [(2, 120), (10, 480), (30, 1200)] {
            d.idle(calls);
            shot(&mut d, &format!("50-dock-{danger}-{s}s"));
        }
        d.app.sel = 1;
        d.tap(button::A);
        d.idle(40);
        shot(&mut d, &format!("51-market-{danger}"));
        d.tap(button::B);
        d.app.sel = 2;
        d.tap(button::A);
        d.idle(40);
        shot(&mut d, &format!("52-outfit-{danger}"));
        d.tap(button::B);
        d.tap(button::START);
        d.idle(2);
        shot(&mut d, &format!("53-dock-paused-{danger}"));
    }
    // The drift stays within its sway, and starts at the old view.
    let mut d = docked_in(0);
    assert_eq!(scene::dock_turn(&d.app.world), scene::DOCK_TURN);
    for _ in 0..80 {
        d.idle(60);
        let t = i32::from(scene::dock_turn(&d.app.world)) - i32::from(scene::DOCK_TURN);
        assert!(t.abs() <= scene::DOCK_SWAY);
    }
}
