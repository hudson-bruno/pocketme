//! The two wishes of round "hold-range":
//!
//! * Holding a buy or sell key in the market trades on by itself, by
//!   game time (`app::REPEAT_*`): the same number of trades in the same
//!   time at 30 and at 60 pictures a second; one press is one trade; it
//!   stops at each limit with one note; it never moves to another row or
//!   screen.
//! * In flight, the view shows how far ships are and whether the laser
//!   reaches them: the marks follow the laser's own rule
//!   (`World::in_reach`, `World::first_hit`), checked against an
//!   independent copy of that rule and against real shots.

mod common;

use common::*;
use game_kit::{Touch, button};
use mercator::app::{NOTE_TICKS, REPEAT_FIRST, Scene, repeat_gap};
use mercator::art::{ACCENT, BLIP_RAIDER, TEXT_DIM};
use mercator::fixed::{Mat3, ONE14, V3, scale14, sub, wide};
use mercator::hud;
use mercator::scene::{self, Eye, VIEW};
use mercator::world::{self, KM, Kind, LASER_RANGE, M, Mode, PLAYER, Pad};

// ---- The market ---------------------------------------------------------------

/// Docked at the start, in the market on row 0, with money, room (the
/// DRAY's hold) and stock to spare; calls `call_us` apart (the picture
/// rate: one tick at 60 a second, two at 30).
fn market(call_us: u64) -> Driver {
    let mut d = Driver::new();
    d.call_us = call_us;
    d.app.opts.fast = call_us == CALL_US;
    d.start();
    let c = &mut d.app.world.car;
    c.credits = 1_000_000;
    // Step M5: the DRAY's 60 t hold (the big bay's 40 t before).
    c.ship = world::DRAY;
    d.app.world.stock = [200; 8];
    d.app.sel = 1;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Market);
    assert_eq!(d.app.sel, 0);
    d.idle(2);
    d
}

/// Units of good `g` in the hold.
fn units(d: &Driver, g: usize) -> u32 {
    u32::from(d.app.world.car.hold.0[g])
}

/// Whether the last call set a note (it then has all its ticks left:
/// every call first takes at least one off).
fn note_set(d: &Driver) -> Option<&'static str> {
    d.app
        .note
        .filter(|&(_, left)| left == NOTE_TICKS)
        .map(|(t, _)| t)
}

/// Holds `keys` for `ticks` game ticks (from the press); returns the
/// units of good `g` after each call, with the tick it came at, and the
/// notes set on the way.
fn hold_ticks(
    d: &mut Driver,
    keys: u32,
    g: usize,
    ticks: u64,
) -> (Vec<(u64, u32)>, Vec<&'static str>) {
    let per = d.call_us / CALL_US;
    let mut seen = Vec::new();
    let mut notes = Vec::new();
    let mut t = 0;
    while t <= ticks {
        d.hold(keys);
        seen.push((t, units(d, g)));
        notes.extend(note_set(d));
        t += per;
    }
    (seen, notes)
}

/// Trades a hold of `ticks` makes after the press, by the rule alone.
fn model_repeats(ticks: u32) -> u32 {
    let (mut n, mut next) = (0, REPEAT_FIRST);
    while next <= ticks {
        n += 1;
        next += repeat_gap(next);
    }
    n
}

#[test]
fn the_repeat_runs_at_the_same_speed_at_30_and_60_pictures() {
    let mut runs = Vec::new();
    for call in [CALL_US, 2 * CALL_US] {
        let mut d = market(call);
        // 2.5 s: 31 tonnes, inside the big bay's 40.
        let (seen, notes) = hold_ticks(&mut d, button::A, 0, 150);
        assert!(notes.is_empty(), "{notes:?}");
        // While held, the game asks to be called at its picture rate.
        assert!(!d.app.still());
        runs.push(seen);
    }
    let (sixty, thirty) = (&runs[0], &runs[1]);
    // At every moment both rates see, the same count.
    for &(t, n) in thirty {
        let at60 = sixty.iter().find(|&&(u, _)| u == t).unwrap().1;
        assert_eq!(n, at60, "tick {t}");
    }
    // And the count the rule gives: the press, then the repeats.
    for &(t, n) in sixty {
        assert_eq!(n, 1 + model_repeats(t as u32), "tick {t}");
    }
    let at = |t: u64| sixty.iter().find(|&&(u, _)| u == t).unwrap().1;
    // Nothing more before 0.45 s; 5 in the first second, 11 by 1.5 s,
    // 21 by 2 s, then 20 a second.
    assert_eq!(at(26), 1);
    assert_eq!(at(27), 2);
    assert_eq!(at(60), 5);
    assert_eq!(at(90), 11);
    assert_eq!(at(120), 21);
    assert_eq!(at(150) - at(120), 10);
    println!(
        "units bought while A is held, by tick (60 a second): {:?}",
        sixty
            .iter()
            .filter(|(t, _)| t % 15 == 0)
            .collect::<Vec<_>>()
    );
}

#[test]
fn one_press_trades_exactly_once() {
    for call in [CALL_US, 2 * CALL_US] {
        // A press and let go.
        let mut d = market(call);
        d.tap(button::A);
        d.idle(60);
        assert_eq!(units(&d, 0), 1);
        // Right on the d-pad.
        d.tap(button::RIGHT);
        d.idle(60);
        assert_eq!(units(&d, 0), 2);
        // X sells one.
        d.tap(button::X);
        d.idle(60);
        assert_eq!(units(&d, 0), 1);
        // Held just short of the first repeat: still one.
        let (seen, _) = hold_ticks(&mut d, button::A, 0, u64::from(REPEAT_FIRST) - 2);
        assert_eq!(seen.last().unwrap().1, 2);
        d.idle(30);
        assert_eq!(units(&d, 0), 2);
        // Step M3 (renewed on purpose): the repeat is off; the game rests
        // only when the live docked scene is switched off.
        assert_eq!(d.app.repeat.what, 0);
        assert_eq!(d.app.still(), !mercator::fx::DOCK_LIVE_ON);
        // A tap on the BUY column of row 3.
        let y = hud::LIST_Y + 3 * hud::LIST_STEP + 2;
        d.touch(50, y);
        d.idle(60);
        assert_eq!(units(&d, 3), 1);
        assert_eq!(d.app.sel, 3);
    }
}

/// Holds `keys` for 4 s and checks the hold of good `g` ends at `want`
/// with exactly one note, `text`; then the repeat is off for good.
fn stops_once(d: &mut Driver, keys: u32, g: usize, want: u32, text: &str) {
    let (seen, notes) = hold_ticks(d, keys, g, 240);
    assert_eq!(seen.last().unwrap().1, want, "{text}");
    assert_eq!(notes, vec![text]);
    assert_eq!(d.app.repeat.what, 0);
    // Still held: nothing more, and once the note is gone the game
    // sleeps again (step M3, renewed on purpose: unless the live docked
    // scene keeps it drawing).
    let (more, notes) = hold_ticks(d, keys, g, 120);
    assert!(more.iter().all(|&(_, n)| n == want));
    assert!(notes.is_empty());
    assert_eq!(d.app.repeat.what, 0);
    assert_eq!(d.app.still(), !mercator::fx::DOCK_LIVE_ON);
    d.hold(0);
}

#[test]
fn a_hold_stops_at_each_limit_with_one_note() {
    for call in [CALL_US, 2 * CALL_US] {
        // The hold fills: 3 tonnes of room (step M5: of the DRAY's 60).
        let mut d = market(call);
        d.app.world.car.hold.0[5] = 57;
        stops_once(&mut d, button::A, 0, 3, "THE HOLD IS FULL");
        // The money runs out: enough for 4.
        let mut d = market(call);
        let p = d.app.world.price(0);
        d.app.world.car.credits = p * 4 + p / 2;
        stops_once(&mut d, button::A, 0, 4, "NOT ENOUGH CREDITS");
        // The station runs out: 2 in stock (right on the d-pad).
        let mut d = market(call);
        d.app.world.stock[0] = 2;
        stops_once(&mut d, button::RIGHT, 0, 2, "NONE LEFT HERE");
        // Selling: 5 in the hold, held X sells them all.
        let mut d = market(call);
        d.app.world.car.hold.0[0] = 5;
        stops_once(&mut d, button::X, 0, 0, "NONE IN THE HOLD");
        // Already full: the press says so once, no repeat.
        let mut d = market(call);
        d.app.world.car.hold.0[5] = 60;
        stops_once(&mut d, button::A, 0, 0, "THE HOLD IS FULL");
    }
}

#[test]
fn a_hold_never_moves_to_another_row_or_screen() {
    for call in [CALL_US, 2 * CALL_US] {
        // DOWN while A is held: row 0 stops, row 1 gets nothing.
        let mut d = market(call);
        let (seen, _) = hold_ticks(&mut d, button::A, 0, 40);
        let row0 = seen.last().unwrap().1;
        assert!(row0 >= 2);
        d.hold(button::A | button::DOWN);
        assert_eq!(d.app.sel, 1);
        hold_ticks(&mut d, button::A | button::DOWN, 1, 120);
        hold_ticks(&mut d, button::A, 1, 120);
        assert_eq!((units(&d, 0), units(&d, 1)), (row0, 0));

        // B while A is held: back at the dock, nothing more is bought
        // there; the A that opens the market again, held, buys nothing.
        let mut d = market(call);
        hold_ticks(&mut d, button::A, 0, 40);
        let before = d.app.world.car;
        d.hold(button::A | button::B);
        assert_eq!(d.app.scene, Scene::Docked);
        assert_eq!(d.app.repeat.what, 0);
        hold_ticks(&mut d, button::A, 0, 120);
        d.hold(0);
        assert_eq!(d.app.sel, 1);
        hold_ticks(&mut d, button::A, 0, 180);
        assert_eq!(d.app.scene, Scene::Market);
        assert_eq!(d.app.world.car, before);

        // A finger held on row 2's BUY column repeats; slid down to row
        // 3 it stops, and row 3 gets nothing; lifted, nothing more.
        let mut d = market(call);
        let row_y = |r: i16| hud::LIST_Y + r * hud::LIST_STEP + 2;
        let finger = |y: i16, started: bool, down: bool| Touch {
            x: 50,
            y,
            down,
            started,
            ended: !down,
        };
        let press = |d: &mut Driver, t: Touch| {
            d.call_keys(game_kit_keys(t));
        };
        press(&mut d, finger(row_y(2), true, true));
        for _ in 0..(60 * CALL_US / d.call_us) {
            press(&mut d, finger(row_y(2), false, true));
        }
        let row2 = units(&d, 2);
        assert_eq!(row2, 1 + model_repeats(60));
        for _ in 0..(120 * CALL_US / d.call_us) {
            press(&mut d, finger(row_y(3), false, true));
        }
        press(&mut d, finger(row_y(3), false, false));
        d.idle(60);
        assert_eq!((units(&d, 2), units(&d, 3)), (row2, 0));
        // The SELL column, held: sells.
        press(
            &mut d,
            Touch {
                x: 80,
                ..finger(row_y(2), true, true)
            },
        );
        for _ in 0..(60 * CALL_US / d.call_us) {
            press(
                &mut d,
                Touch {
                    x: 80,
                    ..finger(row_y(2), false, true)
                },
            );
        }
        assert_eq!(units(&d, 2), row2 - 1 - model_repeats(60));

        // The stick held right repeats like the d-pad; let go, it stops.
        let mut d = market(call);
        let stick = |x: i16| mercator::app::Keys {
            left: (x, 0),
            ..Default::default()
        };
        for _ in 0..=(60 * CALL_US / d.call_us) {
            d.call_keys(stick(30_000));
        }
        assert_eq!(units(&d, 0), 1 + model_repeats(60));
        for _ in 0..60 {
            d.call_keys(stick(0));
        }
        assert_eq!(units(&d, 0), 1 + model_repeats(60));
    }
}

fn game_kit_keys(t: Touch) -> mercator::app::Keys {
    mercator::app::Keys {
        touch: Some(t),
        ..Default::default()
    }
}

// ---- Flight: distance and laser reach -------------------------------------------

/// In flight, still, 20 km out from the station, nose along +z exactly,
/// no other ships, the laser cool; in the cockpit view (its crosshair
/// shows the lock).
fn flying() -> Driver {
    let mut d = Driver::new();
    d.app.opts.eye = Eye::Cockpit;
    d.start();
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    d.idle(40);
    let w = &mut d.app.world;
    let b = w.bodies;
    w.me.pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    w.me.rot = Mat3::looking([0, 0, ONE14], [0, ONE14, 0]);
    assert_eq!(w.me.rot.col(2), [0, 0, ONE14]);
    w.me.speed = 0;
    w.me.throttle = 0;
    w.ships = [None; world::SHIPS];
    d
}

/// A ship of `kind` at `off` (x right, y up, z ahead, in world units)
/// from the player, still and holding its fire; hunting the player
/// when a raider (so it is bracketed).
fn put(d: &mut Driver, kind: Kind, off: V3) -> usize {
    let w = &mut d.app.world;
    let p = world::add(w.me.pos, off);
    let i = w.add_ship(kind, p, Mode::Travel).unwrap();
    let s = w.ships[i].as_mut().unwrap();
    s.pos = p;
    s.speed = 0;
    s.cool = 250;
    if kind == Kind::Raider {
        s.target = PLAYER;
        s.mode = Mode::Hunt;
    }
    i
}

/// The laser's rule written out again here, in wide numbers: within
/// LASER_RANGE, in front, and the nose line passes within the ship's
/// size plus 1/40 of the distance ahead (step M2).
fn rule_reach(off: V3) -> bool {
    let d2: i128 = off.iter().map(|&c| i128::from(c) * i128::from(c)).sum();
    d2 <= i128::from(LASER_RANGE) * i128::from(LASER_RANGE)
}
fn rule_hits(kind: Kind, off: V3) -> bool {
    let help = i64::from(kind.size()) + i64::from(off[2] / 40);
    rule_reach(off) && off[2] > 0 && wide(off[0], off[0]) + wide(off[1], off[1]) <= help * help
}

/// Draws the view as it is, without a tick: the ships stay exactly
/// where they were put.
fn look(d: &mut Driver) {
    d.app.draw(&mut d.frame);
}

/// The target line for ship `i` and whether it says IN RANGE.
fn line_of(w: &world::World, i: usize) -> (String, bool) {
    let mut t = hud::Txt::new();
    let reach = hud::target_text(&mut t, w, &w.ships[i].unwrap());
    (t.as_str().to_string(), reach)
}

/// The index of the ship the target line names, and where it is.
fn named(w: &world::World) -> Option<(usize, i32, i32)> {
    let cam = scene::camera(w, Eye::Cockpit);
    let mut scratch = Box::new(game_kit::Frame::new());
    hud::ship_marks(&mut scratch, w, &cam).map(|(sh, x, y)| {
        let i = w.ships.iter().position(|s| s.as_ref() == Some(sh)).unwrap();
        (i, x, y)
    })
}

/// The pixel at (x, y) of the picture.
fn px(d: &Driver, x: i32, y: i32) -> u8 {
    d.frame.0[(y as usize) * game_kit::W + x as usize]
}

/// Whether the crosshair shows the lock: its arms and its middle box
/// amber (four pixels no single bracket or bar can all cover).
fn lock_drawn(d: &Driver) -> bool {
    [(74, 48), (86, 48), (78, 46), (82, 50)]
        .iter()
        .all(|&(x, y)| px(d, x, y) == ACCENT)
}

/// What the view shows for ship `i`: (closed amber box round it, the
/// crosshair's lock).
fn marks(d: &Driver, i: usize) -> (bool, bool) {
    let w = &d.app.world;
    let cam = scene::camera(w, Eye::Cockpit);
    let (x, y) = scene::on_screen(&VIEW, &cam, w.ships[i].unwrap().pos).unwrap();
    // The box's left edge at mid height: the corner brackets leave it
    // empty.
    let boxed = px(d, x - 9, y) == ACCENT;
    if !boxed {
        assert_eq!(px(d, x - 9, y - 9), BLIP_RAIDER, "corner bracket");
    }
    (boxed, lock_drawn(d))
}

/// A shot now: whether ship `i` lost hull.
fn shoot(d: &mut Driver, i: usize) -> bool {
    let w = &mut d.app.world;
    let before = w.ships[i].unwrap().hp;
    w.me.cool = 0;
    w.tick(&Pad {
        fire: true,
        ..Pad::default()
    });
    w.ships[i].is_none_or(|s| s.hp < before)
}

#[test]
fn the_range_marks_are_the_lasers_own_rule() {
    let r = LASER_RANGE;
    // The help at 1 km: the raider's size and 1/40 of 1 km (step M2:
    // 126 + 25 m in sizes A, 84 + 25 m in sizes B; 42 + 50 m before).
    let h1 = Kind::Raider.size() + 25 * M;
    // (where, in range, on aim) — ahead just inside, at, and just
    // outside the range; off the aim line inside the range.
    let cases: [(&str, V3, bool, bool); 9] = [
        ("ahead 2 km", [0, 0, 2000 * M], true, true),
        ("ahead, last unit in", [0, 0, r], true, true),
        ("ahead, first unit out", [0, 0, r + 1], false, false),
        ("ahead 3 km", [0, 0, 3000 * M], false, false),
        // At 1 km 5 m inside the help still hits, 5 m outside not.
        (
            "1 km, 5 m inside the help",
            [h1 - 5 * M, 0, 1000 * M],
            true,
            true,
        ),
        (
            "1 km, 5 m outside the help",
            [0, h1 + 5 * M, 1000 * M],
            true,
            false,
        ),
        ("1.5 km, 600 m off", [600 * M, 0, 1500 * M], true, false),
        // 600 m to the side: 2,396 m away is in, 2,406 m out.
        ("off aim, just in", [600 * M, 0, 2320 * M], true, false),
        ("off aim, just out", [600 * M, 0, 2330 * M], false, false),
    ];
    for (name, off, reach, hit) in cases {
        assert_eq!(rule_reach(off), reach, "{name}: the case itself");
        assert_eq!(rule_hits(Kind::Raider, off), hit, "{name}: the case itself");
        let mut d = flying();
        let i = put(&mut d, Kind::Raider, off);
        look(&mut d);
        let w = &d.app.world;
        let p = w.ships[i].unwrap().pos;
        assert_eq!(w.in_reach(p), reach, "{name}: in_reach");
        assert_eq!(
            w.first_hit().map(|h| h.0),
            hit.then_some(i),
            "{name}: first_hit"
        );
        assert_eq!(marks(&d, i), (reach, hit), "{name}: the marks");
        let (text, shown) = line_of(w, i);
        assert_eq!(shown, reach, "{name}");
        assert_eq!(text.ends_with("IN RANGE"), reach, "{name}");
        assert_eq!(shoot(&mut d, i), hit, "{name}: a real shot");
        println!("{name}: in range {reach}, on aim {hit}: {text}");
    }
    // Behind, 1 km: in reach (IN RANGE on the line, were it named), but
    // the laser only fires ahead: no lock, no hit, and not on the view.
    let mut d = flying();
    let i = put(&mut d, Kind::Raider, [0, 0, -1000 * M]);
    look(&mut d);
    let w = &d.app.world;
    assert!(w.in_reach(w.ships[i].unwrap().pos));
    assert!(w.first_hit().is_none() && !hud::locked(w) && !lock_drawn(&d));
    assert_eq!(named(w), None);
    assert!(!shoot(&mut d, i));
    // Too hot to fire: the same ship on aim, no lock.
    let mut d = flying();
    let i = put(&mut d, Kind::Raider, [0, 0, 1000 * M]);
    d.app.world.me.hot = true;
    d.app.world.me.heat = world::HEAT_MAX;
    look(&mut d);
    assert!(d.app.world.first_hit().is_some());
    assert!(!hud::locked(&d.app.world) && !lock_drawn(&d));
    assert!(!shoot(&mut d, i));
}

/// Random places round the player (each kind, every direction, up to
/// 3.5 km): the marks, the shared rule and the copy of the rule agree
/// every time, and so does a real shot.
#[test]
fn random_ships_get_the_marks_the_laser_agrees_with() {
    let mut seed = 0x1234_5678u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut d = flying();
    let (mut reached, mut hits, mut boxes) = (0, 0, 0);
    for round in 0..600 {
        let kind = [Kind::Raider, Kind::Hauler, Kind::Warden, Kind::Canister][round % 4];
        // Mostly ahead and near the aim line, where the edges are.
        let z = (next() % (3600 * M as u32)) as i32 - if round % 5 == 0 { 1800 * M } else { 0 };
        let spread = (next() % 400 * M as u32) as i32 + 1;
        let x = (next() % (2 * spread as u32)) as i32 - spread;
        let y = (next() % (2 * spread as u32)) as i32 - spread;
        let off = [x, y, z];
        d.app.world.ships = [None; world::SHIPS];
        d.app.world.me.hot = false;
        d.app.world.me.heat = 0;
        let i = put(&mut d, kind, off);
        if kind == Kind::Raider {
            d.app.world.ships[i].as_mut().unwrap().target = PLAYER;
        }
        look(&mut d);
        let w = &d.app.world;
        let p = w.ships[i].unwrap().pos;
        let real = sub(p, w.me.pos);
        let reach = rule_reach(real);
        let hit = rule_hits(kind, real);
        assert_eq!(w.in_reach(p), reach, "{off:?}");
        assert_eq!(w.first_hit().is_some(), hit, "{off:?}");
        assert_eq!(line_of(w, i).1, reach);
        // The crosshair's lock: the same rule, and on the picture.
        assert_eq!(hud::locked(w), hit, "{off:?}");
        assert_eq!(lock_drawn(&d), hit, "{off:?}");
        // Bracketed ships (raiders hunting, canisters) on the view.
        let cam = scene::camera(w, Eye::Cockpit);
        let bracketed = matches!(kind, Kind::Raider | Kind::Canister);
        if bracketed
            && let Some((x, y)) = scene::on_screen(&VIEW, &cam, p)
            && (9..151).contains(&x)
            && (10..87).contains(&y)
        {
            assert_eq!(px(&d, x - 9, y) == ACCENT, reach, "{off:?}");
            boxes += u32::from(reach);
        }
        assert_eq!(shoot(&mut d, i), hit, "{off:?}: a real shot");
        reached += u32::from(reach);
        hits += u32::from(hit);
    }
    println!("600 ships: {reached} in range, {hits} on aim, {boxes} amber boxes seen");
    assert!(reached > 150 && hits > 50 && boxes > 50);
}

#[test]
fn the_target_line_names_the_ship_and_its_distance() {
    let mut d = flying();
    let cases: [(Kind, V3, &str); 9] = [
        (Kind::Raider, [0, 0, 640 * M], "RAIDER 640 M  IN RANGE"),
        (Kind::Hauler, [0, 0, 999 * M], "HAULER 999 M  IN RANGE"),
        (Kind::Warden, [0, 0, 1000 * M], "WARDEN 1.0 KM  IN RANGE"),
        (Kind::Raider, [0, 1800 * M, 0], "RAIDER 1.8 KM  IN RANGE"),
        (
            Kind::Canister,
            [0, 0, -2400 * M],
            "CANISTER 2.4 KM  IN RANGE",
        ),
        (Kind::Raider, [0, 0, 2400 * M + 1], "RAIDER 2.4 KM"),
        (Kind::Raider, [0, 0, 2450 * M], "RAIDER 2.4 KM"),
        (Kind::Hauler, [3000 * M, 0, 4000 * M], "HAULER 5.0 KM"),
        (Kind::Warden, [0, 0, 12_345 * M], "WARDEN 12.3 KM"),
    ];
    for (kind, off, want) in cases {
        d.app.world.ships = [None; world::SHIPS];
        let i = put(&mut d, kind, off);
        assert_eq!(line_of(&d.app.world, i).0, want);
    }

    // Which ship the line names: the one nearest the view's middle...
    d.app.world.ships = [None; world::SHIPS];
    let side = put(&mut d, Kind::Hauler, [700 * M, 0, 1000 * M]);
    let far = put(&mut d, Kind::Warden, [0, 150 * M, 5000 * M]);
    let named_i = |d: &Driver| named(&d.app.world).map(|t| t.0);
    assert_eq!(named_i(&d), Some(far));
    // ...but the one a shot would hit first, when there is one.
    let aimed = put(&mut d, Kind::Canister, [10 * M, 0, 800 * M]);
    assert_eq!(named_i(&d), Some(aimed));
    // Behind or beyond 7 km: not named.
    d.app.world.ships = [None; world::SHIPS];
    put(&mut d, Kind::Raider, [0, 0, -1000 * M]);
    put(&mut d, Kind::Raider, [0, 0, 7100 * M]);
    assert_eq!(named_i(&d), None);
    // On the picture: the line at the view's foot, amber in reach, and
    // the bar under the named ship.
    for (off, c) in [
        ([400 * M, 0, 1500 * M], ACCENT),
        ([400 * M, 0, 3000 * M], TEXT_DIM),
    ] {
        d.app.world.ships = [None; world::SHIPS];
        put(&mut d, Kind::Hauler, off);
        look(&mut d);
        let row: Vec<u8> = (0..160).map(|x| px(&d, x, 89)).collect();
        assert!(row.contains(&c), "the target line is drawn");
        let (_, x, y) = named(&d.app.world).unwrap();
        assert!((x - 2..=x + 2).all(|x| px(&d, x, y + 12) == c), "the bar");
    }
    let _ = side;
}
