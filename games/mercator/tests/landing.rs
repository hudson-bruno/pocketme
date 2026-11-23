//! Step G2: MERCATOR's way down and flown landing (SPEC 3.3, 3.5, 3.7):
//! the touchdown table at its edges, the sink assist, the lift by ship
//! and load, the lift-off's fuel, the loss below, the entry and the
//! take-off, and a scripted pilot over many seeds of every landable
//! look. The pilot measures the rules, not a human's skill.
//!
//! `LANDING_SEEDS` sets the pilot's seeds per look (default 100).

mod common;

use common::below::{below_in, docked_at, fly_call, pilot, system_with_look};
use common::{Driver, save_frame, shots};
use game_kit::button;
use mercator::app::{Event, Scene};
use mercator::market::Hold;
use mercator::planet::lander::{self, FINE, Land, MPS, Phase, TO_WORLD};
use mercator::planet::tune::{
    CRASH_HULL, ENTRY_SKIP, ENTRY_TICKS, HARD_HULL, LIFT, LIFT_FUEL, LIFT_HOLD, LIFT_MIN,
    TAKEOFF_TICKS, TIP_HULL,
};
use mercator::planet::{self, Below, Outcome, entry, place, site};
use mercator::world::{CRAFTS, M};

/// The looks with a surface (all but the giant).
const SURFACES: [u8; 7] = [0, 1, 2, 3, 5, 6, 7];

// ---- The touchdown table ------------------------------------------------------------

#[test]
fn the_touchdown_table_at_its_edges() {
    let t = |v: f64, s: f64| {
        lander::touchdown(
            (v * 1000.0f64).round() as i32,
            (s * 1000.0f64).round() as i32,
            0,
        )
    };
    // Down, with nothing sideways.
    assert_eq!(t(0.99, 0.0), (Land::Greased, 0));
    assert_eq!(t(1.01, 0.0), (Land::Landed, 0));
    assert_eq!(t(3.99, 0.0), (Land::Landed, 0));
    let (l, cost) = t(4.01, 0.0);
    assert_eq!(l, Land::Hard);
    assert_eq!(cost, 1, "5 a m/s over 4, at least 1");
    let (l, cost) = t(7.99, 0.0);
    assert_eq!(l, Land::Hard);
    assert_eq!(i32::from(cost), (399 * HARD_HULL + 99) / 100);
    let (l, cost) = t(8.01, 0.0);
    assert_eq!(l, Land::Crash);
    assert_eq!(i32::from(cost), (801 * CRASH_HULL + 99) / 100);
    // Sideways, at a slow touch down.
    assert_eq!(t(0.5, 0.99), (Land::Greased, 0));
    assert_eq!(t(0.5, 2.99), (Land::Landed, 0));
    assert_eq!(t(0.5, 3.01).0, Land::Hard);
    assert_eq!(t(0.5, 5.99).0, Land::Hard);
    assert_eq!(t(0.5, 6.01).0, Land::Crash);
    // Only down and sideways both under 1 m/s grease it.
    assert_eq!(t(0.99, 1.01).0, Land::Landed);
    // Each result's notes and whether the ship stays down.
    for (l, down) in [
        (Land::Greased, true),
        (Land::Landed, true),
        (Land::Hard, true),
        (Land::Tipped, false),
        (Land::Crash, false),
    ] {
        assert_eq!(l.down(), down);
        assert!(!l.note().is_empty());
    }
    assert_eq!(Land::Greased.note(), "GREASED IT: +20 S");
}

/// Four feet 6 m apart on ground rising `deg` degrees to the right.
fn slope_feet(deg: f64) -> [i32; 4] {
    let rise = (6.0 * f64::from(M) * deg.to_radians().tan()).round() as i32;
    let h = rise / 2;
    // Right-front, left-front, right-back, left-back.
    [h, -h, h, -h]
}

#[test]
fn the_slope_limit_is_twelve_degrees() {
    for (deg, tipped) in [(0.0, false), (11.9, false), (12.1, true), (20.0, true)] {
        let tan2 = lander::tan2(slope_feet(deg));
        let (l, cost) = lander::touchdown(500, 0, tan2);
        assert_eq!(l == Land::Tipped, tipped, "{deg} degrees");
        if tipped {
            assert_eq!(cost, TIP_HULL);
        }
    }
    // A crash wins over the slope.
    let (l, _) = lander::touchdown(9000, 0, lander::tan2(slope_feet(30.0)));
    assert_eq!(l, Land::Crash);
}

// ---- A lander on its own ------------------------------------------------------------

/// A site of `look` from `seed`, and a lander ready at its entry: ship,
/// tonnes, gravity (1/100 m/s^2).
fn lander_at(look: u8, seed: u32, ship: u8, tonnes: u32, gc: i32) -> Below {
    let mut s = site::make(seed, look).expect("a site");
    let places = place::place(&mut s, (seed % 4) as u8);
    let mut b = Below {
        on: true,
        site: s,
        places,
        axes: [[16384, 0, 0], [0, 16384, 0], [0, 0, 16384]],
        sun: [0, 11_585, 11_585],
        station: [0, 16384, 0],
        ..Below::EMPTY
    };
    let hull = CRAFTS[usize::from(ship)].hull;
    planet::ready(&mut b, ship, tonnes, gc, (hull, hull), 10);
    b
}

/// The same lander already flying, `up` m over the pad's middle.
fn flying_over_pad(ship: u8, tonnes: u32, gc: i32, up: i32) -> Below {
    let mut b = lander_at(0, 7, ship, tonnes, gc);
    b.phase = Phase::Fly;
    b.phase_t = 0;
    b.p = [0, (b.site.pad + up * M) << TO_WORLD, 0];
    b.v = [0; 3];
    b
}

fn ms(v: i32) -> f64 {
    f64::from(v) / f64::from(MPS)
}

/// With no input the assist holds the speed down at alt / 5 within 2..15
/// m/s (kept under what the jets can stop), and brings every ship down
/// at LANDED or better, light or full, at the lowest and the highest
/// gravity.
#[test]
fn the_assist_holds_the_sink_rate() {
    let mut rows = String::new();
    for ship in 0..3u8 {
        let full = u32::from(CRAFTS[usize::from(ship)].hold);
        for tonnes in [0, full] {
            for gc in [400, 650, 900] {
                let mut b = flying_over_pad(ship, tonnes, gc, 120);
                let mut worst = 0.0f64;
                let mut landed = None;
                for n in 0..4000 {
                    let thrust = b.g * i32::from(b.k) / 100;
                    let want = -lander::sink(lander::alt(&b), thrust - b.g);
                    if n > 90 && lander::alt(&b) > 3 * FINE {
                        // Faster down than the assist's speed.
                        worst = worst.max(ms(want - b.v[1]));
                    }
                    let out = planet::tick(&mut b, &Default::default());
                    if out == Outcome::Stay && b.phase == Phase::Landed {
                        landed = b.touched;
                        break;
                    }
                }
                let l = landed.expect("down");
                rows += &format!(
                    "ship {ship} {tonnes:2} t g {gc:3}: k {} worst {worst:.2} m/s over, {l:?}, {} s\n",
                    b.k,
                    b.t / 60
                );
                assert!(
                    matches!(l, Land::Landed | Land::Greased),
                    "ship {ship} {tonnes} t g {gc}: {l:?}"
                );
                assert!(
                    worst < 1.5,
                    "ship {ship} {tonnes} t g {gc}: {worst:.2} m/s over"
                );
            }
        }
    }
    println!("{rows}");
}

/// HOLD keeps the height.
#[test]
fn the_hold_mode_keeps_the_height() {
    for ship in 0..3u8 {
        let mut b = flying_over_pad(ship, 0, 650, 50);
        b.assist = lander::ASSIST_HOLD;
        for _ in 0..120 {
            planet::tick(&mut b, &Default::default());
        }
        let h0 = lander::alt(&b);
        for _ in 0..300 {
            planet::tick(&mut b, &Default::default());
            assert!(ms(b.v[1]).abs() < 0.2);
        }
        let moved = f64::from((lander::alt(&b) - h0).abs()) / f64::from(FINE);
        assert!(moved < 1.0, "ship {ship}: {moved:.2} m in 5 s");
        assert!(!b.hot, "hovering 7 s does not overheat");
    }
}

/// Y switches the assist off (the ship falls) and on again.
#[test]
fn y_switches_the_assist() {
    let mut b = flying_over_pad(0, 0, 650, 100);
    let y = lander::In {
        held: button::Y,
        ..Default::default()
    };
    planet::tick(&mut b, &y);
    assert_eq!(b.assist, lander::ASSIST_OFF);
    for _ in 0..60 {
        planet::tick(&mut b, &Default::default());
    }
    assert!(ms(b.v[1]) < -5.0, "falling freely: {:.1}", ms(b.v[1]));
    planet::tick(&mut b, &y);
    assert_eq!(b.assist, lander::ASSIST_SINK);
}

/// A tap shorter than a picture (noted by the console, never held)
/// counts, and once, though the call runs two ticks with its keys.
#[test]
fn a_tap_between_pictures_counts_once() {
    let mut b = flying_over_pad(0, 0, 650, 100);
    let tap = lander::In {
        pressed: button::Y,
        ..Default::default()
    };
    // One call, two ticks: switched off once.
    planet::tick(&mut b, &tap);
    planet::tick(&mut b, &tap);
    assert_eq!(b.assist, lander::ASSIST_OFF);
    // The next call has no keys; the call after it taps again: on.
    planet::tick(&mut b, &Default::default());
    planet::tick(&mut b, &tap);
    planet::tick(&mut b, &tap);
    assert_eq!(b.assist, lander::ASSIST_SINK);
    // A press held over several calls (after a call with no keys)
    // counts once too.
    planet::tick(&mut b, &Default::default());
    let held = lander::In {
        held: button::Y,
        pressed: button::Y,
        ..Default::default()
    };
    planet::tick(&mut b, &held);
    let still = lander::In {
        held: button::Y,
        ..Default::default()
    };
    for _ in 0..10 {
        planet::tick(&mut b, &still);
    }
    assert_eq!(b.assist, lander::ASSIST_OFF);
}

/// Heavier lifts worse for each ship; every ship lifts with a full hold
/// at every gravity the planets have (4.0 to 9.0 m/s^2).
#[test]
fn a_full_hold_lifts_worse_but_every_ship_lifts() {
    let mut rows = String::new();
    for ship in 0..3u8 {
        let full = u32::from(CRAFTS[usize::from(ship)].hold);
        let (empty_k, full_k) = (lander::lift_k(ship, 0), lander::lift_k(ship, full));
        assert_eq!(empty_k, LIFT[usize::from(ship)]);
        assert!(full_k < empty_k, "ship {ship}");
        assert!(full_k >= LIFT_MIN && LIFT_MIN > 100);
        for gc in (400..=900).step_by(50) {
            let mut climb = [0.0; 2];
            for (i, tonnes) in [0, full].into_iter().enumerate() {
                let mut b = flying_over_pad(ship, tonnes, gc, 2);
                b.assist = lander::ASSIST_OFF;
                let up = lander::In {
                    lift: 256,
                    ..Default::default()
                };
                let h0 = lander::alt(&b);
                for _ in 0..120 {
                    planet::tick(&mut b, &up);
                }
                climb[i] = f64::from(lander::alt(&b) - h0) / f64::from(FINE);
                assert!(
                    climb[i] > 0.5,
                    "ship {ship} {tonnes} t g {gc}: {:.2} m",
                    climb[i]
                );
                assert!(b.v[1] > 0);
            }
            assert!(climb[1] < climb[0], "ship {ship} g {gc}: full climbs less");
            if gc % 250 == 150 {
                rows += &format!(
                    "ship {ship} g {gc}: k {empty_k}/{full_k}, 2 s full lift climbs {:.1} m empty, {:.1} m full\n",
                    climb[0], climb[1]
                );
            }
        }
    }
    println!("{rows}");
}

/// Landed with less than 0.3 LY: B held is refused with a note, and the
/// ship stays; with the fuel, the take-off.
#[test]
fn the_lift_off_needs_its_fuel() {
    for (fuel, goes) in [(LIFT_FUEL - 1, false), (LIFT_FUEL, true)] {
        let mut b = flying_over_pad(0, 0, 650, 0);
        b.phase = Phase::Landed;
        b.fuel = fuel;
        let hold = lander::In {
            held: button::B,
            ..Default::default()
        };
        for _ in 0..LIFT_HOLD {
            planet::tick(&mut b, &hold);
        }
        if goes {
            assert_eq!(b.phase, Phase::TakeOff);
        } else {
            assert_eq!(b.phase, Phase::Landed);
            assert_eq!(b.note, Some("NO FUEL TO LIFT OFF: 0.3 LY"));
        }
    }
}

/// A finger kept on the JETS box lifts off the ground like B, and a tap
/// on it switches HOLD and SINK in flight.
#[test]
fn the_jets_box_takes_a_finger() {
    let mut b = flying_over_pad(0, 0, 650, 0);
    b.phase = Phase::Landed;
    let finger = lander::In {
        finger: Some((140, 110)),
        ..Default::default()
    };
    for _ in 0..u32::from(LIFT_HOLD) + 20 {
        planet::tick(&mut b, &finger);
    }
    assert_eq!(b.phase, Phase::TakeOff);
    let mut b = flying_over_pad(0, 0, 650, 50);
    planet::tick(&mut b, &finger);
    planet::tick(&mut b, &Default::default());
    assert_eq!(b.assist, lander::ASSIST_HOLD);
    planet::tick(&mut b, &finger);
    planet::tick(&mut b, &Default::default());
    assert_eq!(b.assist, lander::ASSIST_SINK);
}

/// A hard landing costs hull; a crash bounces the ship off the ground.
#[test]
fn hard_landings_cost_hull_and_crashes_bounce() {
    for (down, want) in [(6.0, Land::Hard), (12.0, Land::Crash)] {
        let mut b = flying_over_pad(2, 0, 650, 0);
        b.p[1] += FINE / 4;
        b.v[1] = -(down * f64::from(MPS)) as i32;
        b.assist = lander::ASSIST_OFF;
        let before = b.hull;
        for _ in 0..30 {
            planet::tick(&mut b, &Default::default());
            if b.touched.is_some() {
                break;
            }
        }
        assert_eq!(b.touched, Some(want));
        assert!(b.hull < before);
        if want == Land::Crash {
            assert_eq!(b.phase, Phase::Fly);
            assert!(b.v[1] > 0, "a bounce");
        } else {
            assert_eq!(b.phase, Phase::Landed);
        }
    }
}

// ---- The entry and the take-off -----------------------------------------------------

/// The entry: black sky to full bands, streaks only in its first 2 s,
/// the fog's reach growing; A skips after 1 s, not before. The take-off:
/// 60 m up, the sky black again at its end.
#[test]
fn the_entry_and_the_take_off_run_their_timelines() {
    let mut b = lander_at(3, 11, 0, 0, 650);
    assert_eq!(b.phase, Phase::Entry);
    assert_eq!(entry::bands_lit(&b), 0);
    assert_eq!(entry::fog(&b), 0);
    let a = lander::In {
        held: button::A,
        ..Default::default()
    };
    // A too early does nothing.
    planet::tick(&mut b, &a);
    planet::tick(&mut b, &Default::default());
    assert_eq!(b.phase, Phase::Entry);
    for _ in 2..ENTRY_SKIP {
        planet::tick(&mut b, &Default::default());
    }
    let mid = entry::bands_lit(&b);
    assert!(mid > 0 && mid < 10);
    planet::tick(&mut b, &a);
    assert_eq!(b.phase, Phase::Fly, "skipped");
    assert_eq!(entry::bands_lit(&b), 10);
    assert_eq!(entry::fog(&b), 256);
    // The skip ends where the full entry ends: the landing start.
    let start = b.places.start.map(|v| v << TO_WORLD);
    let at = b.p;
    for k in 0..3 {
        assert!((at[k] - start[k]).abs() < FINE, "the landing start");
    }
    // A full entry ends there too, at rest.
    let mut b2 = lander_at(3, 11, 0, 0, 650);
    for _ in 0..ENTRY_TICKS {
        planet::tick(&mut b2, &Default::default());
    }
    assert_eq!(b2.phase, Phase::Fly);
    for (p, s) in b2.p.iter().zip(start) {
        assert!((p - s).abs() < FINE);
    }
    assert_eq!(b2.v, [0; 3], "at rest at the landing start");
    // The take-off.
    let mut b = flying_over_pad(0, 0, 650, 0);
    b.phase = Phase::Landed;
    let hold = lander::In {
        held: button::B,
        ..Default::default()
    };
    for _ in 0..LIFT_HOLD {
        planet::tick(&mut b, &hold);
    }
    assert_eq!(b.phase, Phase::TakeOff);
    let base = b.p[1];
    let mut out = Outcome::Stay;
    for _ in 0..TAKEOFF_TICKS + 2 {
        out = planet::tick(&mut b, &Default::default());
        if out != Outcome::Stay {
            break;
        }
    }
    assert_eq!(out, Outcome::Lifted);
    assert_eq!((b.p[1] - base) / FINE, 60);
    assert_eq!(entry::bands_lit(&b), 0);
}

// ---- The loss below -----------------------------------------------------------------

/// A docked career with cargo and an unpaid TAG.
fn loaded(d: &mut Driver) {
    let mut c = d.app.world.car;
    c.hold = Hold([0; mercator::market::GOODS]);
    c.hold.0[0] = 6;
    c.tags = 1;
    d.app.world.car = c;
    d.app.last_dock = c;
}

/// The hull gone on the ground ends as a loss in space ends: the LOST
/// card, then A back at the last station without the cargo and the
/// unpaid TAGs.
#[test]
fn a_loss_below_is_a_loss_in_space() {
    let (g, id) = system_with_look(1);
    // Below.
    let mut d = docked_at(g, id);
    loaded(&mut d);
    common::below::dive(&mut d, 30 * M);
    d.idle(u32::from(ENTRY_TICKS));
    let b = &mut d.app.below;
    b.p = [0, (b.site.pad + 20 * M) << TO_WORLD, 0];
    b.v = [0, -15 * MPS, 0];
    b.assist = lander::ASSIST_OFF;
    b.hull = 5;
    let mut lost = false;
    for _ in 0..200 {
        d.idle(1);
        if d.app.scene == Scene::Lost {
            lost = true;
            break;
        }
    }
    assert!(lost, "the crash loses the ship");
    assert!(
        d.events.iter().any(|e| matches!(e, Event::Lost { .. })),
        "the loss is told as in space"
    );
    assert_eq!(d.app.world.car.hull, 0);
    // The card shows over the planet's last picture.
    assert!(d.app.below.on);
    d.wait_card();
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Docked);
    assert!(!d.app.below.on);
    let below_car = d.app.world.car;
    // In space.
    let mut s = docked_at(g, id);
    loaded(&mut s);
    s.app.sel = 0;
    s.tap(button::A);
    assert_eq!(s.app.scene, Scene::Flight);
    s.app.world.car.hull = 0;
    s.app.world.lose();
    for _ in 0..10 {
        s.idle(1);
    }
    assert_eq!(s.app.scene, Scene::Lost);
    s.wait_card();
    s.tap(button::A);
    assert_eq!(s.app.scene, Scene::Docked);
    assert_eq!(below_car, s.app.world.car, "the same career after both");
    assert_eq!(below_car.hold.total(), 0);
    assert_eq!(below_car.tags, 0);
    assert_eq!(below_car.system, id);
}

// ---- The scripted pilot over many seeds ---------------------------------------------

fn seeds() -> u32 {
    std::env::var("LANDING_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
}

/// The scripted pilot (assist on, fly to the pad, a touch of lift in the
/// last 3 m) lands softly (LANDED or GREASED IT, no hull lost) on every
/// seed of every landable look, in every ship, empty to full, at
/// gravities over the whole range.
#[test]
fn the_scripted_pilot_lands_softly_everywhere() {
    let n = seeds();
    let mut rows = String::new();
    let (mut greased, mut all, mut worst_s, mut hot_max) = (0, 0, 0u32, 0i32);
    let mut secs = Vec::new();
    for &look in SURFACES.iter() {
        for i in 0..n {
            let seed = i.wrapping_mul(0x9E37_79B9) ^ u32::from(look) << 24 ^ 0x5EED;
            let ship = (i % 3) as u8;
            let full = u32::from(CRAFTS[usize::from(ship)].hold);
            let tonnes = (i * 7) % (full + 1);
            let gc = 400 + (i as i32 * 37) % 501;
            let mut b = lander_at(look, seed, ship, tonnes, gc);
            let hull = b.hull;
            let mut ticks = 0u32;
            while b.phase != Phase::Landed {
                let input = pilot(&b);
                let out = planet::tick(&mut b, &input);
                assert_eq!(out, Outcome::Stay, "look {look} seed {i}");
                hot_max = hot_max.max(b.heat);
                assert!(!b.hot, "look {look} seed {i}: jets hot");
                ticks += 1;
                assert!(ticks < 120 * 60, "look {look} seed {i}: not down in 2 min");
            }
            let l = b.touched.unwrap();
            assert!(
                matches!(l, Land::Landed | Land::Greased),
                "look {look} seed {i} ship {ship} {tonnes} t g {gc}: {l:?}"
            );
            assert_eq!(b.hull, hull);
            let at = lander::at(&b);
            let off = ((f64::from(at[0]).powi(2) + f64::from(at[2]).powi(2)).sqrt() / f64::from(M))
                as u32;
            worst_s = worst_s.max(off);
            all += 1;
            greased += u32::from(l == Land::Greased);
            secs.push(ticks - u32::from(ENTRY_TICKS));
        }
    }
    secs.sort_unstable();
    let s = |t: u32| f64::from(t) / 60.0;
    rows += &format!(
        "{all} landings ({n} seeds x 7 looks): all soft, {greased} greased; flown time median {:.1} s, longest {:.1} s; farthest from the pad's middle {worst_s} m; most heat {}\n",
        s(secs[secs.len() / 2]),
        s(*secs.last().unwrap()),
        hot_max / lander::HEAT_UNIT / 1000
    );
    println!("{rows}");
}

// ---- The pictures -------------------------------------------------------------------

/// Pictures of the way down for four looks (git-ignored, under
/// `.local/mercator/shots/planet-g2/`): the entry, 120 m up, 20 m up
/// with the shadow, the touchdown, LANDED, the take-off.
#[test]
fn pictures_of_the_way_down() {
    let dir = shots().join("planet-g2");
    std::fs::create_dir_all(&dir).unwrap();
    for look in [0u8, 3, 5, 7] {
        let name = mercator::art::LOOK_NAMES[usize::from(look)].to_ascii_lowercase();
        let mut d = below_in(look);
        let save = |d: &mut Driver, what: &str| {
            let c = d.app.colours().to_vec();
            save_frame(&dir.join(format!("{name}-{what}.png")), &d.frame, &c, 3);
        };
        d.idle(40);
        save(&mut d, "1-entry");
        d.idle(u32::from(ENTRY_TICKS) - 40 + 2);
        save(&mut d, "2-120m");
        let (mut seen20, mut seen_down) = (false, false);
        for _ in 0..8000 {
            fly_call(&mut d);
            let alt = lander::alt(&d.app.below) / FINE;
            if !seen20 && alt <= 20 && d.app.below.phase == Phase::Fly {
                save(&mut d, "3-20m");
                seen20 = true;
            }
            if d.app.below.phase == Phase::Landed {
                save(&mut d, "4-touchdown");
                seen_down = true;
                break;
            }
        }
        assert!(seen20 && seen_down, "look {look}");
        d.idle(60);
        save(&mut d, "5-landed");
        for _ in 0..LIFT_HOLD {
            d.hold(button::B);
        }
        d.idle(100);
        save(&mut d, "6-takeoff");
    }
}
