//! Replays: the autopilot races the whole cup from the grid to the
//! finish card, with no help from the test, and must end in the top
//! three of every course (else the next course would not open). Then
//! the cup's end screen and the way back to the title. The same driver
//! also races with the d-pad only and with a finger only, to show both
//! are enough to finish.

mod common;

use common::{Driver, autopilot, plan};
use game_kit::button;
use hairpin::app::{CARD_WAIT, Event, Keys, Scene};
use hairpin::hud::{self, MENU_Y, STEER_DEAD, STEER_FULL};
use hairpin::race::{LAPS, Pad};
use hairpin::render::CX;
use hairpin::track::COURSES;

/// Calls allowed per race (30 a second): 6 minutes.
const MAX_CALLS: usize = 30 * 360;

#[test]
fn the_autopilot_wins_the_cup() {
    let mut d = Driver::new();
    d.race_course(0);
    for (c, course) in COURSES.iter().enumerate() {
        assert_eq!(d.app.scene, Scene::Race);
        assert_eq!(usize::from(d.app.race.course), c);
        let trip = autopilot(&mut d, MAX_CALLS);
        println!(
            "course {} {}: place {}, {} ticks ({} s), laps {:?}, best lap {}, crashes {}",
            c + 1,
            course.name,
            trip.place,
            trip.ticks,
            trip.ticks / 60,
            d.app.race.laps,
            d.app.race.best_lap,
            trip.crashes
        );
        assert!(trip.finished, "course {} not finished", c + 1);
        assert!(trip.place <= 3, "course {}: place {}", c + 1, trip.place);
        assert!(d.app.race.laps.iter().all(|&l| l > 0));
        // Every lap was logged, then the finish.
        let laps = d
            .events
            .iter()
            .filter(|e| matches!(e, Event::Lap { .. }))
            .count();
        assert_eq!(laps, (c + 1) * LAPS as usize);
        d.idle(CARD_WAIT / 2 + 2);
        d.tap(button::A);
    }
    assert_eq!(d.app.scene, Scene::Cup);
    assert!(d.events.iter().any(|e| matches!(e, Event::Cup { .. })));
    assert!(d.app.cup_places.iter().all(|&p| (1..=3).contains(&p)));
    assert!(d.app.best.iter().all(|&t| t > 0));
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Title);
}

/// Each course on its own, from the title; then how far behind the
/// rivals came in (how much room a human player has).
#[test]
fn every_course_can_be_raced_from_the_title() {
    for c in 0..COURSES.len() as u8 {
        let mut d = Driver::new();
        d.race_course(c);
        let trip = autopilot(&mut d, MAX_CALLS);
        assert!(trip.finished, "course {} not finished", c + 1);
        // Let the rivals finish too (the race goes on under the card).
        let r = &mut d.app.race;
        for _ in 0..60 * 120 {
            if r.karts[1..].iter().all(|k| k.finish > 0) {
                break;
            }
            r.tick(&Pad::default());
            while r.take_event().is_some() {}
        }
        let me = r.karts[0].finish;
        let gaps: Vec<i64> = r.karts[1..]
            .iter()
            .map(|k| i64::from(k.finish) - i64::from(me))
            .collect();
        println!(
            "course {}: autopilot {} ticks, place {}; rivals came in this many ticks later: {gaps:?}",
            c + 1,
            trip.ticks,
            trip.place
        );
        assert!(r.karts[1..].iter().all(|k| k.finish > 0), "rivals finish");
    }
}

/// The steering wish (-256..256) the autopilot put on the stick.
fn steer_of_keys(k: &Keys) -> i32 {
    Driver::new().app.controls(k).steer
}

/// Races the current course with keys made by `turn` from the
/// autopilot's; returns the place, or None if not finished.
fn race_with(d: &mut Driver, mut turn: impl FnMut(&mut Driver, Keys)) -> Option<u8> {
    for _ in 0..MAX_CALLS {
        if d.app.scene == Scene::Finish {
            return Some(d.app.race.finish_place);
        }
        let k = plan(&d.app);
        turn(d, k);
    }
    None
}

#[test]
fn the_d_pad_alone_finishes_a_course() {
    let mut d = Driver::new();
    d.race_course(0);
    // The d-pad steers fully or not at all: tap it as often as the
    // stick would have been pushed.
    let mut acc = 0;
    let place = race_with(&mut d, |d, k| {
        let s = steer_of_keys(&k);
        acc += s.abs();
        let mut held = k.held & (button::A | button::B | button::X);
        if acc >= 256 {
            acc -= 256;
            held |= if s < 0 { button::LEFT } else { button::RIGHT };
        }
        d.hold(held);
    });
    println!("d-pad only, course 1: place {place:?}");
    assert!(place.is_some(), "not finished");
}

#[test]
fn a_finger_alone_plays_from_the_title_to_the_finish() {
    let mut d = Driver::new();
    // PLAY by finger.
    d.touch(80, MENU_Y + 2);
    assert_eq!(d.app.scene, Scene::Race);
    let (brake, boost) = hud::boxes(d.app.opts.detail);
    let mut last: Option<(i16, i16)> = None;
    let place = race_with(&mut d, |d, k| {
        let s = steer_of_keys(&k);
        // Where the finger goes: the brake box, the boost box on a
        // straight, or the drive area as far from the middle as the
        // steering wants; lifted to coast.
        let want = if k.held & button::B != 0 {
            Some((brake.0 + 6, brake.1 + 6))
        } else if k.held & button::X != 0 && s.abs() < 24 {
            Some((boost.0 + 6, boost.1 + 6))
        } else if k.held & button::A != 0 {
            let a = if s == 0 {
                0
            } else {
                STEER_DEAD + 1 + s.abs() * (STEER_FULL - STEER_DEAD) / 256
            };
            Some(((CX + s.signum() * a).clamp(0, 159) as i16, 60))
        } else {
            None
        };
        match (want, last) {
            (Some((x, y)), was) => {
                d.finger(x, y, was.is_none());
            }
            (None, Some((x, y))) => {
                d.lift(x, y);
            }
            (None, None) => {
                d.call_keys(Keys::default());
            }
        }
        last = want;
    });
    println!("finger only, course 1: place {place:?}");
    assert!(place.is_some(), "not finished");
    // The finish card by finger: go on.
    if let Some((x, y)) = last {
        d.lift(x, y);
    }
    d.idle(CARD_WAIT / 2 + 2);
    d.touch(80, 76 + 2);
    assert_eq!(d.app.scene, Scene::Race);
}
