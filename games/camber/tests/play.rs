//! Whole races: the player's ship flown through the keys, the rivals on
//! their own, every track and class; and the same keys giving the same
//! race twice.

mod common;

use camber::app::{Log, Scene};
use camber::race::{Class, Ev, LAPS, Mode, Phase, Race, SHIPS};
use camber::scene::{Detail, ViewKind};
use camber::track::TRACKS;
use common::{CALL_30, CALL_60, Driver};
use game_kit::button;

/// What happened in a race run by the computer alone.
#[derive(Debug, Default)]
struct Tally {
    finish: [u32; SHIPS],
    walls: [u32; SHIPS],
    falls: [u32; SHIPS],
    min_shield: [i16; SHIPS],
    limp_ticks: [u32; SHIPS],
    boosts: [u32; SHIPS],
    ticks: u32,
}

fn run_alone(track: u8, class: Class, mode: Mode, seed: u32) -> Tally {
    let mut r = Race::new();
    r.setup(track, class, mode, seed);
    let mut t = Tally {
        min_shield: [i16::MAX; SHIPS],
        ..Tally::default()
    };
    let n = usize::from(r.n);
    // Three laps take well under 4 minutes at the slowest class.
    for _ in 0..60 * 240 {
        r.step(None);
        t.ticks += 1;
        while let Some(e) = r.take_ev() {
            match e {
                Ev::Wall { ship } => t.walls[usize::from(ship)] += 1,
                Ev::Fell { ship } => t.falls[usize::from(ship)] += 1,
                Ev::Boost { ship } => t.boosts[usize::from(ship)] += 1,
                _ => {}
            }
        }
        for i in 0..n {
            let sh = &r.ships[i];
            t.min_shield[i] = t.min_shield[i].min(sh.shield);
            if sh.limp {
                t.limp_ticks[i] += 1;
            }
            t.finish[i] = sh.finish;
        }
        if r.ships[..n].iter().all(|s| s.finished()) {
            break;
        }
    }
    t
}

#[test]
fn rivals_finish_every_track_at_every_class() {
    for track in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            for seed in [0, 3] {
                let t = run_alone(track, class, Mode::Race, seed);
                let name = TRACKS[usize::from(track)].name;
                eprintln!(
                    "{name} {} seed {seed}: finish {:?} walls {:?} falls {:?} min shield {:?} limp {:?} boosts {:?}",
                    class.name(),
                    t.finish,
                    t.walls,
                    t.falls,
                    t.min_shield,
                    t.limp_ticks,
                    t.boosts
                );
                for i in 0..SHIPS {
                    assert!(
                        t.finish[i] > 0,
                        "{name} {}: ship {i} did not finish",
                        class.name()
                    );
                }
            }
        }
    }
}

#[test]
fn a_time_trial_finishes_alone() {
    for track in 0..TRACKS.len() as u8 {
        let t = run_alone(track, Class::Wild, Mode::Trial, 0);
        assert!(t.finish[0] > 0);
        assert!(t.finish[1..].iter().all(|&f| f == 0));
    }
}

/// The player's ship flown through the keys by the autopilot, at both
/// rates, to the results and back to the title.
#[test]
fn a_race_through_the_keys_finishes_all_laps_on_every_track() {
    for fast in [false, true] {
        for track in 0..TRACKS.len() as u8 {
            let mut d = Driver::new();
            d.draw = false;
            d.set(Detail::Low, ViewKind::Chase, fast);
            d.race(track, Class::Brisk, Mode::Race);
            let mut calls = 0;
            while !d.app.results() {
                d.auto();
                calls += 1;
                assert!(calls < 60 * 300, "no end in five minutes");
            }
            let sh = &d.app.race.ships[0];
            assert_eq!(sh.lap, LAPS + 1);
            assert!(sh.best > 0 && sh.best * 3 <= sh.finish + 60);
            let laps = d
                .logs
                .iter()
                .filter(|l| matches!(l, Log::Lap { .. }))
                .count();
            assert_eq!(laps, LAPS as usize - 1, "{:?}", d.logs);
            assert!(d.logs.iter().any(|l| matches!(l, Log::Finish { .. })));
            eprintln!(
                "{} {}fps: place {} time {} best {} shield {}",
                TRACKS[usize::from(track)].name,
                if fast { 60 } else { 30 },
                d.app.race.place(0),
                sh.finish,
                sh.best,
                sh.shield
            );
            // Results: TITLE, then the title again.
            d.draw = true;
            d.tap(button::DOWN);
            d.tap(button::A);
            assert_eq!(d.app.scene, Scene::Title);
        }
    }
}

/// The same keys give the same race, call for call, and the same
/// pictures.
#[test]
fn the_same_keys_give_the_same_race() {
    let run = |call: u32| {
        let mut d = Driver::new();
        d.call_us = call;
        d.race(2, Class::Wild, Mode::Race);
        let mut h = 0u64;
        for k in 0..1_500u32 {
            // A pattern of keys that wanders: steer, thrust, carves (L,
            // R, both), the brake.
            let held = if k % 97 < 80 { button::A } else { 0 }
                | if k % 211 > 190 {
                    button::L | button::R
                } else if k % 131 > 90 {
                    button::L
                } else if k % 173 > 130 {
                    button::R
                } else if k % 389 > 370 {
                    button::B
                } else {
                    0
                };
            let stick = (((k * 37) % 400) as i16 - 200) * 160;
            d.hold(held, (stick, 0));
            for b in d.frame.0.iter().step_by(7) {
                h = h.wrapping_mul(1_000_003).wrapping_add(u64::from(*b));
            }
        }
        (d.app.race, h)
    };
    let (a, ha) = run(CALL_30);
    let (b, hb) = run(CALL_30);
    assert_eq!(a, b);
    assert_eq!(ha, hb);
    let (c, _) = run(CALL_60);
    let (e, _) = run(CALL_60);
    assert_eq!(c, e);
}

/// 30 and 60 calls a second run the same ticks: with the same keys held,
/// the race is the same, ship for ship, at the same tick.
#[test]
fn both_rates_run_the_same_race() {
    let start = |call: u32| {
        let mut d = Driver::new();
        d.draw = false;
        d.call_us = call;
        d.race(0, Class::Brisk, Mode::Race);
        d
    };
    let mut a = start(CALL_30);
    let mut b = start(CALL_60);
    // 60 s of game time at 30 calls a second, then 60 a second up to
    // the same tick.
    for _ in 0..60_000_000 / CALL_30 {
        a.hold(button::A, (0, 0));
    }
    while b.app.race.tick < a.app.race.tick {
        b.hold(button::A, (0, 0));
    }
    assert_eq!(a.app.race.phase, Phase::Run);
    assert_eq!(a.app.race, b.app.race);
}

/// Detail and view change only the picture: the same keys give the same
/// race at every setting, on every track.
#[test]
fn detail_and_view_never_change_the_race() {
    for t in 0..TRACKS.len() as u8 {
        let mut first: Option<Race> = None;
        for detail in Detail::ALL {
            for view in ViewKind::ALL {
                let mut d = Driver::new();
                d.set(detail, view, false);
                d.race(t, Class::Brisk, Mode::Race);
                for _ in 0..1_200 {
                    d.auto();
                }
                match &first {
                    None => first = Some(d.app.race),
                    Some(a) => {
                        assert_eq!(*a, d.app.race, "{} {} {}", t, detail.name(), view.name())
                    }
                }
            }
        }
    }
}
