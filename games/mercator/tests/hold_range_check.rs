//! The checker's attacks on round "hold-range" (separate from the
//! builder's tests/hold_range.rs):
//!
//! * Market: random held keys, sticks, fingers, row changes and screen
//!   changes at 30 and 60 pictures a second. Money, cargo and stock stay
//!   sound; no trade comes without a held or pressed trade input; at
//!   most one note per held stretch; a very long hold cannot overflow.
//! * Flight: ships in line, the station in the way, the chase view and
//!   the distance text at its edges.

mod common;

use common::*;
use game_kit::{Touch, button};
use mercator::app::{Keys, NOTE_TICKS, Repeat, Scene, repeat_gap};
use mercator::art::ACCENT;
use mercator::fixed::{Mat3, ONE14, V3, scale14};
use mercator::hud;
use mercator::market::GOODS;
use mercator::scene::{self, Eye, VIEW};
use mercator::world::{self, KM, Kind, M, Mode, PLAYER, Pad};

// ---- The market ---------------------------------------------------------------

fn market(call_us: u64) -> Driver {
    let mut d = Driver::new();
    d.call_us = call_us;
    d.app.opts.fast = call_us == CALL_US;
    d.start();
    d.app.world.car.credits = 5_000;
    // Step M5: the DRAY's 60 t hold (the big bay's 40 t before).
    d.app.world.car.ship = mercator::world::DRAY;
    d.app.world.stock = [30; GOODS];
    d.app.sel = 1;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Market);
    d.idle(2);
    d
}

/// Credits plus what the hold sells for here: no trade may raise it.
fn worth(d: &Driver) -> u64 {
    let w = &d.app.world;
    let mut v = u64::from(w.car.credits);
    for g in 0..GOODS {
        v += u64::from(w.car.hold.0[g]) * u64::from(w.sell_price(g));
    }
    v
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

/// A random market input that is kept for a random stretch of calls.
#[derive(Clone, Copy, Debug)]
enum Input {
    Nothing,
    Keys(u32),
    Stick(i16, i16),
    /// A finger at (x, y).
    Finger(i16, i16),
}

#[test]
fn random_holds_keep_money_cargo_and_stock_sound() {
    let mut rng = Rng(0x9E37_79B9);
    let (mut trades, mut notes_total, mut stretches) = (0u64, 0u32, 0u32);
    for run in 0..120 {
        let call = if run % 2 == 0 { CALL_US } else { 2 * CALL_US };
        let mut d = market(call);
        let total0: [u32; GOODS] = core::array::from_fn(|g| {
            u32::from(d.app.world.stock[g]) + u32::from(d.app.world.car.hold.0[g])
        });
        let mut last = Input::Nothing;
        let mut prev_held = 0u32;
        let mut prev_touch_down = false;
        for _ in 0..40 {
            let input = match rng.below(10) {
                0 => Input::Nothing,
                1 => Input::Keys(button::A),
                2 => Input::Keys(button::X),
                3 => Input::Keys(button::RIGHT),
                4 => Input::Keys(button::LEFT),
                5 => Input::Keys(
                    [
                        button::A | button::X,
                        button::A | button::DOWN,
                        button::X | button::UP,
                        button::A | button::B,
                        button::RIGHT | button::LEFT,
                    ][rng.below(5) as usize],
                ),
                6 => Input::Stick(
                    [-30_000, 0, 30_000][rng.below(3) as usize],
                    [-30_000, 0, 0, 0, 30_000][rng.below(5) as usize],
                ),
                7 | 8 => Input::Finger(
                    [10, 44, 50, 71, 72, 80, 99, 100, 130][rng.below(9) as usize],
                    hud::LIST_Y + rng.below(9) as i16 * hud::LIST_STEP + 2,
                ),
                _ => Input::Keys(button::A),
            };
            // Back at the dock: into the market again (line 1, A).
            if d.app.scene == Scene::Docked {
                d.idle(2);
                d.app.sel = 1;
                d.tap(button::A);
                assert_eq!(d.app.scene, Scene::Market);
                d.hold(0);
                prev_held = 0;
            }
            stretches += 1;
            let calls = 1 + rng.below(150);
            let mut notes_here = 0;
            for c in 0..calls {
                let held = match input {
                    Input::Keys(b) => b,
                    _ => 0,
                };
                let touch = match input {
                    Input::Finger(x, y) => Some(Touch {
                        x,
                        y,
                        down: true,
                        started: !prev_touch_down,
                        ended: false,
                    }),
                    _ if prev_touch_down => Some(Touch {
                        x: 0,
                        y: 0,
                        down: false,
                        started: false,
                        ended: true,
                    }),
                    _ => None,
                };
                let left = match input {
                    Input::Stick(x, y) => (x, y),
                    _ => (0, 0),
                };
                let k = Keys {
                    held,
                    pressed: held & !prev_held,
                    left,
                    touch,
                    ..Keys::default()
                };
                let scene0 = d.app.scene;
                let sel0 = d.app.sel;
                let hold0 = d.app.world.car.hold;
                let worth0 = worth(&d);
                d.call_keys(k);
                prev_held = held;
                prev_touch_down = matches!(input, Input::Finger(..));
                let w = &d.app.world;
                // Sound after every call.
                assert!(w.car.hold.total() <= w.car.capacity());
                for (g, &t) in total0.iter().enumerate() {
                    assert_eq!(u32::from(w.stock[g]) + u32::from(w.car.hold.0[g]), t);
                }
                assert!(worth(&d) <= worth0, "a trade made money: {input:?}");
                let moved: Vec<usize> = (0..GOODS)
                    .filter(|&g| w.car.hold.0[g] != hold0.0[g])
                    .collect();
                if !moved.is_empty() {
                    trades += 1;
                    assert_eq!(scene0, Scene::Market, "traded outside the market");
                    assert_eq!(moved.len(), 1, "two goods in one call: {input:?}");
                    let g = moved[0];
                    let delta = i32::from(w.car.hold.0[g]) - i32::from(hold0.0[g]);
                    // A press of both keys trades twice, but nets out.
                    assert!(delta.abs() <= 2, "{delta} in one call");
                    // Only on the row selected (before, or by this
                    // call's finger or step).
                    assert!(g == usize::from(sel0) || g == usize::from(d.app.sel), "row");
                    assert!(
                        !matches!(input, Input::Nothing),
                        "a trade with nothing held: call {c} of {input:?}"
                    );
                    if let Input::Finger(x, _) = input {
                        assert!((44..100).contains(&x), "a finger off the columns traded");
                        assert_eq!(delta > 0, x < 72, "the finger's column decides");
                    }
                    if let Input::Stick(x, _) = input {
                        assert_ne!(x, 0, "a stick held up or down traded");
                        assert_eq!(delta > 0, x > 0);
                    }
                }
                if d.app.note.is_some_and(|(_, l)| l == NOTE_TICKS) && c > 0 {
                    notes_here += 1;
                }
                if matches!(input, Input::Nothing) {
                    assert_eq!(d.app.repeat.what, 0, "a repeat with nothing held");
                }
                if d.app.scene != Scene::Market {
                    assert_eq!(d.app.repeat.what, 0, "a repeat off the market");
                    break;
                }
            }
            // While one input is held, at most one note after its press.
            assert!(
                notes_here <= 1,
                "{notes_here} notes in one held stretch of {input:?}"
            );
            notes_total += notes_here;
            last = input;
        }
        let _ = last;
    }
    println!(
        "{stretches} held stretches, {trades} calls that traded, {notes_total} notes after a press"
    );
    assert!(trades > 1000);
}

/// A repeat state far along (as if held for two years) cannot overflow:
/// the counters wrap, nothing panics, and limits still stop it.
#[test]
fn a_very_long_hold_cannot_overflow() {
    for call in [CALL_US, 2 * CALL_US] {
        let mut d = market(call);
        d.hold(button::A);
        assert_eq!(d.app.repeat.what, 1);
        d.app.repeat = Repeat {
            what: 1,
            row: 0,
            held: u32::MAX - 3,
            next: u32::MAX - 1,
        };
        for _ in 0..200 {
            d.hold(button::A);
        }
        let w = &d.app.world;
        assert!(w.car.hold.total() <= w.car.capacity());
        assert!(repeat_gap(u32::MAX) >= mercator::app::REPEAT_FAST);
        assert_eq!(repeat_gap(0), mercator::app::REPEAT_SLOW);
    }
}

/// A note opened by something else during a hold does not stop or
/// double the repeat; a whole run of the bay from empty to full gives
/// the same count at both rates, and selling it all back never pays.
#[test]
fn a_foreign_note_and_a_full_run_round_trip() {
    let mut counts = Vec::new();
    for call in [CALL_US, 2 * CALL_US] {
        let mut d = market(call);
        d.app.world.car.credits = 1_000_000;
        d.app.world.stock = [200; GOODS];
        let c0 = d.app.world.car.credits;
        let mut calls = 0;
        while d.app.world.car.hold.total() < 40 && calls < 1000 {
            if calls == 20 {
                d.app.note("SOMETHING ELSE");
            }
            d.hold(button::A);
            calls += 1;
        }
        assert_eq!(d.app.world.car.hold.0[0], 40);
        counts.push(calls * (call / CALL_US) as u32);
        d.hold(0);
        d.idle(5);
        // Sell it all back, held.
        let mut calls = 0;
        while d.app.world.car.hold.total() > 0 && calls < 1000 {
            d.hold(button::X);
            calls += 1;
        }
        d.hold(0);
        assert_eq!(d.app.world.car.hold.total(), 0);
        assert!(d.app.world.car.credits < c0, "a round trip paid");
        println!(
            "{} pictures/s: 40 t in {} ticks; round trip lost {} credits",
            if call == CALL_US { 60 } else { 30 },
            counts.last().unwrap(),
            c0 - d.app.world.car.credits
        );
    }
    // The same real time at both rates (to one call of two ticks).
    assert!(counts[0].abs_diff(counts[1]) <= 2, "{counts:?}");
}

/// A buy key and a sell key held together repeat nothing: pressed in
/// the same call they buy and sell one each, as before; a buy pressed
/// while X sells on stops the selling after its one trade.
#[test]
fn buy_and_sell_held_together_repeat_nothing() {
    for call in [CALL_US, 2 * CALL_US] {
        let mut d = market(call);
        d.app.world.car.hold.0[0] = 10;
        // Both at once: +1 -1, then nothing for 2 s.
        for _ in 0..(120 * CALL_US / call) {
            d.hold(button::A | button::X);
        }
        assert_eq!(d.app.world.car.hold.0[0], 10);
        assert_eq!(d.app.repeat.what, 0);
        d.hold(0);
        // X held sells on; A pressed on top buys one; then both held:
        // nothing more.
        for _ in 0..(60 * CALL_US / call) {
            d.hold(button::X);
        }
        let sold = d.app.world.car.hold.0[0];
        assert!(sold <= 6, "{sold}");
        for _ in 0..(120 * CALL_US / call) {
            d.hold(button::X | button::A);
        }
        assert_eq!(d.app.world.car.hold.0[0], sold + 1);
        // A let go, X still held: no new press, nothing more.
        for _ in 0..(60 * CALL_US / call) {
            d.hold(button::X);
        }
        assert_eq!(d.app.world.car.hold.0[0], sold + 1);
        // Right held and the stick pushed left: the stick's step sells
        // one, then nothing more.
        d.hold(0);
        for _ in 0..(120 * CALL_US / call) {
            d.call_keys(Keys {
                held: button::RIGHT,
                pressed: 0,
                left: (-30_000, 0),
                ..Keys::default()
            });
        }
        assert_eq!(d.app.world.car.hold.0[0], sold);
    }
}

/// A finger and a button at once: the finger decides; A held with a
/// finger elsewhere does not repeat. The finger on the BACK box while A
/// repeats: back to the dock, the repeat ends.
#[test]
fn finger_and_button_together() {
    let mut d = market(CALL_US);
    d.app.world.car.credits = 1_000_000;
    let y = hud::LIST_Y + 2;
    // A held, then a finger lands on row 0's SELL column and stays.
    for _ in 0..40 {
        d.hold(button::A);
    }
    let bought = d.app.world.car.hold.0[0];
    assert!(bought >= 2);
    let finger = |started| Touch {
        x: 80,
        y,
        down: true,
        started,
        ended: false,
    };
    d.call_keys(Keys {
        held: button::A,
        touch: Some(finger(true)),
        ..Keys::default()
    });
    for _ in 0..90 {
        d.call_keys(Keys {
            held: button::A,
            touch: Some(finger(false)),
            ..Keys::default()
        });
    }
    // The tap sold one; the held finger sells on; A held buys nothing.
    let now = d.app.world.car.hold.0[0];
    assert!(now < bought, "{bought} -> {now}");
    // A finger on the BACK box while A repeats.
    let mut d = market(CALL_US);
    for _ in 0..40 {
        d.hold(button::A);
    }
    assert_eq!(d.app.repeat.what, 1);
    d.call_keys(Keys {
        held: button::A,
        touch: Some(Touch {
            x: hud::BACK_BOX.0 + 4,
            y: hud::BACK_BOX.1 + 4,
            down: true,
            started: true,
            ended: false,
        }),
        ..Keys::default()
    });
    assert_eq!(d.app.scene, Scene::Docked);
    assert_eq!(d.app.repeat.what, 0);
}

// ---- Flight -------------------------------------------------------------------

fn flying() -> Driver {
    let mut d = Driver::new();
    d.start();
    d.app.sel = 0;
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Flight);
    d.idle(40);
    let w = &mut d.app.world;
    let b = w.bodies;
    w.me.pos = world::add(b.station_pos, scale14(b.station_rot.col(2), 20 * KM));
    w.me.rot = Mat3::looking([0, 0, ONE14], [0, ONE14, 0]);
    w.me.speed = 0;
    w.me.throttle = 0;
    w.ships = [None; world::SHIPS];
    d
}

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

fn named(d: &Driver, eye: Eye) -> Option<usize> {
    let w = &d.app.world;
    let cam = scene::camera(w, eye);
    let mut scratch = Box::new(game_kit::Frame::new());
    hud::ship_marks(&mut scratch, w, &cam)
        .map(|(sh, _, _)| w.ships.iter().position(|s| s.as_ref() == Some(sh)).unwrap())
}

fn shoot_hp(d: &mut Driver) -> Vec<i32> {
    let w = &mut d.app.world;
    w.me.cool = 0;
    w.tick(&Pad {
        fire: true,
        ..Pad::default()
    });
    w.ships.iter().map(|s| s.map_or(-1, |s| s.hp)).collect()
}

#[test]
fn ships_in_line_the_nearest_is_named_locked_and_hit() {
    let mut d = flying();
    let far = put(&mut d, Kind::Raider, [0, 0, 2000 * M]);
    let mid = put(&mut d, Kind::Hauler, [20 * M, 0, 1400 * M]);
    let near = put(&mut d, Kind::Raider, [0, 10 * M, 800 * M]);
    d.app.draw(&mut d.frame);
    let w = &d.app.world;
    assert_eq!(w.first_hit().map(|h| h.0), Some(near));
    assert!(hud::locked(w));
    assert_eq!(named(&d, Eye::Cockpit), Some(near));
    let before: Vec<i32> = d
        .app
        .world
        .ships
        .iter()
        .map(|s| s.map_or(-1, |s| s.hp))
        .collect();
    let after = shoot_hp(&mut d);
    assert!(after[near] < before[near]);
    assert_eq!(after[mid], before[mid]);
    assert_eq!(after[far], before[far]);
}

/// The laser passes through the station (an old rule): the marks say
/// the same, a raider behind the station is locked and is hit.
#[test]
fn the_station_in_the_way() {
    let mut d = flying();
    let me = d.app.world.me.pos;
    d.app.world.bodies.station_pos = world::add(me, [0, 0, 1000 * M]);
    let r = put(&mut d, Kind::Raider, [0, 0, 2000 * M]);
    d.app.draw(&mut d.frame);
    let w = &d.app.world;
    let locked = hud::locked(w);
    let hp = w.ships[r].unwrap().hp;
    let after = shoot_hp(&mut d);
    let hit = after[r] < hp;
    println!("station in the way: locked {locked}, hit {hit}");
    assert_eq!(locked, hit, "the mark and the shot disagree");
    shot(&mut d, "chk-station-in-way");
}

/// The chase view: no crosshair, but the box and the line follow the
/// same rule.
#[test]
fn the_chase_view_shows_the_same_reach() {
    let mut d = flying();
    d.app.opts.eye = Eye::Chase;
    let i = put(&mut d, Kind::Raider, [150 * M, 0, 1500 * M]);
    let o = put(&mut d, Kind::Raider, [-900 * M, 0, 3000 * M]);
    d.app.draw(&mut d.frame);
    let w = &d.app.world;
    let cam = scene::camera(w, Eye::Chase);
    let px = |x: i32, y: i32| d.frame.0[(y as usize) * game_kit::W + x as usize];
    let (x, y) = scene::on_screen(&VIEW, &cam, w.ships[i].unwrap().pos).unwrap();
    assert_eq!(px(x - 9, y), ACCENT, "in reach: closed amber box");
    let (x, y) = scene::on_screen(&VIEW, &cam, w.ships[o].unwrap().pos).unwrap();
    assert_ne!(px(x - 9, y), ACCENT, "out of reach: corners only");
    assert!(named(&d, Eye::Chase).is_some());
    shot(&mut d, "chk-chase-view");
}

#[test]
fn the_distance_text_at_its_edges() {
    let mut d = flying();
    let cases: [(V3, &str); 9] = [
        ([0, 0, 0], "RAIDER 0 M  IN RANGE"),
        ([0, 0, 1], "RAIDER 0 M  IN RANGE"),
        ([0, 0, 999 * M + 255], "RAIDER 999 M  IN RANGE"),
        ([0, 0, 1000 * M], "RAIDER 1.0 KM  IN RANGE"),
        ([0, 0, 2400 * M], "RAIDER 2.4 KM  IN RANGE"),
        ([0, 0, 2499 * M], "RAIDER 2.4 KM"),
        ([0, 0, 12 * KM], "RAIDER 12.0 KM"),
        ([0, 0, 99_999 * M], "RAIDER 99.9 KM"),
        ([0, 0, 250 * KM], "RAIDER 250 KM"),
    ];
    for (off, want) in cases {
        d.app.world.ships = [None; world::SHIPS];
        let i = put(&mut d, Kind::Raider, off);
        let w = &d.app.world;
        let mut t = hud::Txt::new();
        hud::target_text(&mut t, w, &w.ships[i].unwrap());
        assert_eq!(t.as_str(), want);
        // The longest line fits the 160-pixel picture.
        assert!(game_kit::text_width(t.as_str()) <= 156, "{want}");
    }
    // The longest name with the longest shown distance (7 km is the
    // limit for being named): the line fits.
    d.app.world.ships = [None; world::SHIPS];
    let i = put(&mut d, Kind::Canister, [0, 0, 2399 * M]);
    let w = &d.app.world;
    let mut t = hud::Txt::new();
    hud::target_text(&mut t, w, &w.ships[i].unwrap());
    let px = game_kit::text_width(t.as_str());
    println!("longest line: {:?}, {px} px", t.as_str());
    assert!(px <= 156);
}
