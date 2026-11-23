//! Step M5: the three ships to buy (LARK, DRAY, SHRIKE). Each flies by
//! its own row of `world::CRAFTS` (top speed, turn rates, hull, shield,
//! overboost); buying one at a station trades the flown one in (3/4 of
//! its list price less the hull's mending); the SHIPYARD screen buys by
//! keys and by two taps; the three read as different ships in the chase
//! view, and none makes a picture too heavy. Pictures go to
//! `.local/mercator/shots/` (names `M5-*`).

mod common;

use common::*;
use game_kit::button;
use mercator::app::{Event, Scene, seed_of};
use mercator::fixed::{Mat3, ONE14, V3, scale14, sub};
use mercator::galaxy;
use mercator::hud;
use mercator::models;
use mercator::poly::Stats;
use mercator::scene::{Detail, Eye};
use mercator::world::{
    self, CRAFTS, CREDITS_MAX, Career, DRAY, Kind, LARK, M, Mode, Pad, REPAIR_PRICE, SHRIKE, State,
    TRADE_IN, World,
};

/// The first system of galaxy 1 whose station has at least `tech` (and
/// at most `below`, exclusive).
fn system_with(tech: u8, below: u8) -> u8 {
    let seed = seed_of(1);
    (0..galaxy::SYSTEMS as u8)
        .find(|&i| {
            let s = galaxy::system(seed, i);
            s.tech >= tech && s.tech < below
        })
        .unwrap()
}

/// The shield points of the three shield levels for a ship of shield
/// 100 % (`world::SHIELDS`, private; read from the code).
const SHIELD_POINTS: [i32; 3] = [80, 140, 220];

/// Docked at `system` of galaxy 1 with `credits`.
fn docked_at(system: u8, credits: u32) -> Box<World> {
    let mut c = Career::new(seed_of(1));
    c.system = system;
    c.target = system;
    c.credits = credits;
    let mut w = Box::new(World::empty());
    w.begin(c);
    assert_eq!(w.state, State::Docked);
    w
}

#[test]
fn the_ship_table_is_what_the_notes_say() {
    println!("ship    price tech hold  m/s  turn(p,y,r)    hull shield boost(s) boost-m/s tail");
    for c in &CRAFTS {
        println!(
            "{:<7} {:>5} {:>4} {:>4} {:>4}  {:>3},{:>3},{:>3}  {:>4} {:>5}% {:>8.1} {:>9} {:>4}",
            c.name,
            c.price,
            c.tech,
            c.hold,
            c.top * 15 / 64,
            c.turn[0],
            c.turn[1],
            c.turn[2],
            c.hull,
            u32::from(c.shield) * 25,
            f64::from(c.od_full) / 180.0,
            c.top * i32::from(c.od_top) / 4 * 15 / 64,
            c.tail
        );
    }
    let (l, d, s) = (&CRAFTS[0], &CRAFTS[1], &CRAFTS[2]);
    // The LARK is the courier as it was before step M5.
    assert_eq!(l.name, "LARK");
    assert_eq!(
        (l.hold, l.top, l.hull, l.od_full, l.tech),
        (24, world::MAX_SPEED, 100, world::OD_FULL, 1)
    );
    assert_eq!(l.hull as i32, world::HULL_MAX);
    // The cargo ship: the biggest hold, the slowest, the most fragile.
    assert!(d.hold > l.hold && d.top < l.top && d.hull < l.hull);
    assert!(d.turn.iter().zip(l.turn).all(|(a, b)| *a < b));
    // The combat ship: the smallest hold, the fastest, the toughest.
    assert!(s.hold < l.hold && s.top > l.top && s.hull > l.hull && s.shield > l.shield);
    assert!(s.turn.iter().zip(l.turn).all(|(a, b)| *a >= b));
    // Each dearer and of a higher tech than the one before.
    assert!(l.price < d.price && d.price < s.price);
    assert!(l.tech < d.tech && d.tech < s.tech);
    assert!(s.tech <= 12, "sold somewhere");
    assert_eq!(world::HOLD_MOST, 60);
    assert_eq!(world::HULL_MOST, 160);
    // Names, models: three different ones.
    assert_ne!(l.name, d.name);
    assert_ne!(d.name, s.name);
    for i in 0..3u8 {
        let m = models::craft_model(i);
        println!(
            "{}: {} corners, {} faces, radius {} m",
            CRAFTS[usize::from(i)].name,
            m.verts.len(),
            m.faces.len(),
            m.radius / 16
        );
    }
    assert!(!core::ptr::eq(
        models::craft_model(0),
        models::craft_model(1)
    ));
    assert!(!core::ptr::eq(
        models::craft_model(1),
        models::craft_model(2)
    ));
    assert!(!core::ptr::eq(
        models::craft_model(0),
        models::craft_model(2)
    ));
}

/// Launched from a station that sells ship `i`, flying it, put 30 km
/// out into empty space looking away from the station.
fn flying(i: u8) -> Box<World> {
    let mut w = docked_at(system_with(CRAFTS[usize::from(i)].tech, 99), 0);
    w.car.ship = i;
    w.car.hull = CRAFTS[usize::from(i)].hull;
    w.launch();
    let b = w.bodies;
    let out = b.station_rot.col(2);
    w.me.pos = world::add(b.station_pos, scale14(out, 30 * world::KM));
    w.me.rot = Mat3::looking(out, [0, ONE14, 0]);
    w.me.speed = 0;
    w.me.throttle = 0;
    w.ships = [None; world::SHIPS];
    w
}

#[test]
fn each_ship_flies_by_its_own_numbers() {
    for (i, c) in CRAFTS.iter().enumerate() {
        for shield in 0..3u8 {
            let mut w = docked_at(system_with(c.tech, 99), 0);
            w.car.ship = i as u8;
            w.car.hull = c.hull;
            w.car.shield = shield;
            w.launch();
            // Launched: the ship's shield and overboost charge, full.
            let most = SHIELD_POINTS[usize::from(shield)] * i32::from(c.shield) / 4;
            assert_eq!(w.shield_max(), most, "{} shield {shield}", c.name);
            assert_eq!(w.me.shield, most, "{} shield {shield}", c.name);
            assert_eq!(w.me.od, c.od_full, "{}", c.name);
        }
        // Full throttle for 20 s: up to the top speed, never past it.
        let mut w = flying(i as u8);
        let mut fastest = 0;
        for _ in 0..60 * 20 {
            w.tick(&Pad {
                throttle: 256,
                ..Pad::default()
            });
            w.ships = [None; world::SHIPS];
            while w.take_event().is_some() {}
            fastest = fastest.max(w.me.speed);
        }
        assert_eq!(fastest, c.top, "{} top speed", c.name);
        assert_eq!(w.me.throttle, c.top, "{} throttle", c.name);
        // Full stick: the ship's own turn rates, once eased in.
        for (k, pad) in [
            Pad {
                steer: (0, 256),
                ..Pad::default()
            },
            Pad {
                steer: (256, 0),
                ..Pad::default()
            },
            Pad {
                roll: 256,
                ..Pad::default()
            },
        ]
        .into_iter()
        .enumerate()
        {
            let mut w = flying(i as u8);
            for _ in 0..60 {
                w.tick(&pad);
                w.ships = [None; world::SHIPS];
                while w.take_event().is_some() {}
            }
            assert_eq!(w.me.rates[k], c.turn[k], "{} turn {k}", c.name);
        }
        // The overboost top: a quarter-steps of the top speed.
        let w = flying(i as u8);
        assert_eq!(w.od_top(), c.top * i32::from(c.od_top) / 4, "{}", c.name);
        assert_eq!(w.top(), c.top);
        assert_eq!(w.od_full(), c.od_full);
        // Hits take the hull down from the ship's own whole hull.
        assert_eq!(w.car.hull_max(), i32::from(c.hull));
        assert_eq!(w.car.capacity(), u32::from(c.hold));
    }
}

#[test]
fn the_overboost_runs_at_each_ships_own_top() {
    for (i, c) in CRAFTS.iter().enumerate() {
        let mut w = flying(i as u8);
        w.me.speed = c.top;
        w.me.throttle = c.top;
        // A raider near: cruise is refused, so the held press boosts.
        let at = world::add(w.me.pos, scale14(w.me.rot.col(2), 2 * world::KM));
        let mut fastest = 0;
        let mut ticks = 0;
        for t in 0..60 * 30 {
            if w.ships.iter().flatten().count() == 0 {
                w.add_ship(Kind::Raider, world::add(at, [0, 0, 0]), Mode::Hunt);
            }
            w.me.shield = w.shield_max();
            w.car.hull = c.hull;
            w.tick(&Pad {
                cruise: t == 0,
                boost: true,
                ..Pad::default()
            });
            while w.take_event().is_some() {}
            if w.me.boost {
                ticks += 1;
            }
            fastest = fastest.max(w.me.speed);
            if !w.me.boost && t > 0 {
                break;
            }
        }
        let top = c.top * i32::from(c.od_top) / 4;
        println!(
            "{}: overboost {ticks} ticks ({:.1} s), fastest {} m/s (top {} m/s)",
            c.name,
            f64::from(ticks) / 60.0,
            fastest * 15 / 64,
            top * 15 / 64
        );
        assert!(fastest <= top && fastest > c.top, "{}", c.name);
        // The charge runs out after od_full / 3 ticks (3 a tick).
        let want = (i32::from(c.od_full) + 2) / 3;
        assert!(
            (want - 2..=want + 1).contains(&ticks),
            "{}: {ticks} of {want}",
            c.name
        );
    }
}

#[test]
fn the_trade_in_is_three_quarters_less_the_mending() {
    let w = docked_at(system_with(12, 99), 0);
    let mut c = w.car;
    assert_eq!(TRADE_IN, (3, 4));
    for own in 0..3u8 {
        c.ship = own;
        let o = &CRAFTS[usize::from(own)];
        for hull in [o.hull, o.hull / 2, 1] {
            c.hull = hull;
            let mend = (i32::from(o.hull) - i32::from(hull)) * REPAIR_PRICE as i32;
            let back = (o.price as i32 * 3 / 4 - mend).max(0);
            for (i, n) in CRAFTS.iter().enumerate() {
                assert_eq!(c.ship_cost(i), n.price as i32 - back, "{own} -> {i}");
            }
        }
    }
    // The numbers the notes give, for a whole hull.
    c.ship = LARK;
    c.hull = 100;
    assert_eq!(c.ship_cost(usize::from(DRAY)), 3_600 - 1_200);
    assert_eq!(c.ship_cost(usize::from(SHRIKE)), 9_000 - 1_200);
    c.ship = DRAY;
    c.hull = 70;
    assert_eq!(c.ship_cost(usize::from(SHRIKE)), 9_000 - 2_700);
    assert_eq!(c.ship_cost(usize::from(LARK)), 1_600 - 2_700);
    c.ship = SHRIKE;
    c.hull = 160;
    assert_eq!(c.ship_cost(usize::from(LARK)), 1_600 - 6_750);
    assert_eq!(c.ship_cost(usize::from(DRAY)), 3_600 - 6_750);
    // Past the table: nothing.
    assert_eq!(c.ship_cost(3), 0);
}

#[test]
fn buying_a_ship_and_its_refusals() {
    // Refused while flying, and for a number past the table.
    let mut w = flying(LARK);
    assert_eq!(w.buy_ship(usize::from(DRAY)), Err("?"));
    let mut w = docked_at(system_with(12, 99), 100_000);
    assert_eq!(w.buy_ship(3), Err("?"));
    assert_eq!(w.buy_ship(usize::from(LARK)), Err("ALREADY FLYING IT"));
    // A station of tech 4 to 6 sells the DRAY, not the SHRIKE.
    let mut w2 = docked_at(system_with(4, 7), 100_000);
    assert!(w2.ship_sold_here(usize::from(DRAY)));
    assert!(!w2.ship_sold_here(usize::from(SHRIKE)));
    assert_eq!(w2.buy_ship(usize::from(SHRIKE)), Err("NOT SOLD HERE"));
    // Every station sells the LARK; tech 1 to 3 only the LARK.
    let w3 = docked_at(system_with(1, 4), 0);
    assert!(w3.ship_sold_here(usize::from(LARK)));
    assert!(!w3.ship_sold_here(usize::from(DRAY)));
    // Cargo past the new hold: sell first.
    w.car.hold.0 = [10, 0, 3, 0, 0, 0, 0, 0];
    assert_eq!(w.buy_ship(usize::from(SHRIKE)), Err("SELL CARGO FIRST"));
    // Too few credits: nothing changes.
    w.car.hold.0 = [10, 0, 2, 0, 0, 0, 0, 0];
    w.car.credits = 7_799;
    let before = w.car;
    assert_eq!(w.buy_ship(usize::from(SHRIKE)), Err("NOT ENOUGH CREDITS"));
    assert_eq!(w.car, before);
    // Just enough: the SHRIKE, whole, with its shield and overboost;
    // cargo, gear, fuel and TAGs move over.
    w.car.credits = 7_800;
    w.car.laser = 1;
    w.car.shield = 2;
    w.car.scoop = true;
    w.car.tags = 2;
    w.car.owed = 100;
    w.car.hull = 100;
    let before = w.car;
    assert_eq!(w.buy_ship(usize::from(SHRIKE)), Ok(()));
    assert_eq!(w.car.credits, 0);
    assert_eq!(w.car.ship, SHRIKE);
    assert_eq!(w.car.hull, 160);
    assert_eq!(w.me.shield, w.shield_max());
    assert_eq!(w.shield_max(), SHIELD_POINTS[2] * 5 / 4);
    assert_eq!(w.me.od, CRAFTS[2].od_full);
    assert_eq!(
        Career {
            credits: before.credits,
            ship: before.ship,
            hull: before.hull,
            ..w.car
        },
        before
    );
    // Back down to the LARK: the station pays the difference.
    w.car.hold.0 = [0; 8];
    assert_eq!(w.buy_ship(usize::from(LARK)), Ok(()));
    assert_eq!(w.car.credits, 6_750 - 1_600);
    assert_eq!(w.car.hull, 100);
    // Credits stay capped.
    w.car.credits = CREDITS_MAX - 10;
    w.car.ship = SHRIKE;
    w.car.hull = 160;
    assert_eq!(w.buy_ship(usize::from(LARK)), Ok(()));
    assert_eq!(w.car.credits, CREDITS_MAX);
    // A DRAY at 1 hull point: 69 points of mending at 2 cr come off
    // its 2,700.
    w.car.ship = DRAY;
    w.car.hull = 1;
    w.car.credits = 10_000;
    assert_eq!(w.buy_ship(usize::from(SHRIKE)), Ok(()));
    assert_eq!(w.car.credits, 10_000 - (9_000 - (2_700 - 69 * 2)));
}

/// A driver docked at a station that sells every ship, with credits.
fn yard_driver(credits: u32) -> Driver {
    let mut d = Driver::new();
    d.start();
    let mut c = Career::new(seed_of(1));
    c.system = system_with(12, 99);
    c.target = c.system;
    c.credits = credits;
    d.app.world.begin(c);
    d.app.last_dock = c;
    d.idle(2);
    d
}

#[test]
fn the_shipyard_buys_by_keys_and_by_two_taps() {
    let mut d = yard_driver(20_000);
    assert_eq!(d.app.scene, Scene::Docked);
    // The fifth line of the dock menu.
    d.app.sel = 4;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Shipyard);
    assert_eq!(d.app.sel, LARK, "the flown ship selected");
    d.idle(20);
    shot(&mut d, "M5-shipyard-lark");
    // Keys: down to the DRAY, A buys it.
    d.tap(button::DOWN);
    assert_eq!(d.app.sel, DRAY);
    d.idle(20);
    shot(&mut d, "M5-shipyard-dray");
    d.events.clear();
    d.tap(button::A);
    assert_eq!(d.app.world.car.ship, DRAY);
    assert_eq!(d.app.world.car.credits, 20_000 - 2_400);
    assert_eq!(
        d.events,
        vec![Event::Ship {
            from: LARK,
            to: DRAY,
            credits: 17_600
        }]
    );
    assert_eq!(d.app.note.map(|n| n.0), Some("NEW SHIP: ENJOY"));
    // A again on the flown ship: refused.
    d.tap(button::A);
    assert_eq!(d.app.note.map(|n| n.0), Some("ALREADY FLYING IT"));
    // Touch: the first tap on the SHRIKE's row selects it, the second
    // buys it.
    let row = |r: i16| hud::LIST_Y + r * hud::LIST_STEP + 2;
    d.events.clear();
    d.touch(60, row(2));
    assert_eq!(d.app.sel, SHRIKE);
    assert_eq!(d.app.world.car.ship, DRAY);
    d.idle(20);
    shot(&mut d, "M5-shipyard-shrike");
    d.touch(60, row(2));
    assert_eq!(d.app.world.car.ship, SHRIKE);
    assert_eq!(d.app.world.car.credits, 17_600 - (9_000 - 2_700));
    assert_eq!(d.events.len(), 1);
    d.idle(20);
    shot(&mut d, "M5-shipyard-bought");
    // BACK (the box): the dock menu on the SHIPYARD line, and the last
    // dock (what a save would hold) has the new ship.
    d.touch(hud::BACK_BOX.0 + 4, hud::BACK_BOX.1 + 4);
    assert_eq!(d.app.scene, Scene::Docked);
    assert_eq!(d.app.sel, 4);
    assert_eq!(d.app.last_dock.ship, SHRIKE);
    d.idle(20);
    shot(&mut d, "M5-docked-menu");
    // In and out by B, and by START.
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Shipyard);
    d.tap(button::B);
    assert_eq!(d.app.scene, Scene::Docked);
    d.tap(button::A);
    d.tap(button::START);
    assert_eq!(d.app.scene, Scene::Docked);
    // Too few credits: a note, nothing bought.
    let mut d = yard_driver(100);
    d.app.sel = 4;
    d.tap(button::A);
    d.tap(button::DOWN);
    d.tap(button::A);
    assert_eq!(d.app.note.map(|n| n.0), Some("NOT ENOUGH CREDITS"));
    assert_eq!(d.app.world.car.ship, LARK);
    // A tap on the dock menu's fifth line opens the shipyard too.
    d.tap(button::B);
    assert_eq!(d.app.scene, Scene::Docked);
}

/// A driver flying ship `i` in the chase view, out in empty space, still.
fn chase(i: u8, detail: Detail) -> Driver {
    let mut d = yard_driver(0);
    d.app.opts.detail = detail;
    d.app.opts.eye = Eye::Chase;
    d.app.world.car.ship = i;
    d.app.world.car.hull = CRAFTS[usize::from(i)].hull;
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    d
}

/// The ship put at `pos`, looking at `at`, still.
fn place(d: &mut Driver, pos: V3, at: V3) {
    let w = &mut d.app.world;
    w.me.pos = pos;
    w.me.rot = Mat3::looking(sub(at, pos), [0, ONE14, 0]);
    w.me.speed = 0;
    w.me.throttle = 0;
    w.me.rates = [0; 3];
    w.ships = [None; world::SHIPS];
}

#[test]
fn the_three_ships_differ_in_the_chase_view() {
    // Two places: empty space (stars only), and in front of the station.
    for (where_, near) in [("space", false), ("station", true)] {
        let mut pics = Vec::new();
        for i in 0..3u8 {
            let mut d = chase(i, Detail::High);
            let b = d.app.world.bodies;
            let out = b.station_rot.col(2);
            if near {
                let pos = world::add(b.station_pos, scale14(out, 2 * world::KM));
                place(&mut d, pos, b.station_pos);
            } else {
                let pos = world::add(b.station_pos, scale14(out, 40 * world::KM));
                place(&mut d, pos, world::add(pos, scale14(out, world::KM)));
            }
            d.idle(1);
            let name = CRAFTS[usize::from(i)].name.to_ascii_lowercase();
            shot(&mut d, &format!("M5-chase-{where_}-{name}"));
            pics.push(d.frame.0.to_vec());
        }
        for (a, b) in [(0, 1), (1, 2), (0, 2)] {
            let diff: Vec<usize> = (0..pics[a].len())
                .filter(|&k| pics[a][k] != pics[b][k])
                .collect();
            let (x0, x1) = (
                diff.iter().map(|k| k % game_kit::W).min().unwrap_or(0),
                diff.iter().map(|k| k % game_kit::W).max().unwrap_or(0),
            );
            let (y0, y1) = (
                diff.iter().map(|k| k / game_kit::W).min().unwrap_or(0),
                diff.iter().map(|k| k / game_kit::W).max().unwrap_or(0),
            );
            println!(
                "{where_}: {} and {}: {} pixels differ, in x {x0}..={x1}, y {y0}..={y1}",
                CRAFTS[a].name,
                CRAFTS[b].name,
                diff.len()
            );
            // Of the 19,200 pixels: the ship's picture, not a few edges.
            assert!(diff.len() >= 150, "{where_}: {a} and {b} look alike");
        }
    }
}

/// Step M3's weight of a picture (a triangle's set-up as 40 pixels).
fn score(s: &Stats) -> u32 {
    s.pixels + 40 * s.tris
}

#[test]
fn no_ship_makes_the_chase_view_too_heavy() {
    // The worst picture of a 20 s fight in a lawless system, at each
    // detail level, with each ship in the chase view; and the worst of
    // 15 s on the live SHIPYARD screen (its turning model in front).
    let mut worst = Stats::default();
    for detail in Detail::ALL {
        for i in 0..3u8 {
            let mut c = Career::new(seed_of(1));
            c.system = (0..galaxy::SYSTEMS as u8)
                .find(|&k| {
                    let s = galaxy::system(c.seed, k);
                    s.danger == 3 && s.tech >= CRAFTS[usize::from(i)].tech
                })
                .unwrap_or_else(|| {
                    (0..galaxy::SYSTEMS as u8)
                        .find(|&k| galaxy::system(c.seed, k).danger == 3)
                        .unwrap()
                });
            c.target = c.system;
            c.ship = i;
            c.hull = CRAFTS[usize::from(i)].hull;
            let mut d = Driver::new();
            d.start();
            d.app.world.begin(c);
            d.app.last_dock = c;
            d.app.opts.detail = detail;
            d.app.opts.eye = Eye::Chase;
            d.idle(2);
            d.app.sel = 0;
            d.tap(button::A);
            assert_eq!(d.app.scene, Scene::Flight);
            let w = &mut d.app.world;
            let out = w.bodies.station_rot.col(2);
            let at = world::add(w.bodies.station_pos, scale14(out, 14 * world::KM));
            let side = w.bodies.station_rot.col(0);
            w.me.pos = at;
            w.me.rot = Mat3::looking(side, [0, ONE14, 0]);
            w.ships = [None; world::SHIPS];
            for k in 0..3 {
                let p = world::add(at, scale14(side, 900 * M + k * 500 * M));
                let p = world::add(p, [0, (k - 1) * 120 * M, (k - 1) * 200 * M]);
                w.add_ship(Kind::Raider, p, Mode::Hunt);
            }
            let mut most = Stats::default();
            for _ in 0..200 {
                for _ in 0..6 {
                    let w = &mut d.app.world;
                    if w.state != State::Flying {
                        break;
                    }
                    w.me.shield = w.shield_max();
                    let pad = match hunter(w, 20 * world::KM) {
                        Some(k) => fight_pad(w, k),
                        None => Pad::default(),
                    };
                    w.tick(&pad);
                    while w.take_event().is_some() {}
                }
                d.idle(1);
                let s = d.app.render.stats;
                assert_eq!(s.dropped, 0, "triangles dropped");
                if score(&s) > score(&most) {
                    most = s;
                }
            }
            println!(
                "chase fight {:<6} {detail:?}: objects {:>2} faces {:>3} tris {:>3} pixels {:>6} score {:>6}",
                CRAFTS[usize::from(i)].name,
                most.objects,
                most.faces,
                most.tris,
                most.pixels,
                score(&most)
            );
            worst.tris = worst.tris.max(most.tris);
        }
        let mut d = yard_driver(0);
        d.app.opts.detail = detail;
        d.app.sel = 4;
        d.tap(button::A);
        assert_eq!(d.app.scene, Scene::Shipyard);
        for row in 0..3u8 {
            d.app.sel = row;
            let mut most = Stats::default();
            for _ in 0..300 {
                d.idle(1);
                let s = d.app.render.stats;
                assert_eq!(s.dropped, 0, "triangles dropped");
                if score(&s) > score(&most) {
                    most = s;
                }
            }
            println!(
                "shipyard {:<6} {detail:?}: objects {:>2} faces {:>3} tris {:>3} pixels {:>6} score {:>6}",
                CRAFTS[usize::from(row)].name,
                most.objects,
                most.faces,
                most.tris,
                most.pixels,
                score(&most)
            );
            worst.tris = worst.tris.max(most.tris);
        }
    }
    // The renderer's budget (tests/look.rs): 160 triangles.
    assert!(worst.tris <= 160, "too many triangles: {}", worst.tris);
}
