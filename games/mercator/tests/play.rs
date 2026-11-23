//! Scripted play: a trader who makes money, fights that end, a career
//! that is won, the auto-dock, a lost ship, the raiders' TAGs and their
//! bounty, the save bytes.

mod common;

use common::*;
use game_kit::button;
use mercator::app::{Event, Scene, seed_of};
use mercator::galaxy;
use mercator::market::{self, GOODS};
use mercator::save::{self, Bad, SIZE};
use mercator::world::{
    self, Career, KM, Kind, M, Mode, PLAYER, Pad, State, TANK, WIN_CREDITS, World,
};

fn world_at_start(galaxy: u8) -> Box<World> {
    let mut w = Box::new(World::empty());
    w.begin(Career::new(seed_of(galaxy)));
    w
}

/// Docked at the first system of `danger` in galaxy `galaxy`.
fn world_in(galaxy: u8, danger: u8) -> Box<World> {
    let mut c = Career::new(seed_of(galaxy));
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| mercator::galaxy::system(c.seed, i).danger == danger)
        .unwrap();
    let mut w = Box::new(World::empty());
    w.begin(c);
    w
}

/// Up to `trips` trading trips through systems of danger at most
/// `max_danger`; returns (credits after each trip, deaths, kills,
/// ticks flown).
fn trade(w: &mut World, trips: usize, max_danger: u8) -> (Vec<u32>, u32, u32, u64) {
    let mut log = Vec::new();
    let (mut deaths, mut kills, mut ticks) = (0, 0, 0u64);
    for _ in 0..trips {
        sell_all(w);
        log.push(w.car.credits);
        // Step M5: the cargo ship first, when it pays (as the big bay
        // before it: with 1,100 credits left for cargo).
        if w.car.ship == world::LARK
            && w.ship_sold_here(usize::from(world::DRAY))
            && i64::from(w.car.credits)
                > 1_100 + i64::from(w.car.ship_cost(usize::from(world::DRAY)))
        {
            w.buy_ship(usize::from(world::DRAY)).unwrap();
        }
        let (to, g, profit) = plan_any(w, max_danger);
        if profit > 0 {
            buy_max(w, g, 0);
        }
        let last_dock = w.car;
        match trip(w, to, 60 * 60 * 10) {
            Trip::Docked { ticks: t, kills: k } => {
                ticks += u64::from(t);
                kills += k;
            }
            Trip::Died { ticks: t } => {
                ticks += u64::from(t);
                deaths += 1;
                w.begin(last_dock.wrecked());
            }
            Trip::TimedOut => panic!("a trip to {to} never ended (state {:?})", w.state),
        }
    }
    sell_all(w);
    log.push(w.car.credits);
    (log, deaths, kills, ticks)
}

#[test]
fn a_scripted_trade_run_makes_money() {
    let mut w = world_at_start(1);
    let start = w.car.credits;
    let (log, deaths, kills, ticks) = trade(&mut w, 12, 1);
    println!("credits after each trip: {log:?}");
    println!(
        "12 trips: {} -> {} credits, {deaths} lost, {kills} kills, {:.1} min flown, day {}",
        start,
        w.car.credits,
        ticks as f64 / 3600.0,
        w.car.day
    );
    assert!(w.car.credits > start * 3, "trading must pay");
    assert_eq!(deaths, 0, "the safe routes are safe");
}

/// The safe routes are safe by the game's rules, not by one lucky
/// seed: 8 galaxies, 6 start days each, 20 trips each (960 trips),
/// no ship lost. (In SECURE and QUIET systems no raiders appear within
/// 8 km of the station, so a fight never pins a trader at the mouth.)
#[test]
fn the_safe_routes_are_safe_in_every_galaxy_and_season() {
    let (mut trips, mut deaths, mut kills) = (0, 0, 0);
    for galaxy in 1..=8 {
        for season in 0..6u16 {
            let mut c = Career::new(seed_of(galaxy));
            c.day = season * 61;
            let mut w = Box::new(World::empty());
            w.begin(c);
            let (_, d, k, _) = trade(&mut w, 20, 1);
            assert_eq!(d, 0, "galaxy {galaxy}, day {}: ship lost", season * 61);
            trips += 20;
            deaths += d;
            kills += k;
        }
    }
    println!("{trips} safe trips: {deaths} lost, {kills} raiders downed");
}

#[test]
fn every_galaxy_trades_at_a_profit() {
    for n in 1..=8 {
        let mut w = world_at_start(n);
        let start = w.car.credits;
        let (log, deaths, _, ticks) = trade(&mut w, 5, 1);
        println!(
            "galaxy {n}: {log:?}, {deaths} lost, {:.1} min",
            ticks as f64 / 3600.0
        );
        assert!(w.car.credits > start, "galaxy {n} lost money");
    }
}

/// A raider pack round the player, in a lawless system.
fn ambush(w: &mut World, raiders: usize) {
    w.launch();
    // Out of the station's reach first.
    w.me.pos = world::add(w.bodies.station_pos, [0, 0, 0]);
    let out = w.bodies.station_rot.col(2);
    w.me.pos = world::add(w.me.pos, mercator::fixed::scale14(out, 12 * KM));
    for k in 0..raiders {
        let p = world::add(
            w.me.pos,
            mercator::fixed::scale14(w.me.rot.col(2), (2 + k as i32) * KM),
        );
        w.add_ship(
            Kind::Raider,
            world::add(p, [k as i32 * 300 * M, 0, 0]),
            Mode::Hunt,
        );
    }
}

#[test]
fn a_scripted_fight_ends() {
    let mut endings = Vec::new();
    let mut won = 0;
    let mut lost = 0;
    for (galaxy, danger, laser, shield, raiders) in [
        (1u8, 0u8, 0u8, 0u8, 1usize),
        (2, 1, 0, 0, 1),
        (3, 1, 0, 0, 2),
        (4, 2, 0, 0, 2),
        (5, 2, 1, 1, 2),
        (6, 3, 0, 0, 3),
        (7, 3, 1, 1, 3),
        (8, 3, 2, 2, 3),
    ] {
        let mut w = world_in(galaxy, danger);
        w.car.laser = laser;
        w.car.shield = shield;
        ambush(&mut w, raiders);
        let mut t = 0;
        let end = loop {
            if w.state == State::Dead {
                break "lost";
            }
            let pad = match hunter(&w, 20 * KM) {
                Some(i) => fight_pad(&w, i),
                None => break "won",
            };
            w.tick(&pad);
            while w.take_event().is_some() {}
            t += 1;
            assert!(t < 60 * 60 * 5, "a fight still going after 5 minutes");
        };
        endings.push(format!(
            "{} (danger {}), laser {laser} shield {shield} vs {raiders}: {end} after {:.1} s, {} kills, hull {}, shield {}",
            w.sys.name().as_str(),
            w.sys.danger,
            t as f64 / 60.0,
            w.car.kills,
            w.car.hull,
            w.me.shield
        ));
        if end == "won" {
            assert!(w.car.kills as usize >= 1);
            won += 1;
        } else {
            lost += 1;
        }
    }
    println!("fights won {won}, lost {lost}");
    for e in &endings {
        println!("{e}");
    }
}

#[test]
fn a_kill_pays_nothing_at_once_and_a_hauler_kill_makes_you_wanted() {
    let mut w = world_at_start(1);
    ambush(&mut w, 1);
    let before = w.car.credits;
    for _ in 0..60 * 120 {
        let Some(i) = hunter(&w, 20 * KM) else { break };
        let pad = fight_pad(&w, i);
        w.tick(&pad);
        while w.take_event().is_some() {}
    }
    assert_eq!(w.car.kills, 1);
    assert_eq!(w.car.credits, before, "a kill pays nothing at once");
    assert_eq!(tags_in_space(&w).len(), 1, "the raider dropped its TAG");
    assert!(!w.wanted);
    // Now a hauler, shot on purpose.
    let ahead = world::add(w.me.pos, mercator::fixed::scale14(w.me.rot.col(2), 600 * M));
    let i = w.add_ship(Kind::Hauler, ahead, Mode::Travel).unwrap();
    let mut wanted = false;
    for _ in 0..60 * 60 {
        let Some(s) = w.ships[i] else { break };
        if s.kind != Kind::Hauler {
            break;
        }
        let pad = Pad {
            steer: steer_to(&w, s.pos),
            fire: w.me.heat < world::HEAT_MAX / 2,
            throttle: -256,
            ..Pad::default()
        };
        w.tick(&pad);
        while let Some(e) = w.take_event() {
            wanted |= e == world::Event::Wanted;
        }
    }
    assert!(wanted && w.wanted, "shooting a hauler must make you wanted");
}

#[test]
fn the_auto_dock_docks_from_anywhere_round_the_station() {
    let mut worst = 0;
    for k in 0..120u32 {
        let mut w = world_at_start(1 + (k % 8) as u8);
        w.launch();
        w.ships = [None; world::SHIPS];
        // A point on a sphere 3 to 9 km round the station, behind it
        // too, turned any way.
        let h = mercator::fixed::hash(k, 77);
        let d = mercator::fixed::norm(
            [
                (h % 2001) as i32 - 1000,
                ((h >> 11) % 2001) as i32 - 1000,
                ((h >> 22) % 1001) as i32 - 1000 + (k as i32 % 2) * 1000,
            ],
            mercator::fixed::ONE14,
        );
        let r = (3 + (k % 7) as i32) * KM;
        w.me.pos = world::add(w.bodies.station_pos, mercator::fixed::scale14(d, r));
        // Not inside the planet: lifted 1 km over its ground.
        let alt = mercator::fixed::length(w.me.pos) - w.bodies.planet_r;
        if alt < KM {
            let up = mercator::fixed::norm(w.me.pos, mercator::fixed::ONE14);
            w.me.pos = mercator::fixed::scale14(up, w.bodies.planet_r + KM);
        }
        w.me.rot = mercator::fixed::Mat3::rotation((h >> 3) as u16, (h >> 9) as u16, 0);
        let mut t = 0u32;
        let mut crashes = 0;
        while w.state != State::Docked {
            let pad = Pad {
                auto: !w.me.auto,
                ..Pad::default()
            };
            w.tick(&pad);
            while let Some(e) = w.take_event() {
                crashes += u32::from(e == world::Event::Crash);
            }
            // No raiders here: the auto-dock may not stop.
            w.ships = [None; world::SHIPS];
            t += 1;
            assert!(t < 60 * 120, "start {k}: no dock after 2 minutes");
        }
        assert_eq!(crashes, 0, "start {k}: the auto-dock crashed");
        worst = worst.max(t);
    }
    println!("slowest auto-dock: {:.1} s", f64::from(worst) / 60.0);
}

/// Docks with the auto-dock (no raiders about); returns the events.
fn auto_dock(w: &mut World) -> Vec<world::Event> {
    let mut events = Vec::new();
    let mut t = 0;
    while w.state != State::Docked {
        let pad = Pad {
            auto: !w.me.auto,
            ..Pad::default()
        };
        w.tick(&pad);
        while let Some(e) = w.take_event() {
            events.push(e);
        }
        w.ships = [None; world::SHIPS];
        t += 1;
        assert!(t < 60 * 120, "no dock after 2 minutes");
    }
    events
}

#[test]
fn the_first_dock_at_a_system_surveys_it_and_pays_once() {
    let mut w = world_at_start(2);
    let home = w.car.system;
    assert!(w.car.charted(home) && w.car.surveys() == 1);
    assert_eq!(w.car.standing(), "DRIFTER");
    // As if the home system were new.
    w.car.charted = [0; world::CHART_BYTES];
    let fee = world::survey_fee(&w.sys);
    assert!((50..=125).contains(&fee));
    for round in 0..2 {
        let before = w.car.credits;
        w.launch();
        let events = auto_dock(&mut w);
        let surveyed = events.contains(&world::Event::Surveyed);
        assert_eq!(surveyed, round == 0, "round {round}");
        let paid = if round == 0 { fee } else { 0 };
        assert_eq!(w.car.credits, before + paid, "round {round}");
        assert!(w.car.charted(home) && w.car.surveys() == 1);
    }
    // Standing grows with the surveys.
    for (n, name) in [
        (2, "DRIFTER"),
        (3, "SKETCHER"),
        (8, "PLOTTER"),
        (19, "CHARTWRIGHT"),
        (20, "MAPMAKER"),
        (136, "MAPMAKER"),
    ] {
        w.car.charted = [0; world::CHART_BYTES];
        for i in 0..n {
            w.car.charted[i / 8] |= 1 << (i % 8);
        }
        assert_eq!(w.car.surveys(), n as u32);
        assert_eq!(w.car.standing(), name, "{n} surveys");
    }
}

#[test]
fn a_career_is_won_at_the_charter_sum_and_surveys() {
    let mut d = Driver::new();
    d.start();
    d.app.world.car.credits = WIN_CREDITS - 1;
    d.app.last_dock = d.app.world.car;
    // Credits enough but one survey short: no charter yet.
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    d.app.world.car.credits = WIN_CREDITS;
    // The home system (surveyed at the start) and 18 others: 19.
    let home = usize::from(d.app.world.car.system);
    let mut k = 0;
    while d.app.world.car.surveys() < world::CHARTER_SURVEYS - 1 {
        if k != home {
            d.app.world.car.charted[k / 8] |= 1 << (k % 8);
        }
        k += 1;
    }
    assert!(d.app.world.car.charted(home as u8));
    assert_eq!(d.app.world.car.surveys(), world::CHARTER_SURVEYS - 1);
    let mut n = 0;
    while d.app.scene == Scene::Flight {
        d.hold(if d.app.world.me.auto { 0 } else { button::Y });
        d.app.world.ships = [None; world::SHIPS];
        n += 1;
        assert!(n < 60 * 120, "never docked");
    }
    assert_eq!(d.app.scene, Scene::Docked, "one survey short: no charter");
    assert!(!d.app.world.car.won);
    // Now with the surveys too.
    d.app.world.car.charted = [0xFF; world::CHART_BYTES];
    d.app.world.car.credits = WIN_CREDITS - 1;
    d.app.last_dock = d.app.world.car;
    // The menu is on MARKET after a dock: up to LAUNCH.
    d.tap(button::UP);
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    d.app.world.car.credits = WIN_CREDITS;
    // Fly back in with the auto-dock.
    let mut n = 0;
    while d.app.scene == Scene::Flight {
        d.hold(if d.app.world.me.auto { 0 } else { button::Y });
        d.app.world.ships = [None; world::SHIPS];
        n += 1;
        assert!(n < 60 * 120, "never docked");
    }
    assert_eq!(d.app.scene, Scene::Won);
    assert!(d.app.world.car.won && d.app.last_dock.won);
    assert!(d.events.iter().any(|e| matches!(e, Event::Won { .. })));
    shot(&mut d, "won");
    d.wait_card();
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Docked);
    // Only once.
    d.tap(button::A);
    let mut n = 0;
    while d.app.scene == Scene::Flight {
        d.hold(if d.app.world.me.auto { 0 } else { button::Y });
        d.app.world.ships = [None; world::SHIPS];
        n += 1;
        assert!(n < 60 * 120);
    }
    assert_eq!(d.app.scene, Scene::Docked);
}

#[test]
fn a_lost_ship_goes_back_to_the_last_dock() {
    let mut d = Driver::new();
    d.start();
    let docked = d.app.world.car;
    d.tap(button::A);
    d.app.world.car.hull = 1;
    d.app.world.me.shield = 0;
    // Into the sun's way: simply let raiders win.
    let w = &mut d.app.world;
    for k in 0..4 {
        let p = world::add(w.me.pos, [k * 200 * M, 0, 600 * M]);
        w.add_ship(Kind::Raider, p, Mode::Hunt);
    }
    let mut n = 0;
    while d.app.scene != Scene::Lost {
        // The shield is kept down: the first hit that lands is the
        // last (how soon one lands is luck; that it does is not).
        if d.app.world.state == State::Flying {
            d.app.world.me.shield = 0;
        }
        d.hold(0);
        n += 1;
        assert!(n < 60 * 60, "not lost");
    }
    d.wait_card();
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Docked);
    assert_eq!(d.app.world.car, docked);
}

/// Flies the driver's ship to its end: raiders round it and the
/// shield kept down, until the SHIP LOST card; then A.
fn lose_the_ship(d: &mut Driver) {
    d.app.world.car.hull = 1;
    d.app.world.me.shield = 0;
    let w = &mut d.app.world;
    for k in 0..4 {
        let p = world::add(w.me.pos, [k * 200 * M, 0, 600 * M]);
        w.add_ship(Kind::Raider, p, Mode::Hunt);
    }
    let mut n = 0;
    while d.app.scene != Scene::Lost {
        if d.app.world.state == State::Flying {
            d.app.world.me.shield = 0;
        }
        d.hold(0);
        n += 1;
        assert!(n < 60 * 60, "not lost");
    }
    d.wait_card();
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Docked);
}

#[test]
fn a_lost_ship_loses_its_cargo_and_keeps_the_rest() {
    let mut d = Driver::new();
    d.start();
    // Cargo bought in the market and TAGs from earlier (as if the last
    // stop were LAWLESS), then back at the dock screen: the last dock.
    d.app.world.car.tags = 3;
    d.app.world.car.owed = 175;
    d.app.world.car.kills = 3;
    d.tap(button::DOWN);
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Market);
    let g = (0..GOODS).find(|&g| d.app.world.stock[g] > 0).unwrap();
    d.app.sel = g as u8;
    for _ in 0..3 {
        d.tap(button::A);
    }
    d.tap(button::B);
    assert_eq!(d.app.scene, Scene::Docked);
    let docked = d.app.last_dock;
    assert_eq!(docked.hold.0[g], 3);
    assert_eq!(docked.tags, 3);
    // Up to LAUNCH, out, lost.
    d.tap(button::UP);
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    lose_the_ship(&mut d);
    let c = d.app.world.car;
    assert_eq!(c, docked.wrecked());
    assert_eq!(c.hold.total(), 0, "the cargo is lost");
    assert_eq!((c.tags, c.owed), (0, 0), "the TAGs are lost");
    // The rest is as at the dock.
    assert_eq!(
        (c.credits, c.fuel, c.laser, c.shield, c.ship, c.scoop),
        (
            docked.credits,
            docked.fuel,
            docked.laser,
            docked.shield,
            docked.ship,
            docked.scoop
        )
    );
    assert_eq!(
        (c.hull, c.kills, c.day, c.system, c.charted),
        (
            docked.hull,
            docked.kills,
            docked.day,
            docked.system,
            docked.charted
        )
    );
    assert_eq!(d.app.last_dock, c, "the last dock is the wreck's career");
}

#[test]
fn a_second_loss_without_docking_finds_the_hold_still_empty() {
    let mut d = Driver::new();
    d.start();
    d.app.world.car.hold.0[2] = 4;
    d.app.world.car.tags = 1;
    d.app.world.car.owed = 25;
    d.app.last_dock = d.app.world.car;
    d.tap(button::A);
    lose_the_ship(&mut d);
    let once = d.app.world.car;
    assert_eq!((once.hold.total(), once.tags, once.owed), (0, 0, 0));
    // Straight out again (no trade, no dock) and lost again.
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    // Something scooped on the way is lost too.
    d.app.world.car.hold.0[5] = 2;
    d.app.world.car.tags = 2;
    d.app.world.car.owed = 100;
    lose_the_ship(&mut d);
    assert_eq!(d.app.world.car, once);
    assert_eq!(d.app.last_dock, once);
}

#[test]
fn a_broke_pilot_after_a_loss_gets_the_free_tank_and_can_earn() {
    let mut w = world_at_start(3);
    // All money in the hold, the tank nearly dry.
    w.car.credits = 20;
    w.car.fuel = 2;
    w.car.hold.0[1] = 10;
    let last_dock = w.car;
    w.launch();
    w.car.hull = 0;
    w.tick(&Pad::default());
    assert_eq!(w.state, State::Dead);
    w.begin(last_dock.wrecked());
    assert_eq!(w.car.credits, 20);
    assert_eq!(w.car.hold.total(), 0);
    assert_eq!(
        w.car.fuel, TANK,
        "a broke pilot with an empty hold gets a full tank"
    );
    // A new neighbour in reach, the safest first: its survey fee is
    // money from nothing but flying.
    let here = galaxy::system(w.car.seed, w.car.system);
    let to = (0..galaxy::SYSTEMS as u8)
        .filter(|&i| !w.car.charted(i))
        .map(|i| galaxy::system(w.car.seed, i))
        .filter(|s| here.dist(s) <= u32::from(TANK))
        .min_by_key(|s| (s.danger, here.dist(s)))
        .unwrap();
    let fee = world::survey_fee(&to);
    assert!(matches!(
        trip(&mut w, to.id, 60 * 60 * 10),
        Trip::Docked { .. }
    ));
    assert!(w.car.charted(to.id));
    assert!(
        w.car.credits >= 20 + fee,
        "the survey fee is paid ({} credits)",
        w.car.credits
    );
}

// ---- Raiders' TAGs ----------------------------------------------------------------

/// The TAGs in space: (slot, ship).
fn tags_in_space(w: &World) -> Vec<(usize, world::Ship)> {
    w.ships
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.map(|s| (i, s)))
        .filter(|(_, s)| s.kind == Kind::Canister && s.load.0 == world::TAG)
        .collect()
}

/// Launched, 12 km out of the mouth (away from the station's guns and
/// the tow lane), still, no other ship about.
fn out_in_space(galaxy: u8, danger: u8) -> Box<World> {
    let mut w = world_in(galaxy, danger);
    w.launch();
    let out = w.bodies.station_rot.col(2);
    w.me.pos = world::add(w.bodies.station_pos, mercator::fixed::scale14(out, 12 * KM));
    w.me.speed = 0;
    w.me.throttle = 0;
    w.ships = [None; world::SHIPS];
    w
}

/// Holds ship `i` 300 m in front of the nose with one hull point left
/// and fires the nose laser until it is gone; returns the events.
fn shoot_down(w: &mut World, i: usize) -> Vec<world::Event> {
    let kind = w.ships[i].unwrap().kind;
    w.ships[i].as_mut().unwrap().hp = 1;
    let ahead = mercator::fixed::scale14(w.me.rot.col(2), 300 * M);
    let mut events = Vec::new();
    for _ in 0..60 {
        match w.ships[i].as_mut() {
            Some(s) if s.kind == kind => {
                s.pos = world::add(w.me.pos, ahead);
                s.cool = 255;
            }
            _ => break,
        }
        w.tick(&Pad {
            fire: true,
            ..Pad::default()
        });
        while let Some(e) = w.take_event() {
            events.push(e);
        }
    }
    assert!(
        w.ships[i].is_none_or(|s| s.kind != kind),
        "the {kind:?} was not shot down"
    );
    events
}

/// A TAG worth `value` (1..=4 times 25) placed at `pos`, with a TAG's life
/// (as a raider's kill makes it).
fn put_tag(w: &mut World, pos: mercator::fixed::V3, value: u8) -> usize {
    let i = w.add_ship(Kind::Canister, pos, Mode::Travel).unwrap();
    let s = w.ships[i].as_mut().unwrap();
    s.load = (world::TAG, value);
    s.timer = world::TAG_LIFE;
    i
}

#[test]
fn a_raider_you_kill_drops_a_tag_and_pays_nothing_yet() {
    for danger in 0..=3u8 {
        let mut w = out_in_space(1, danger);
        let here = w.me.pos;
        let i = w.add_ship(Kind::Raider, here, Mode::Hunt).unwrap();
        let gun = w.ships[i].unwrap().gun;
        let (credits, kills) = (w.car.credits, w.car.kills);
        let events = shoot_down(&mut w, i);
        assert!(events.contains(&world::Event::Killed { kind: Kind::Raider }));
        assert_eq!(w.car.kills, kills + 1);
        assert_eq!(w.car.credits, credits, "danger {danger}: paid at the kill");
        assert_eq!((w.car.tags, w.car.owed), (0, 0));
        let tags = tags_in_space(&w);
        assert_eq!(tags.len(), 1, "danger {danger}: one TAG");
        let (_, t) = tags[0];
        // Its worth is fixed by the raider's gun and the kill system's
        // danger (step M1): (gun level + danger / 2) x 25, at most 100.
        assert_eq!(t.load.1, world::tag_worth(gun, danger), "danger {danger}");
        assert!(t.timer > world::TAG_LIFE - 60, "a fresh TAG lasts 120 s");
    }
}

#[test]
fn the_gunners_kill_counts_as_yours() {
    let mut w = out_in_space(2, 2);
    let behind = mercator::fixed::scale14(w.me.rot.col(2), -800 * M);
    let i = w
        .add_ship(Kind::Raider, world::add(w.me.pos, behind), Mode::Hunt)
        .unwrap();
    w.ships[i].as_mut().unwrap().hp = 1;
    let mut killed = false;
    for _ in 0..60 * 10 {
        if let Some(s) = w.ships[i].as_mut()
            && s.kind == Kind::Raider
        {
            s.pos = world::add(w.me.pos, behind);
            s.cool = 255;
        } else {
            break;
        }
        w.tick(&Pad {
            turret: true,
            ..Pad::default()
        });
        while let Some(e) = w.take_event() {
            killed |= e == world::Event::Killed { kind: Kind::Raider };
        }
    }
    assert!(killed, "the turret's kill is the player's");
    assert_eq!(tags_in_space(&w).len(), 1, "and drops a TAG");
}

#[test]
fn a_raider_a_warden_kills_drops_no_tag() {
    let mut w = out_in_space(4, 1);
    let up = mercator::fixed::scale14(w.me.rot.col(1), 5 * KM);
    let at = world::add(w.me.pos, up);
    let r = w.add_ship(Kind::Raider, at, Mode::Hunt).unwrap();
    // Step M2: 300 m times FIGHTER_X (was 500 m): beyond the warden's
    // break-off (200 m times FIGHTER_X), within its 1.1 km fire range.
    let side = mercator::fixed::scale14(w.me.rot.col(0), 300 * M * world::FIGHTER_X);
    let g = w
        .add_ship(Kind::Warden, world::add(at, side), Mode::Hunt)
        .unwrap();
    // The player's own shot hit it before: still not the player's kill.
    w.ships[r].as_mut().unwrap().angry = true;
    w.ships[r].as_mut().unwrap().hp = 1;
    let kills = w.car.kills;
    let mut t = 0;
    while w.ships[r].is_some_and(|s| s.kind == Kind::Raider) {
        // Both held still; the warden faces the raider.
        let s = w.ships[r].as_mut().unwrap();
        s.pos = at;
        s.cool = 255;
        let wd = w.ships[g].as_mut().unwrap();
        wd.pos = world::add(at, side);
        wd.rot = mercator::fixed::Mat3::looking(
            [-side[0], -side[1], -side[2]],
            [0, mercator::fixed::ONE14, 0],
        );
        wd.mode = Mode::Hunt;
        w.tick(&Pad::default());
        while let Some(e) = w.take_event() {
            assert!(!matches!(e, world::Event::Killed { .. }));
        }
        t += 1;
        assert!(t < 60 * 60, "the warden never downed the raider");
    }
    // It blew up (it did not just leave).
    assert!(
        w.blasts
            .iter()
            .any(|b| world::within(b.pos, at, 100 * M) && b.age < world::BLAST_TICKS),
        "the raider left without a blast"
    );
    assert_eq!(w.car.kills, kills);
    assert!(tags_in_space(&w).is_empty(), "a warden's kill drops no TAG");
}

#[test]
fn a_hauler_or_warden_kill_drops_no_tag() {
    for kind in [Kind::Hauler, Kind::Warden] {
        let mut w = out_in_space(5, 1);
        let here = w.me.pos;
        let i = w.add_ship(kind, here, Mode::Travel).unwrap();
        let events = shoot_down(&mut w, i);
        assert!(events.contains(&world::Event::Killed { kind }));
        assert!(tags_in_space(&w).is_empty(), "{kind:?}: no TAG");
        assert!(w.wanted);
    }
}

#[test]
fn a_tag_takes_the_dead_ships_slot_even_when_all_slots_are_full() {
    let mut w = out_in_space(6, 3);
    // Every fight slot taken (the traffic slots after them stay out of
    // fights): canisters far off, the raider in the last.
    let far = mercator::fixed::scale14(w.me.rot.col(1), 8 * KM);
    for _ in 0..world::FIGHT - 1 {
        w.add_ship(Kind::Canister, world::add(w.me.pos, far), Mode::Travel)
            .unwrap();
    }
    let here = w.me.pos;
    let i = w.add_ship(Kind::Raider, here, Mode::Hunt).unwrap();
    assert_eq!(i, world::FIGHT - 1);
    assert!(w.ships[..world::FIGHT].iter().all(Option::is_some));
    shoot_down(&mut w, i);
    let tags = tags_in_space(&w);
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].0, i, "the TAG takes the raider's slot");
}

#[test]
fn a_tag_is_scooped_with_a_full_hold_and_takes_no_space() {
    let mut w = out_in_space(7, 0);
    w.car.hold.0[3] = w.car.capacity() as u8;
    assert_eq!(w.car.room(), 0);
    let hold = w.car.hold;
    let here = w.me.pos;
    put_tag(&mut w, here, 3);
    // A cargo canister beside it stays out: the hold is full.
    let c = w.add_ship(Kind::Canister, here, Mode::Travel).unwrap();
    w.ships[c].as_mut().unwrap().load = (1, 2);
    w.tick(&Pad::default());
    let mut events = Vec::new();
    while let Some(e) = w.take_event() {
        events.push(e);
    }
    assert!(events.contains(&world::Event::Scooped {
        good: world::TAG,
        n: 1
    }));
    assert!(events.contains(&world::Event::HoldFull));
    assert_eq!((w.car.tags, w.car.owed), (1, 75));
    assert_eq!(w.car.hold, hold, "a TAG takes no hold space");
    assert!(tags_in_space(&w).is_empty());
    assert!(w.ships[c].is_some(), "the cargo stays in space");
}

#[test]
fn your_laser_cannot_destroy_a_tag() {
    let mut w = out_in_space(8, 2);
    let ahead = world::add(w.me.pos, mercator::fixed::scale14(w.me.rot.col(2), 300 * M));
    let i = put_tag(&mut w, ahead, 3);
    let hp = w.ships[i].unwrap().hp;
    // The laser passes through it: the nose line hits nothing.
    assert_eq!(w.first_hit(), None);
    for _ in 0..60 * 5 {
        w.ships[i].as_mut().unwrap().pos = ahead;
        w.me.heat = 0;
        w.tick(&Pad {
            fire: true,
            ..Pad::default()
        });
        while w.take_event().is_some() {}
        let s = w.ships[i].expect("the TAG is gone");
        assert_eq!(s.hp, hp, "a TAG takes no damage");
        assert!(!s.angry);
    }
    assert!(!w.foe_in_sights(), "a TAG is never a foe in the sights");
}

/// A TAG between the nose and a raider does not shield the raider: the
/// laser passes through the TAG, and touch steering's auto-fire still
/// sees the raider.
#[test]
fn a_tag_in_front_does_not_shield_a_raider_behind_it() {
    let mut w = out_in_space(8, 2);
    let fwd = w.me.rot.col(2);
    let tag_at = world::add(w.me.pos, mercator::fixed::scale14(fwd, 200 * M));
    let raider_at = world::add(w.me.pos, mercator::fixed::scale14(fwd, 500 * M));
    let t = put_tag(&mut w, tag_at, 3);
    let r = w.add_ship(Kind::Raider, raider_at, Mode::Hunt).unwrap();
    let hp = w.ships[r].unwrap().hp;
    assert_eq!(w.first_hit().map(|h| h.0), Some(r));
    assert!(w.foe_in_sights());
    for _ in 0..120 {
        w.ships[t].as_mut().unwrap().pos = tag_at;
        let s = w.ships[r].as_mut().unwrap();
        s.pos = raider_at;
        s.cool = 255;
        // Kept alive, so the test sees each shot land.
        s.hp = s.hp.max(hp / 2);
        w.me.heat = 0;
        w.tick(&Pad {
            fire: true,
            ..Pad::default()
        });
        while w.take_event().is_some() {}
    }
    assert!(
        w.ships[r].unwrap().hp < hp,
        "the raider behind the TAG was never hit"
    );
    assert_eq!(w.ships[t].unwrap().load.0, world::TAG);
}

#[test]
fn an_unscooped_tag_is_gone_after_120_seconds_and_blinks() {
    let mut w = out_in_space(1, 1);
    let away = world::add(w.me.pos, mercator::fixed::scale14(w.me.rot.col(2), 5 * KM));
    let i = put_tag(&mut w, away, 2);
    let (mut on, mut off) = (0, 0);
    for t in 1..=60 * 120 {
        w.tick(&Pad::default());
        while w.take_event().is_some() {}
        if t < 60 * 120 {
            let s = w.ships[i].expect("gone too soon");
            if s.flash > 0 {
                on += 1;
            } else {
                off += 1;
            }
        }
    }
    assert!(w.ships[i].is_none(), "still there after 120 s");
    assert_eq!(w.car.tags, 0);
    assert!(
        on > 60 * 40 && off > 60 * 40,
        "it blinks: {on} on, {off} off"
    );
}

/// Shoots down a `kind` and counts the ticks its first canister that
/// `pick` matches floats (the kill tick is tick 1). The pilot moves
/// 5 km to the side so it never scoops it; other ships are removed every
/// tick so nothing hunts the pilot.
fn life_after_kill(kind: Kind, pick: fn(&world::Ship) -> bool) -> u32 {
    let mut w = out_in_space(1, 1);
    let here = w.me.pos;
    let i = w.add_ship(kind, here, Mode::Hunt).unwrap();
    shoot_down(&mut w, i);
    let k = (0..world::SHIPS)
        .find(|&k| w.ships[k].is_some_and(|s| s.kind == Kind::Canister && pick(&s)))
        .expect("no canister after the kill");
    let side = mercator::fixed::scale14(w.me.rot.col(0), 5 * KM);
    w.me.pos = world::add(w.me.pos, side);
    let mut t = 1;
    while w.ships[k].is_some() {
        assert!(t < 60 * 200, "a canister that never goes");
        for s in w.ships.iter_mut() {
            if s.is_some_and(|s| s.kind != Kind::Canister) {
                *s = None;
            }
        }
        w.tick(&Pad::default());
        while w.take_event().is_some() {}
        t += 1;
        if let Some(s) = w.ships[k] {
            assert!(pick(&s) && s.kind == Kind::Canister, "slot {k} reused");
        }
    }
    assert_eq!((w.car.tags, w.car.owed), (0, 0), "scooped");
    // Still there after t - 1 ticks, gone after t.
    t
}

#[test]
fn a_tag_lives_120_seconds_and_cargo_still_90() {
    assert_eq!(world::TAG_LIFE, 7_200);
    assert_eq!(world::CARGO_LIFE, 5_400);
    // A raider's TAG: there after 7,199 ticks, gone at 7,200 (120 s).
    let tag = life_after_kill(Kind::Raider, |s| s.load.0 == world::TAG);
    assert_eq!(tag, u32::from(world::TAG_LIFE), "TAG life in ticks");
    // A hauler always drops cargo: there after 5,399 ticks, gone at
    // 5,400 (90 s, as before).
    let cargo = life_after_kill(Kind::Hauler, |s| s.load.0 != world::TAG);
    assert_eq!(cargo, u32::from(world::CARGO_LIFE), "cargo life in ticks");
}

#[test]
fn tags_stop_at_255_and_the_next_one_stays_in_space() {
    let mut w = out_in_space(2, 0);
    w.car.tags = world::TAGS_MAX - 1;
    w.car.owed = u16::from(world::TAGS_MAX - 1) * 25;
    let here = w.me.pos;
    put_tag(&mut w, here, 4);
    w.tick(&Pad::default());
    while w.take_event().is_some() {}
    assert_eq!(w.car.tags, world::TAGS_MAX);
    assert_eq!(w.car.owed, 254 * 25 + 100);
    let here = w.me.pos;
    let i = put_tag(&mut w, here, 4);
    for _ in 0..30 {
        w.ships[i].as_mut().unwrap().pos = w.me.pos;
        w.tick(&Pad::default());
    }
    while let Some(e) = w.take_event() {
        assert!(!matches!(e, world::Event::Scooped { .. }));
    }
    assert_eq!(w.car.tags, world::TAGS_MAX);
    assert_eq!(w.car.owed, 254 * 25 + 100);
    assert!(w.ships[i].is_some(), "the 256th TAG stays in space");
    // And a full tally is still paid in full at a lawful dock.
    w.launch();
    let events = auto_dock(&mut w);
    assert!(events.contains(&world::Event::Bounty {
        sum: 254 * 25 + 100
    }));
    assert_eq!(w.car.tags, 0);
}

/// With `tags` TAGs worth `owed`, launched and auto-docked again;
/// returns the events. Every system counts as surveyed already, so no
/// survey fee mixes with the bounty.
fn dock_with_tags(w: &mut World, tags: u8, owed: u16) -> Vec<world::Event> {
    w.car.charted = [0xFF; world::CHART_BYTES];
    w.car.tags = tags;
    w.car.owed = owed;
    w.launch();
    auto_dock(w)
}

#[test]
fn tags_are_paid_on_docking_with_one_event_and_the_sum() {
    for danger in 0..=2u8 {
        let mut w = world_in(3, danger);
        let before = w.car.credits;
        let events = dock_with_tags(&mut w, 3, 225);
        let paid: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, world::Event::Bounty { .. }))
            .collect();
        assert_eq!(
            paid,
            [&world::Event::Bounty { sum: 225 }],
            "danger {danger}"
        );
        assert_eq!(w.car.credits, before + 225);
        assert_eq!((w.car.tags, w.car.owed), (0, 0));
        // Nothing more at the next dock.
        let events = dock_with_tags(&mut w, 0, 0);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, world::Event::Bounty { .. } | world::Event::Unpaid { .. }))
        );
        assert_eq!(w.car.credits, before + 225);
    }
}

#[test]
fn a_lawless_station_keeps_your_tags_unpaid_and_the_next_lawful_one_pays() {
    let mut w = world_in(4, 3);
    let before = w.car.credits;
    let events = dock_with_tags(&mut w, 4, 300);
    if world::LAWLESS_PAYS {
        // The switch turned on: LAWLESS pays like any other station.
        assert!(events.contains(&world::Event::Bounty { sum: 300 }));
        return;
    }
    assert!(events.contains(&world::Event::Unpaid { lawless: true }));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, world::Event::Bounty { .. }))
    );
    assert_eq!(w.car.credits, before);
    assert_eq!((w.car.tags, w.car.owed), (4, 300), "the TAGs are kept");
    // On to a lawful system: paid there.
    let mut c = w.car;
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| galaxy::system(c.seed, i).danger == 1)
        .unwrap();
    w.begin(c);
    w.launch();
    let events = auto_dock(&mut w);
    assert!(events.contains(&world::Event::Bounty { sum: 300 }));
    assert_eq!(w.car.tags, 0);
}

#[test]
fn an_outlaw_is_not_paid_but_keeps_the_tags() {
    let mut w = world_in(5, 1);
    w.car.charted = [0xFF; world::CHART_BYTES];
    let before = w.car.credits;
    w.car.tags = 2;
    w.car.owed = 100;
    w.launch();
    w.wanted = true;
    let events = auto_dock(&mut w);
    assert!(events.contains(&world::Event::Unpaid { lawless: false }));
    assert_eq!(w.car.credits, before);
    assert_eq!((w.car.tags, w.car.owed), (2, 100));
    // A new system (the warrant stays behind): paid.
    let mut c = w.car;
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| i != c.system && galaxy::system(c.seed, i).danger == 0)
        .unwrap();
    w.begin(c);
    assert!(!w.wanted);
    w.launch();
    assert!(auto_dock(&mut w).contains(&world::Event::Bounty { sum: 100 }));
}

#[test]
fn tags_are_lost_with_the_ship() {
    let mut w = out_in_space(6, 2);
    // TAGs already carried at the last dock (left unpaid there, as at a
    // LAWLESS station), and one more scooped on the way.
    w.car.tags = 4;
    w.car.owed = 325;
    let last_dock = w.car;
    let here = w.me.pos;
    put_tag(&mut w, here, 3);
    w.tick(&Pad::default());
    assert_eq!((w.car.tags, w.car.owed), (5, 400));
    w.car.hull = 0;
    w.tick(&Pad::default());
    assert_eq!(w.state, State::Dead);
    let back = last_dock.wrecked();
    assert_eq!((back.tags, back.owed), (0, 0));
    assert_eq!(
        Career {
            tags: 4,
            owed: 325,
            hold: last_dock.hold,
            ..back
        },
        last_dock,
        "only the cargo and the TAGs are lost"
    );
    w.begin(back);
    assert_eq!((w.car.tags, w.car.owed), (0, 0));
}

#[test]
fn the_app_notes_the_kill_the_scoop_and_the_bounty_with_its_sum() {
    let mut d = Driver::new();
    d.start();
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    let w = &mut d.app.world;
    let out = w.bodies.station_rot.col(2);
    w.me.pos = world::add(w.bodies.station_pos, mercator::fixed::scale14(out, 12 * KM));
    w.ships = [None; world::SHIPS];
    let here = w.me.pos;
    let i = w.add_ship(Kind::Raider, here, Mode::Hunt).unwrap();
    w.ships[i].as_mut().unwrap().hp = 1;
    let gun = w.ships[i].unwrap().gun;
    let ahead = mercator::fixed::scale14(w.me.rot.col(2), 300 * M);
    let mut n = 0;
    while d.app.world.ships[i].is_some_and(|s| s.kind == Kind::Raider) {
        let me = d.app.world.me.pos;
        let s = d.app.world.ships[i].as_mut().unwrap();
        s.pos = world::add(me, ahead);
        s.cool = 255;
        d.hold(button::A);
        n += 1;
        assert!(n < 120, "never fired");
    }
    assert_eq!(d.app.note.unwrap().0, "RAIDER DOWN: SCOOP ITS TAG");
    assert!(d.events.contains(&Event::Killed { kind: Kind::Raider }));
    // Scoop it.
    let me = d.app.world.me.pos;
    d.app.world.ships[i].as_mut().unwrap().pos = me;
    d.hold(0);
    assert_eq!(d.app.note.unwrap().0, "SCOOPED: RAIDER TAG");
    assert_eq!(d.app.world.car.tags, 1);
    // Step M1: the worth follows the raider's gun and the danger.
    let worth = 25 * u32::from(world::tag_worth(gun, d.app.world.sys.danger));
    // Home with the auto-dock.
    let credits = d.app.world.car.credits;
    let mut n = 0;
    while d.app.scene == Scene::Flight {
        d.hold(if d.app.world.me.auto { 0 } else { button::Y });
        d.app.world.ships = [None; world::SHIPS];
        n += 1;
        assert!(n < 60 * 120, "never docked");
    }
    assert_eq!(d.app.scene, Scene::Docked);
    assert_eq!(d.app.note.unwrap().0, "BOUNTY PAID: ");
    assert_eq!(d.app.note_cr, worth);
    assert_eq!(d.app.world.car.credits, credits + worth);
    assert!(d.events.contains(&Event::Bounty { sum: worth }));
    assert_eq!(
        d.app.last_dock, d.app.world.car,
        "the paid career is the last dock"
    );
}

#[test]
fn a_bounty_can_win_the_charter_at_that_dock() {
    let mut d = Driver::new();
    d.start();
    d.app.world.car.charted = [0xFF; world::CHART_BYTES];
    d.app.world.car.credits = WIN_CREDITS - 100;
    d.app.world.car.tags = 2;
    d.app.world.car.owed = 100;
    d.app.last_dock = d.app.world.car;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    let mut n = 0;
    while d.app.scene == Scene::Flight {
        d.hold(if d.app.world.me.auto { 0 } else { button::Y });
        d.app.world.ships = [None; world::SHIPS];
        n += 1;
        assert!(n < 60 * 120, "never docked");
    }
    assert_eq!(
        d.app.scene,
        Scene::Won,
        "the bounty is paid before the charter check"
    );
    assert!(d.app.world.car.won);
    assert_eq!(d.app.world.car.credits, WIN_CREDITS);
    assert!(d.events.contains(&Event::Bounty { sum: 100 }));
}

// ---- Save -------------------------------------------------------------------------

fn some_career() -> Career {
    let mut c = Career::new(seed_of(3));
    c.credits = 123_456;
    c.fuel = 33;
    c.hold.0 = [1, 0, 2, 0, 3, 4, 0, 5];
    c.laser = 2;
    c.shield = 1;
    // Step M5: the DRAY (the bay before; hull 77 then, of 100; 57 of
    // the DRAY's 70 now).
    c.ship = world::DRAY;
    c.scoop = true;
    c.hull = 57;
    c.kills = 512;
    c.day = 40_000;
    c.target = 101;
    c.won = true;
    for (i, b) in c.charted.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(37) | 1;
    }
    c.tags = 7;
    c.owed = 400;
    c
}

#[test]
fn a_save_round_trips() {
    for (c, set) in [
        (Career::new(seed_of(1)), 0u8),
        (some_career(), 0b1_1110),
        (Career::new(0xFFFF_FFFF), 2),
        (
            Career {
                tags: 255,
                owed: 25_500,
                ..some_career()
            },
            0,
        ),
        (
            Career {
                tags: 255,
                owed: 255 * 25,
                ..some_career()
            },
            0,
        ),
    ] {
        let b = save::to_bytes(&c, set);
        assert_eq!(b.len(), SIZE);
        assert_eq!(save::from_bytes(&b), Ok((c, set)));
    }
    println!("a career is {SIZE} bytes (of 64 allowed)");
    const { assert!(SIZE <= 64) };
}

#[test]
fn a_damaged_save_is_refused() {
    let good = save::to_bytes(&some_career(), 0b1010);
    // Every single bit flipped: refused.
    for i in 0..SIZE * 8 {
        let mut b = good;
        b[i / 8] ^= 1 << (i % 8);
        assert!(save::from_bytes(&b).is_err(), "bit {i} flipped and loaded");
    }
    // Wrong version: the next one, and the one before the TAGs (3),
    // even with a right CRC.
    // Step M5: 4 too, at this size (version 4 had 51 bytes).
    for v in [save::VERSION + 1, 4, 3] {
        let mut b = good;
        b[0] = v;
        assert_eq!(save::from_bytes(&b), Err(Bad::Version), "version {v}");
        let crc = save::crc32(&b[..SIZE - 4]);
        b[SIZE - 4..].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(save::from_bytes(&b), Err(Bad::Version), "version {v}");
    }
    // Numbers out of range with a right CRC: refused too.
    let fix = |mut b: [u8; SIZE]| {
        let crc = save::crc32(&b[..SIZE - 4]);
        b[SIZE - 4..].copy_from_slice(&crc.to_le_bytes());
        b
    };
    let mut bad = Vec::new();
    let mut b = good;
    b[10] = TANK + 1;
    bad.push(fix(b));
    // Systems past the galaxy's last.
    let mut b = good;
    b[5] = galaxy::SYSTEMS as u8;
    bad.push(fix(b));
    let mut b = good;
    b[25] = 255;
    bad.push(fix(b));
    let mut b = good;
    b[20] = 0;
    bad.push(fix(b));
    let mut b = good;
    b[20] = 101;
    bad.push(fix(b));
    let mut b = good;
    b[6..10].copy_from_slice(&(world::CREDITS_MAX + 1).to_le_bytes());
    bad.push(fix(b));
    // One tonne past the hold (step M5, renewed on purpose: 47 + the
    // other 14 is 61, past the DRAY's 60; it was 36 + 14 past the bay's
    // 40).
    let mut b = good;
    b[11] = 47;
    bad.push(fix(b));
    let mut b = good;
    b[19] |= 0x80;
    bad.push(fix(b));
    let mut b = good;
    b[19] = (b[19] & !3) | 3;
    bad.push(fix(b));
    let mut b = good;
    b[26] = 3;
    bad.push(fix(b));
    // Step M5: some_career flies the DRAY (hull at most 70, hold 60):
    // one hull point past it, a ship past the SHRIKE, the old bay bit,
    // and 61 tonnes.
    let mut b = good;
    b[20] = 71;
    bad.push(fix(b));
    let mut b = good;
    b[47] = 3;
    bad.push(fix(b));
    let mut b = good;
    b[19] |= 16;
    bad.push(fix(b));
    let mut b = good;
    b[11..19].copy_from_slice(&[61, 0, 0, 0, 0, 0, 0, 0]);
    bad.push(fix(b));
    // TAGs worth more than 100 or less than 25 each, worth without
    // TAGs, or a sum that is no TAG values' sum (not a multiple of 25).
    for (tags, owed) in [
        (7u8, 701u16),
        (7, 174),
        (0, 1),
        (1, 0),
        (255, 25_501),
        (7, 401),
        (1, 26),
        // Only the upper bound refuses these (multiples of 25).
        (7, 725),
        (1, 125),
        (255, 25_525),
    ] {
        let mut b = good;
        b[44] = tags;
        b[45..47].copy_from_slice(&owed.to_le_bytes());
        bad.push(fix(b));
    }
    for (k, b) in bad.iter().enumerate() {
        assert_eq!(save::from_bytes(b), Err(Bad::Range), "case {k}");
    }
    // Random bytes: never loaded (and never a panic).
    let mut x = 1u32;
    for _ in 0..100_000 {
        let mut b = [0u8; SIZE];
        for v in b.iter_mut() {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *v = (x >> 24) as u8;
        }
        b[0] = 1;
        assert!(save::from_bytes(&b).is_err());
    }
}

/// A version 4 save (51 bytes, step M4's format): this version's bytes
/// without the ship byte, with bit 4 of the gear byte the CARGO BAY 40T.
fn v4_bytes(c: &Career, bay: bool, set: u8) -> Vec<u8> {
    let b5 = save::to_bytes(c, set);
    let mut b = b5[..47].to_vec();
    b[0] = 4;
    if bay {
        b[19] |= 16;
    }
    let crc = save::crc32(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    assert_eq!(b.len(), save::SIZE_V4);
    b
}

#[test]
fn a_version_4_save_loads_as_the_lark() {
    let mut c = some_career();
    c.ship = world::LARK;
    c.hull = 100;
    c.credits = 5_000;
    // 40 tonnes: the most the big bay held.
    c.hold.0 = [10, 0, 6, 0, 12, 0, 8, 4];
    let (got, set) = save::from_bytes(&v4_bytes(&c, true, 0b1010)).unwrap();
    assert_eq!(set, 0b1010);
    assert_eq!(got.ship, world::LARK);
    assert_eq!(got.hold.total(), 24);
    // The 16 tonnes past 24 go in the market's order: 10 of good 0 and
    // 6 of good 2, at the save's station's sell prices; the bay is paid
    // back at 900.
    let sys = galaxy::system(c.seed, c.system);
    let sold = 10 * market::sell_price(&sys, 0, c.day) + 6 * market::sell_price(&sys, 2, c.day);
    assert_eq!(got.hold.0, [0, 0, 0, 0, 12, 0, 8, 4]);
    assert_eq!(got.credits, 5_000 + save::BAY_PRICE + sold);
    // Everything else as saved.
    assert_eq!(
        Career {
            credits: c.credits,
            hold: c.hold,
            ..got
        },
        c
    );
    // Credits stay capped.
    c.credits = world::CREDITS_MAX - 100;
    let (got, _) = save::from_bytes(&v4_bytes(&c, true, 0)).unwrap();
    assert_eq!(got.credits, world::CREDITS_MAX);
    // Without the bay: loaded as it was.
    c.credits = 5_000;
    c.hold.0 = [10, 0, 6, 0, 8, 0, 0, 0];
    assert_eq!(save::from_bytes(&v4_bytes(&c, false, 0)), Ok((c, 0)));
    // Refused: 25 tonnes without the bay, 41 with it, a hull past 100,
    // a damaged byte, a cut save.
    c.hold.0 = [10, 0, 6, 0, 9, 0, 0, 0];
    assert_eq!(save::from_bytes(&v4_bytes(&c, false, 0)), Err(Bad::Range));
    c.hold.0 = [10, 0, 6, 0, 25, 0, 0, 0];
    assert_eq!(save::from_bytes(&v4_bytes(&c, true, 0)), Err(Bad::Range));
    c.hold.0 = [10, 0, 6, 0, 0, 0, 0, 0];
    c.hull = 101;
    assert_eq!(save::from_bytes(&v4_bytes(&c, true, 0)), Err(Bad::Range));
    c.hull = 100;
    let mut b = v4_bytes(&c, true, 0);
    b[6] ^= 1;
    assert_eq!(save::from_bytes(&b), Err(Bad::Crc));
    assert_eq!(
        save::from_bytes(&v4_bytes(&c, true, 0)[..50]),
        Err(Bad::Version)
    );
    // A version 5 save of version 4's size, or the other way round.
    let mut b = save::to_bytes(&c, 0).to_vec();
    b.pop();
    assert_eq!(save::from_bytes(&b), Err(Bad::Version));
    let mut b = v4_bytes(&c, false, 0);
    b[0] = save::VERSION;
    assert_eq!(save::from_bytes(&b), Err(Bad::Version));
}

#[test]
fn the_last_dock_is_what_a_save_would_hold() {
    let mut d = Driver::new();
    d.start();
    let b = save::to_bytes(&d.app.last_dock, d.app.opts.byte());
    let (c, set) = save::from_bytes(&b).unwrap();
    assert_eq!(c, d.app.world.car);
    assert_eq!(set, d.app.opts.byte());
    // Goods bought count only once back at the dock screen.
    d.tap(button::DOWN);
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Market);
    let g = (0..GOODS).find(|&g| d.app.world.stock[g] > 0).unwrap();
    d.app.sel = g as u8;
    d.tap(button::A);
    d.tap(button::B);
    assert_eq!(d.app.scene, Scene::Docked);
    assert_eq!(d.app.last_dock, d.app.world.car);
    assert_eq!(d.app.last_dock.hold.0[g], 1);
    let _ = (PLAYER, Mode::Hunt);
}

#[test]
fn the_gunner_turret_shoots_a_raider_behind_and_shares_the_heat() {
    let mut w = world_in(1, 3);
    w.launch();
    let out = w.bodies.station_rot.col(2);
    w.me.pos = world::add(w.bodies.station_pos, mercator::fixed::scale14(out, 12 * KM));
    w.me.speed = 0;
    w.me.throttle = 0;
    w.ships = [None; world::SHIPS];
    // A raider 800 m behind, where the nose laser cannot reach.
    let behind = mercator::fixed::scale14(w.me.rot.col(2), -800 * M);
    let i = w
        .add_ship(Kind::Raider, world::add(w.me.pos, behind), Mode::Hunt)
        .unwrap();
    let hp = w.ships[i].unwrap().hp;
    // No gunner: the turret is still.
    for _ in 0..60 {
        w.tick(&Pad::default());
        w.ships[i].as_mut().unwrap().pos = world::add(w.me.pos, behind);
        w.ships[i].as_mut().unwrap().cool = 255;
    }
    assert_eq!(w.ships[i].unwrap().hp, hp);
    let turret = Pad {
        turret: true,
        ..Pad::default()
    };
    let mut heat = 0;
    let mut t = 0;
    while w.ships[i].is_some_and(|s| s.kind == Kind::Raider) {
        w.tick(&turret);
        heat = heat.max(w.me.heat);
        if let Some(s) = w.ships[i].as_mut() {
            s.pos = world::add(w.me.pos, behind);
            s.cool = 255;
        }
        t += 1;
        assert!(t < 60 * 30, "the turret never downed the raider");
    }
    println!(
        "turret: raider down in {:.1} s, heat up to {heat}",
        f64::from(t) / 60.0
    );
    assert!(heat > 0, "the turret shares the laser's heat");
    assert!(w.car.kills >= 1);
    // Out of reach (2 km): the turret holds fire.
    let far = mercator::fixed::scale14(w.me.rot.col(2), -2 * KM);
    let j = w
        .add_ship(Kind::Raider, world::add(w.me.pos, far), Mode::Hunt)
        .unwrap();
    let hp = w.ships[j].unwrap().hp;
    for _ in 0..120 {
        w.tick(&turret);
        if let Some(s) = w.ships[j].as_mut() {
            s.pos = world::add(w.me.pos, far);
            s.cool = 255;
        }
    }
    assert_eq!(w.ships[j].unwrap().hp, hp);
}

#[test]
fn the_tow_lane_takes_a_slow_ship_in_once() {
    for galaxy in 1..=8 {
        let mut w = world_at_start(galaxy);
        w.launch();
        w.ships = [None; world::SHIPS];
        // 1 km in front of the mouth, a little off the axis, facing in.
        let r = w.bodies.station_rot;
        let axis = r.col(2);
        w.me.pos = world::add(
            world::add(w.bodies.station_pos, mercator::fixed::scale14(axis, KM)),
            mercator::fixed::scale14(r.col(0), 60 * M),
        );
        w.me.rot = mercator::fixed::Mat3::from_cols(
            [-r.col(0)[0], -r.col(0)[1], -r.col(0)[2]],
            r.col(1),
            [-axis[0], -axis[1], -axis[2]],
        );
        w.me.speed = 200;
        w.me.throttle = 200;
        w.tick(&Pad::default());
        assert!(
            w.me.auto,
            "galaxy {galaxy}: the tow lane did not take the ship"
        );
        let mut towed = false;
        while let Some(e) = w.take_event() {
            towed |= e == world::Event::Towed;
        }
        assert!(towed);
        // The pilot turns it off: it does not take the ship again.
        w.tick(&Pad {
            auto: true,
            ..Pad::default()
        });
        assert!(!w.me.auto);
        for _ in 0..30 {
            w.tick(&Pad::default());
            assert!(!w.me.auto, "galaxy {galaxy}: towed again");
        }
        // A new flight: towed in, docked without a crash.
        w.launch();
        w.ships = [None; world::SHIPS];
        w.me.pos = world::add(
            w.bodies.station_pos,
            mercator::fixed::scale14(axis, 1500 * M),
        );
        w.me.rot = mercator::fixed::Mat3::from_cols(
            [-r.col(0)[0], -r.col(0)[1], -r.col(0)[2]],
            r.col(1),
            [-axis[0], -axis[1], -axis[2]],
        );
        w.me.speed = 300;
        w.me.throttle = 300;
        let mut t = 0;
        while w.state != State::Docked {
            w.tick(&Pad::default());
            while let Some(e) = w.take_event() {
                assert_ne!(
                    e,
                    world::Event::Crash,
                    "galaxy {galaxy}: crashed in the lane"
                );
            }
            w.ships = [None; world::SHIPS];
            t += 1;
            assert!(t < 60 * 60, "galaxy {galaxy}: the lane never docked");
        }
        // Behind the station or too fast: no tow.
        w.launch();
        w.ships = [None; world::SHIPS];
        w.me.pos = world::add(w.bodies.station_pos, mercator::fixed::scale14(axis, -KM));
        w.tick(&Pad::default());
        assert!(!w.me.auto);
    }
}
