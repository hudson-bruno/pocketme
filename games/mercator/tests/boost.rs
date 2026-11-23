//! Step M4, the overboost: where cruise is refused (near the station,
//! the planet or the sun, or hunted), X pressed at top speed and held
//! pushes the ship to 7/4 of top speed while its charge lasts (3 s from
//! full, 9 s to fill again), at half the turn rate, with the laser and
//! the turret dead. The raiders' aim suffers as from hard turning.
//! Level-3 raiders overboost too, and never fire meanwhile.
//!
//! The scripted pilots here measure rules; they do not stand for how
//! hard the game is for a person. `cargo test --release --test boost
//! -- --ignored --nocapture` prints the step's tables (escape, chase,
//! head-on) with and without the overboost.

mod common;

use common::*;
use game_kit::{Touch, button};
use mercator::app::{Keys, Scene, seed_of};
use mercator::art::{ACCENT, BAD, GOOD, TEXT_DIM};
use mercator::fixed::{V3, length, scale14, sub};
use mercator::galaxy;
use mercator::hud;
use mercator::scene::Eye;
use mercator::world::{
    self, Career, Event, FLEE_AFTER, HEAT_MAX, Kind, M, MAX_SPEED, Mode, OD_EVERY, OD_FULL, OD_MIN,
    OD_RUN, OD_TOP, Pad, RAIDER_OD_ON, State, World, raider_boost,
};

// ---- Set-up -----------------------------------------------------------------------

/// A raider's top speed (`Kind::drive`, private to the world).
const RAIDER_TOP: i32 = 1000;

/// A system of `danger` in galaxy 1 (not the start system), the player
/// launched and put `out` metres out on the station's axis, nose out,
/// at top speed; nothing else in space; `day` varies the dice.
fn world_at(danger: u8, out: i32, day: u16) -> Box<World> {
    let seed = seed_of(1);
    let start = galaxy::start_system(seed);
    let mut c = Career::new(seed);
    c.system = (0..galaxy::SYSTEMS as u8)
        .find(|&i| i != start && galaxy::system(seed, i).danger == danger)
        .unwrap();
    c.day = day;
    let mut w = Box::new(World::empty());
    w.begin(c);
    w.launch();
    let b = w.bodies;
    w.me.pos = world::add(b.station_pos, scale14(b.station_rot.col(2), out * M));
    w.me.rot = b.station_rot;
    w.me.shield = w.shield_max();
    w.me.cool = 0;
    w.ships = [None; world::SHIPS];
    for _ in 0..u32::from(day % 64) {
        w.tick(&Pad::default());
        w.ships = [None; world::SHIPS];
    }
    w.me.speed = MAX_SPEED;
    w.me.throttle = MAX_SPEED;
    while w.take_event().is_some() {}
    w
}

/// 2.5 km from the station: cruise is refused there.
fn near() -> Box<World> {
    let w = world_at(1, 2500, 0);
    assert!(w.room() <= 0);
    w
}

/// One tick, then the other ships go (a lone pilot); the events.
fn alone(w: &mut World, pad: &Pad) -> Vec<Event> {
    w.tick(pad);
    w.ships = [None; world::SHIPS];
    let mut v = Vec::new();
    while let Some(e) = w.take_event() {
        v.push(e);
    }
    v
}

/// The X press (cruise) with the key held or not.
fn press(held: bool) -> Pad {
    Pad {
        cruise: true,
        boost: held,
        throttle: 256,
        ..Pad::default()
    }
}

/// X held, no new press.
fn hold() -> Pad {
    Pad {
        boost: true,
        throttle: 256,
        ..Pad::default()
    }
}

fn dist_m(a: V3, b: V3) -> i32 {
    length(sub(a, b)) / M
}

fn set_gun(w: &mut World, i: usize, gun: u8) {
    if let Some(s) = w.ships[i].as_mut() {
        s.gun = gun;
    }
}

/// A raider `ahead` metres in front of the player's nose (behind when
/// below zero), hunting it, of gun level `gun`.
fn raider_at(w: &mut World, ahead: i32, gun: u8) -> usize {
    let p = world::add(w.me.pos, scale14(w.me.rot.col(2), ahead * M));
    let k = w.add_ship(Kind::Raider, p, Mode::Hunt).unwrap();
    set_gun(w, k, gun);
    k
}

// ---- Starting and stopping ----------------------------------------------------------

/// Near the station the press alone is refused as before (the note);
/// with the key held it overboosts instead. Far out, the same press
/// with the key held gives cruise: cruise wins where it is allowed.
#[test]
fn a_held_press_overboosts_only_where_cruise_is_refused() {
    let mut w = near();
    let ev = alone(&mut w, &press(false));
    assert!(ev.contains(&Event::CruiseNo));
    assert!(!w.me.boost && !w.me.cruise);

    let mut w = near();
    let ev = alone(&mut w, &press(true));
    assert!(!ev.contains(&Event::CruiseNo));
    assert!(w.me.boost && !w.me.cruise);
    assert_eq!(w.me.od, OD_FULL - 3);

    // Holding the key without a new press never starts it.
    let mut w = near();
    for _ in 0..60 {
        alone(&mut w, &hold());
        assert!(!w.me.boost);
    }

    let mut w = world_at(1, 20_000, 0);
    assert!(w.room() > 0);
    alone(&mut w, &press(true));
    assert!(w.me.cruise && !w.me.boost);
}

/// Hunted out in space (a raider within 6 km), cruise is refused and the
/// held press overboosts.
#[test]
fn being_hunted_refuses_cruise_and_the_press_overboosts() {
    let mut w = world_at(1, 20_000, 0);
    raider_at(&mut w, -4000, 0);
    w.tick(&press(true));
    assert!(w.me.boost && !w.me.cruise);
}

/// Below [`OD_MIN`] (3/4 s of charge) the press is refused with the note.
#[test]
fn it_needs_a_quarter_charge_to_start() {
    let mut w = near();
    w.me.od = OD_MIN - 1;
    let ev = alone(&mut w, &press(true));
    assert!(ev.contains(&Event::CruiseNo));
    assert!(!w.me.boost);
    // That tick won one back: now it is enough.
    assert_eq!(w.me.od, OD_MIN);
    alone(&mut w, &press(true));
    assert!(w.me.boost);
}

/// From full it runs exactly 3 s, the speed rising to 7/4 of top speed
/// and never past it; then the speed falls back to top within a second.
#[test]
fn it_lasts_three_seconds_and_never_passes_its_top() {
    let mut w = near();
    alone(&mut w, &press(true));
    let mut on = 1;
    let mut top = w.me.speed;
    while w.me.boost {
        alone(&mut w, &hold());
        top = top.max(w.me.speed);
        assert!(w.me.speed <= OD_TOP);
        on += u32::from(w.me.boost);
        assert!(on < 1000);
    }
    assert_eq!(on, u32::from(OD_FULL / 3));
    assert_eq!(top, OD_TOP);
    assert_eq!(w.me.od, 1, "the tick it stopped won one back");
    // The key still held: it does not start again by itself.
    for _ in 0..60 {
        alone(&mut w, &hold());
        assert!(!w.me.boost);
    }
    assert!(w.me.speed <= MAX_SPEED);
}

/// Empty, the charge is back to the start mark in 135 ticks and full in
/// 540 (9 s); releasing the key stops it at once.
#[test]
fn it_recharges_in_nine_seconds_and_stops_on_release() {
    let mut w = near();
    w.me.od = 0;
    for t in 1..=u32::from(OD_FULL) {
        alone(&mut w, &Pad::default());
        assert_eq!(u32::from(w.me.od), t);
    }
    alone(&mut w, &Pad::default());
    assert_eq!(w.me.od, OD_FULL);

    let mut w = near();
    alone(&mut w, &press(true));
    alone(&mut w, &hold());
    assert!(w.me.boost);
    let left = w.me.od;
    alone(&mut w, &Pad::default());
    assert!(!w.me.boost);
    assert_eq!(w.me.od, left + 1);
}

/// Cruise or the auto-dock ends it at once; launching (and arriving after
/// a jump) gives a full charge.
#[test]
fn cruise_or_the_auto_dock_end_it_and_a_launch_fills_it() {
    let mut w = near();
    alone(&mut w, &press(true));
    assert!(w.me.boost);
    alone(
        &mut w,
        &Pad {
            auto: true,
            boost: true,
            ..Pad::default()
        },
    );
    assert!(w.me.auto && !w.me.boost);

    // Started near the station, then out of its 3 km: X pressed again
    // with the key held gives cruise.
    let mut w = near();
    alone(&mut w, &press(true));
    while w.room() <= 0 {
        alone(&mut w, &hold());
    }
    alone(&mut w, &press(true));
    assert!(w.me.cruise && !w.me.boost);

    let mut w = near();
    w.me.od = 7;
    w.me.boost = true;
    w.launch();
    assert!(!w.me.boost);
    assert_eq!(w.me.od, OD_FULL);
    w.me.od = 7;
    let to = (w.car.system + 1) % galaxy::SYSTEMS as u8;
    w.jump(to);
    assert!(!w.me.boost);
    assert_eq!(w.me.od, OD_FULL);
}

/// The stick turns the ship half as fast while it runs.
#[test]
fn it_turns_at_half_rate() {
    let mut a = near();
    let mut b = near();
    alone(&mut b, &press(true));
    let steer = (256, 256);
    for _ in 0..40 {
        alone(
            &mut a,
            &Pad {
                steer,
                ..Pad::default()
            },
        );
        alone(&mut b, &Pad { steer, ..hold() });
    }
    assert!(b.me.boost);
    assert_eq!(a.me.rates[0] / 2, b.me.rates[0]);
    assert_eq!(a.me.rates[1] / 2, b.me.rates[1]);
    assert_ne!(b.me.rates[1], 0);
}

// ---- No fire ----------------------------------------------------------------------

/// The player's beams fired this tick.
fn my_shots(w: &World) -> usize {
    w.beams.iter().filter(|b| !b.foe && b.age > 0).count()
}

/// A raider 1 km dead ahead, kept there: the laser and the turret stay
/// dead while the overboost runs, and the crosshair does not lock; the
/// same pilot without it fires at once.
#[test]
fn no_laser_and_no_turret_while_it_runs() {
    for boost in [true, false] {
        let mut w = near();
        let k = raider_at(&mut w, 1000, 0);
        let mut fired = 0;
        for t in 0..90 {
            // Kept 1 km ahead, unhurt, not firing back.
            let p = world::add(w.me.pos, scale14(w.me.rot.col(2), 1000 * M));
            if let Some(s) = w.ships[k].as_mut() {
                s.pos = p;
                s.hp = Kind::Raider.hull(w.sys.danger);
                s.cool = 100;
            }
            w.me.od = OD_FULL;
            let pad = Pad {
                fire: true,
                turret: true,
                cruise: boost && t == 0,
                boost,
                ..Pad::default()
            };
            assert!(w.foe_in_sights());
            if t > 0 {
                assert_eq!(hud::locked(&w), !boost);
            }
            w.tick(&pad);
            assert_eq!(w.me.boost, boost);
            fired += my_shots(&w);
            while w.take_event().is_some() {}
        }
        if boost {
            assert_eq!(fired, 0);
        } else {
            assert!(fired > 0);
        }
    }
}

// ---- The raiders ------------------------------------------------------------------

/// Shots and hits of a level-3 raider kept 1 km behind the player, its
/// nose on the player's tail, over `ticks`; the player flies straight on
/// (no turning), with the overboost kept running or not.
fn tail_shots(boost: bool, ticks: u32) -> (u32, u32) {
    let mut w = world_at(1, 20_000, 0);
    let k = raider_at(&mut w, -1000, 2);
    let hull = w.car.hull;
    let (mut shots, mut hits) = (0, 0);
    for t in 0..ticks {
        let p = world::add(w.me.pos, scale14(w.me.rot.col(2), -1000 * M));
        let rot = w.me.rot;
        if let Some(s) = w.ships[k].as_mut() {
            s.pos = p;
            s.rot = rot;
            s.mode = Mode::Hunt;
            s.timer = 0;
        }
        w.me.od = OD_FULL;
        w.me.shield = w.shield_max();
        w.car.hull = hull;
        w.tick(&Pad {
            cruise: boost && t == 0,
            boost,
            ..Pad::default()
        });
        assert_eq!(w.me.boost, boost);
        for i in 0..world::SHIPS {
            if i != k {
                w.ships[i] = None;
            }
        }
        if w.beams.iter().any(|b| b.foe && b.age == 4) {
            shots += 1;
        }
        while let Some(e) = w.take_event() {
            if e == Event::Hit {
                hits += 1;
            }
        }
        assert_eq!(w.state, State::Flying);
    }
    (shots, hits)
}

/// The raiders' aim: a third of its usual sureness while the overboost
/// runs (as a pilot turning hard all the time).
#[test]
fn the_raiders_hit_a_third_as_often() {
    let (s0, h0) = tail_shots(false, 9000);
    let (s1, h1) = tail_shots(true, 9000);
    assert!(s0 > 200 && s1 > 200, "{s0} {s1}");
    let r0 = f64::from(h0) / f64::from(s0);
    let r1 = f64::from(h1) / f64::from(s1);
    let ratio = r1 / r0;
    println!("hits {h0}/{s0} without, {h1}/{s1} with: {ratio:.2}");
    assert!((0.22..0.45).contains(&ratio), "{h0}/{s0} {h1}/{s1}");
}

/// A level-3 raider out of its gun's reach of its prey overboosts for
/// 3 s (7/4 of its top speed, never more) and does not fire meanwhile;
/// a level-2 raider never does.
#[test]
fn level_3_raiders_overboost_out_of_reach_and_hold_fire() {
    for gun in [1u8, 2] {
        let mut w = world_at(3, 20_000, 0);
        w.me.speed = 0;
        w.me.throttle = 0;
        let k = raider_at(&mut w, 4000, gun);
        let top = RAIDER_TOP;
        let (mut on, mut fast) = (0, 0);
        for _ in 0..600 {
            w.me.shield = w.shield_max();
            alone_but(&mut w, k, &Pad::default());
            let Some(s) = w.ships[k] else { break };
            let od = raider_boost(&s);
            on += u32::from(od);
            fast = fast.max(s.speed);
            if od {
                assert!(!w.beams.iter().any(|b| b.foe && b.age == 4));
            }
        }
        if gun == 2 && RAIDER_OD_ON {
            assert_eq!(on, u32::from(OD_RUN));
            assert_eq!(fast, top * 7 / 4);
        } else {
            assert_eq!(on, 0);
            assert!(fast <= top);
        }
    }
}

/// Coming within its gun's reach ends a level-3 raider's overboost
/// early (it fires again from there); the wait for the next window
/// goes on.
#[test]
fn a_raiders_overboost_ends_in_reach() {
    if !RAIDER_OD_ON {
        return;
    }
    let mut w = world_at(3, 20_000, 0);
    let k = raider_at(&mut w, 4000, 2);
    let (mut on, mut shot_at) = (0u32, None);
    for t in 0..600u32 {
        w.me.shield = w.shield_max();
        let s = w.ships[k].unwrap();
        // Straight at it at top speed.
        let pad = Pad {
            steer: steer_to(&w, s.pos),
            ..Pad::default()
        };
        alone_but(&mut w, k, &pad);
        let Some(s) = w.ships[k] else { break };
        if raider_boost(&s) {
            on += 1;
            assert!(dist_m(s.pos, w.me.pos) > 2350);
        }
        if shot_at.is_none() && w.beams.iter().any(|b| b.foe && b.age == 4) {
            shot_at = Some(t);
        }
        if s.mode == Mode::Hunt && t > 0 && on > 0 && !raider_boost(&s) {
            assert!(s.timer <= OD_EVERY - OD_RUN);
        }
    }
    assert!(on > 0 && on < u32::from(OD_RUN), "{on}");
    assert!(shot_at.is_some_and(|t| t < 200), "{shot_at:?}");
}

/// One tick; every ship but `k` goes.
fn alone_but(w: &mut World, k: usize, pad: &Pad) {
    w.tick(pad);
    for i in 0..world::SHIPS {
        if i != k {
            w.ships[i] = None;
        }
    }
    while w.take_event().is_some() {}
}

/// A hunting level-3 raider opens a new window only once the last one
/// is over and its prey is out of reach again.
#[test]
fn a_raiders_windows_come_at_most_every_twelve_seconds() {
    if !RAIDER_OD_ON {
        return;
    }
    let mut w = world_at(3, 20_000, 0);
    let k = raider_at(&mut w, 4000, 2);
    let mut starts = Vec::new();
    let mut was = false;
    for t in 0..3600u32 {
        // Kept 4 km ahead: always out of reach.
        let p = world::add(w.me.pos, scale14(w.me.rot.col(2), 4000 * M));
        if let Some(s) = w.ships[k].as_mut() {
            s.pos = p;
        }
        alone_but(&mut w, k, &Pad::default());
        let s = w.ships[k].unwrap();
        let od = raider_boost(&s);
        if od && !was {
            starts.push(t);
        }
        was = od;
    }
    assert_eq!(starts.len(), 5, "{starts:?}");
    for p in starts.windows(2) {
        assert_eq!(p[1] - p[0], u32::from(OD_EVERY));
    }
}

/// A hurt level-3 raider overboosts the first 3 s of its flight; a
/// level-2 raider runs at its usual 5/4 of top speed.
#[test]
fn a_fleeing_level_3_raider_overboosts_its_first_three_seconds() {
    for gun in [1u8, 2] {
        let mut w = world_at(3, 20_000, 0);
        w.me.speed = 0;
        w.me.throttle = 0;
        let k = raider_at(&mut w, 1500, gun);
        if let Some(s) = w.ships[k].as_mut() {
            s.shots = FLEE_AFTER;
            s.hp = 1;
        }
        let top = RAIDER_TOP;
        let (mut on, mut fast) = (0, 0);
        for _ in 0..300 {
            alone_but(&mut w, k, &Pad::default());
            let Some(s) = w.ships[k] else { break };
            assert_eq!(s.mode, Mode::Flee);
            on += u32::from(raider_boost(&s));
            fast = fast.max(s.speed);
        }
        if gun == 2 && RAIDER_OD_ON {
            assert_eq!(on, u32::from(OD_RUN));
            assert_eq!(fast, top * 7 / 4);
        } else {
            assert_eq!(on, 0);
            assert_eq!(fast, top * 5 / 4);
        }
    }
}

// ---- Determinism ------------------------------------------------------------------

/// A minute of a fight with presses, holds and releases at random: two
/// runs give the same ships, player and events every tick.
#[test]
fn the_same_presses_give_the_same_flight() {
    fn run() -> Vec<(world::Me, [Option<world::Ship>; world::SHIPS], Vec<Event>)> {
        let mut w = world_at(3, 20_000, 9);
        raider_at(&mut w, 4000, 2);
        raider_at(&mut w, -3000, 1);
        let mut r = 0x1234_5678u32;
        let mut held = false;
        let mut out = Vec::new();
        for _ in 0..3600 {
            r ^= r << 13;
            r ^= r >> 17;
            r ^= r << 5;
            let press = r.is_multiple_of(23);
            if r.is_multiple_of(41) {
                held = !held;
            }
            let pad = Pad {
                steer: ((r >> 8) as i32 % 257, (r >> 16) as i32 % 257),
                fire: r & 4 != 0,
                cruise: press,
                boost: held || press,
                throttle: 256,
                ..Pad::default()
            };
            w.tick(&pad);
            let mut ev = Vec::new();
            while let Some(e) = w.take_event() {
                ev.push(e);
            }
            out.push((w.me, w.ships, ev));
            if w.state != State::Flying {
                break;
            }
        }
        out
    }
    let a = run();
    assert!(a.iter().any(|(m, _, _)| m.boost));
    assert_eq!(a, run());
}

// ---- Controls and the panel -----------------------------------------------------------

fn finger(x: i16, y: i16, started: bool) -> Option<Touch> {
    Some(Touch {
        x,
        y,
        down: true,
        started,
        ended: false,
    })
}

/// X held is the overboost's key; a new press of it at top speed is the
/// cruise press. A finger on the faster bars is the same: a new touch at
/// top speed presses, the finger kept there holds.
#[test]
fn the_x_key_and_the_faster_bars_hold_it() {
    let mut d = Driver::new();
    d.app.world.me.throttle = MAX_SPEED;
    let pad = |d: &Driver, k: Keys| d.app.controls(&k);
    let x = Keys {
        held: button::X,
        pressed: button::X,
        ..Keys::default()
    };
    let p = pad(&d, x);
    assert!(p.cruise && p.boost);
    let p = pad(
        &d,
        Keys {
            held: button::X,
            ..Keys::default()
        },
    );
    assert!(!p.cruise && p.boost);
    let p = pad(
        &d,
        Keys {
            held: button::A,
            ..Keys::default()
        },
    );
    assert!(!p.cruise && !p.boost);
    let y = hud::PANEL_Y + 6;
    let p = pad(
        &d,
        Keys {
            touch: finger(100, y, true),
            ..Keys::default()
        },
    );
    assert!(p.cruise && p.boost);
    let p = pad(
        &d,
        Keys {
            touch: finger(100, y, false),
            ..Keys::default()
        },
    );
    assert!(!p.cruise && p.boost);
    // The slower bars and the view never hold it.
    for (x, y) in [(60, y), (80, 40)] {
        let p = pad(
            &d,
            Keys {
                touch: finger(x, y, true),
                ..Keys::default()
            },
        );
        assert!(!p.boost);
    }
}

/// The charge line under SPD on the panel: its length, in amber while
/// it runs, green when it can start, dim below that.
fn charge_line(d: &Driver) -> (usize, u8) {
    let row = &d.frame.0[103 * 160 + 54..103 * 160 + 81];
    let c = row[0];
    (row.iter().take_while(|&&p| p == c).count(), c)
}

/// The panel's charge line and the view's marks follow the overboost in
/// the game itself (pad and touch). `M4_SHOTS=<folder>` writes the
/// pictures.
#[test]
fn the_panel_and_the_view_show_it() {
    let shots = std::env::var("M4_SHOTS").ok();
    for chase in [false, true] {
        let mut d = Driver::new();
        d.start();
        d.tap(button::A);
        assert_eq!(d.app.scene, Scene::Flight);
        d.app.opts.eye = if chase { Eye::Chase } else { Eye::Cockpit };
        d.app.world.me.speed = MAX_SPEED;
        d.app.world.me.throttle = MAX_SPEED;
        d.idle(2);
        let tag = if chase { "chase" } else { "cockpit" };
        let save = |d: &mut Driver, n: &str| {
            if let Some(s) = &shots {
                let c = d.app.colours().to_vec();
                let p = std::path::Path::new(s).join(format!("{tag}-{n}.png"));
                save_frame(&p, &d.frame, &c, 3);
            }
        };
        assert_eq!(charge_line(&d), (26, GOOD));
        save(&mut d, "ready");
        d.hold(button::X);
        for _ in 0..60 {
            d.hold(button::X);
        }
        assert!(d.app.world.me.boost);
        let (n, c) = charge_line(&d);
        assert_eq!(c, ACCENT);
        assert_eq!(
            n,
            usize::from(d.app.world.me.od) * 26 / usize::from(OD_FULL)
        );
        // The cockpit's crosshair is red (its arms round the middle).
        if !chase {
            assert_eq!(d.frame.0[48 * 160 + 74], BAD);
            assert_eq!(d.frame.0[42 * 160 + 80], BAD);
        }
        save(&mut d, "running");
        while d.app.world.me.boost {
            d.hold(button::X);
        }
        d.idle(30);
        // 31 of 540: one pixel, dim.
        let row = &d.frame.0[103 * 160 + 54..103 * 160 + 56];
        assert_eq!(row[0], TEXT_DIM);
        assert_ne!(row[1], TEXT_DIM);
        save(&mut d, "empty");
        d.idle(200);
        assert_eq!(charge_line(&d).1, GOOD);
        save(&mut d, "recharging");
        // By touch: a new touch on the faster bars, the finger kept there.
        if !chase {
            // Back at the station's mouth, where cruise is refused.
            d.app.world.launch();
            d.app.world.me.speed = MAX_SPEED;
            d.app.world.me.throttle = MAX_SPEED;
            for i in 0..40 {
                d.call_keys(Keys {
                    touch: finger(100, hud::PANEL_Y + 6, i == 0),
                    ..Keys::default()
                });
            }
            assert!(d.app.world.me.boost);
            save(&mut d, "touch");
            d.call_keys(Keys {
                touch: Some(Touch {
                    x: 100,
                    y: hud::PANEL_Y + 6,
                    down: false,
                    started: false,
                    ended: true,
                }),
                ..Keys::default()
            });
            assert!(!d.app.world.me.boost);
        }
    }
}

// ---- Tables (measuring only) --------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Plan {
    /// A raider 1.2 km behind; fly straight away at top speed and press
    /// X whenever cruise is off; with OD the key is held (the overboost
    /// whenever the charge allows), without it never.
    Run,
    /// The same, weaving all the way: full yaw stick, half a second
    /// right, half a second left (as M1's dodger, it spoils the aim).
    RunWeave,
    /// A hurt raider turns to flee 2.8 km ahead (out of the laser's
    /// reach) while a second raider
    /// (level 1) hunts from 5 km behind (so no cruise): follow and fire
    /// when a shot would hit; with OD, overboost while it is over 2 km
    /// ahead.
    Chase,
    /// One raider 4 km ahead, hunting: fly at it and fire when a shot
    /// would hit; with OD, overboost until it is 2.6 km away.
    HeadOn,
}

#[derive(Default, Debug, Clone, Copy)]
struct Out {
    /// Seconds to cruise (Run), to the kill (Chase, HeadOn).
    done: Option<f64>,
    dmg: i32,
    died: bool,
    /// Seconds the overboost ran.
    od_s: f64,
    /// The raider got away (Chase).
    lost: bool,
}

fn table_run(plan: Plan, gun: u8, od: bool, trial: u32) -> Out {
    let mut w = world_at(3, 20_000, trial as u16);
    let side = (trial % 5) as i32 * 60;
    let k = match plan {
        Plan::Run | Plan::RunWeave => raider_at(&mut w, -1200, gun),
        Plan::Chase => raider_at(&mut w, 2800, gun),
        Plan::HeadOn => raider_at(&mut w, 4000, gun),
    };
    if let Some(s) = w.ships[k].as_mut() {
        s.pos = world::add(s.pos, scale14(w.me.rot.col(0), side * M));
        if plan == Plan::Chase {
            // Hurt enough to flee at once, but not one shot from death.
            s.shots = FLEE_AFTER;
            s.hp = Kind::Raider.hull(3) * 3 / 10 - 1;
            s.rot = w.me.rot;
        }
    }
    let k2 = (plan == Plan::Chase).then(|| raider_at(&mut w, -5000, 0));
    let mut r = Out::default();
    let mut od_ticks = 0u32;
    for t in 0..60 * 60u32 {
        let target = w.ships[k].filter(|s| s.kind == Kind::Raider);
        let insight = w.foe_in_sights() && w.me.heat < HEAT_MAX * 8 / 10;
        let pad = match (plan, target) {
            (Plan::Run | Plan::RunWeave, _) => Pad {
                steer: if plan == Plan::RunWeave {
                    (if t % 60 < 30 { 256 } else { -256 }, 0)
                } else {
                    (0, 0)
                },
                throttle: 256,
                cruise: !w.me.cruise && !w.me.boost && (!od || w.me.od >= OD_MIN),
                boost: od,
                ..Pad::default()
            },
            (_, Some(s)) => {
                let d = dist_m(s.pos, w.me.pos);
                let want = od
                    && match plan {
                        Plan::Chase => d > 2000,
                        _ => d > 2600,
                    };
                Pad {
                    steer: steer_to(&w, s.pos),
                    throttle: 256,
                    fire: insight,
                    cruise: want && !w.me.boost && w.me.od >= OD_MIN,
                    boost: want,
                    ..Pad::default()
                }
            }
            (_, None) => break,
        };
        let before = w.me.shield + i32::from(w.car.hull);
        w.tick(&pad);
        r.dmg += (before - w.me.shield - i32::from(w.car.hull)).max(0);
        od_ticks += u32::from(w.me.boost);
        for i in 0..world::SHIPS {
            if i != k && Some(i) != k2 && w.ships[i].is_some_and(|s| s.kind != Kind::Canister) {
                w.ships[i] = None;
            }
        }
        while let Some(e) = w.take_event() {
            if let Event::Killed { kind: Kind::Raider } = e
                && w.ships[k].is_none_or(|s| s.kind != Kind::Raider)
                && r.done.is_none()
                && !matches!(plan, Plan::Run | Plan::RunWeave)
            {
                r.done = Some(f64::from(t) / 60.0);
            }
        }
        if w.state == State::Dead {
            r.died = true;
            break;
        }
        if matches!(plan, Plan::Run | Plan::RunWeave) && w.me.cruise {
            r.done = Some(f64::from(t) / 60.0);
            break;
        }
        if !matches!(plan, Plan::Run | Plan::RunWeave)
            && w.ships[k].is_none_or(|s| s.kind != Kind::Raider)
        {
            r.lost = r.done.is_none();
            break;
        }
    }
    r.od_s = f64::from(od_ticks) / 60.0;
    r
}

fn mean(v: impl Iterator<Item = f64>) -> f64 {
    let v: Vec<f64> = v.collect();
    if v.is_empty() {
        f64::NAN
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

#[test]
#[ignore = "measuring only: prints the step's tables (best with --release)"]
fn tables() {
    const N: u32 = 40;
    println!("## Overboost: danger 3, laser MK1, {N} trials each, 60 s at most");
    println!(
        "| plan | raider gun | overboost | done (count, mean s) | damage taken | player died | raider got away | overboost s |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for plan in [Plan::Run, Plan::RunWeave, Plan::Chase, Plan::HeadOn] {
        for gun in 0..3u8 {
            for od in [false, true] {
                let rs: Vec<Out> = (0..N).map(|t| table_run(plan, gun, od, t)).collect();
                let done: Vec<f64> = rs.iter().filter_map(|r| r.done).collect();
                println!(
                    "| {plan:?} | L{} | {} | {} ({:.1}) | {:.1} | {} | {} | {:.1} |",
                    gun + 1,
                    if od { "yes" } else { "no" },
                    done.len(),
                    mean(done.iter().copied()),
                    mean(rs.iter().map(|r| f64::from(r.dmg))),
                    rs.iter().filter(|r| r.died).count(),
                    rs.iter().filter(|r| r.lost).count(),
                    mean(rs.iter().map(|r| r.od_s)),
                );
            }
        }
    }
}
