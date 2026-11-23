//! LANDER played by script: whole games, every way to end a flight, the
//! menus and the pauses. No console needed: the tests drive the game's
//! own state (`App`) the way the console's calls do.

mod common;

use common::{PERIOD_30, Run, hold, press};
use game_kit::{Touch, button};
use lander::app::{
    App, BROKEN_WAIT, Ending, Event, Input, LANDED_WAIT, LIVES, MAX_CATCH_UP, MAX_START_LEVEL,
    Scene, Steer, View,
};
use lander::craft::{SAFE_DOWN, SAFE_LEAN, SAFE_SIDE, Why};
use lander::draw::{MENU_STEP, MENU_X, MENU_Y};
use lander::fixed::{PX, STEP};

/// A tap at picture pixel (x, y): down in one call, up in the next.
fn tap_at(r: &mut Run, x: i16, y: i16) {
    let down = Touch {
        x,
        y,
        down: true,
        started: true,
        ended: false,
    };
    r.call(&Input {
        touch: Some(down),
        ..Input::default()
    });
    let up = Touch {
        down: false,
        started: false,
        ended: true,
        ..down
    };
    r.call(&Input {
        touch: Some(up),
        ..Input::default()
    });
}

/// Starts level `n` and skips its "LEVEL" card.
fn start_at(n: u16) -> Run {
    let mut r = Run::new();
    r.app.opts.level = n;
    r.play();
    assert_eq!(r.app.scene, Scene::Fly);
    assert_eq!(r.app.level, n);
    r
}

/// Puts the craft `h` pixels over the middle of the level's widest pad,
/// moving at (vx, vy) pixels a second, leaning `lean` steps.
fn over_pad(r: &mut Run, h: i32, vx: i32, vy: i32, lean: i32) {
    let p = *r
        .app
        .world
        .pads()
        .iter()
        .max_by_key(|p| p.w)
        .expect("a pad");
    let c = &mut r.app.craft;
    c.x = (i32::from(p.x) * 2 + i32::from(p.w)) * PX / 2;
    c.y = (i32::from(p.y) - 1 - 4 - h) * PX + PX / 2;
    c.vx = vx * PX / 60;
    c.vy = vy * PX / 60;
    c.angle = lean * STEP;
}

/// Calls with `inp` until the flight ends (at most `max` calls).
fn until_down(r: &mut Run, inp: &Input, max: u32) {
    for _ in 0..max {
        if r.app.scene != Scene::Fly {
            return;
        }
        r.call(inp);
    }
    panic!("still flying after {max} calls");
}

// ---- Whole runs ----------------------------------------------------------

/// The autopilot lands the first nine levels one after another: one
/// screen wide, then two (with wind), then gusts and small pads. A full
/// successful run, as the README's "a full game" means it.
#[test]
fn the_autopilot_lands_level_after_level() {
    let mut r = Run::new();
    r.play();
    for level in 1..=9u16 {
        assert_eq!(r.app.level, level);
        let calls = r.autopilot(3000);
        assert_eq!(
            r.app.scene,
            Scene::Landed,
            "level {level}: {:?} after {calls} calls",
            r.app.ending
        );
        eprintln!(
            "level {level}: landed after {calls} calls ({} s), fuel left {}%, x{} pad, soft {}, points {}",
            calls / 30,
            r.app.tally.fuel,
            r.app.tally.mult,
            r.app.tally.soft,
            r.app.tally.points
        );
        // The card waits, then A calls up the next level.
        r.tap(button::A);
        assert_eq!(r.app.scene, Scene::Landed, "A is ignored at first");
        r.idle(LANDED_WAIT / 2);
        r.tap(button::A);
        assert_eq!(r.app.scene, Scene::Ready);
        while r.app.scene == Scene::Ready {
            r.call(&Input::default());
        }
    }
    assert_eq!(r.landed_count(), 9);
    assert_eq!(r.app.lives, LIVES);
    let sum: u32 = r
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Landed { points, .. } => Some(*points),
            _ => None,
        })
        .sum();
    assert_eq!(r.app.score, sum);
    assert!(r.app.score > 9 * 50);
}

/// Every level the title offers, flown alone by the same simple
/// autopilot. Not all are landed (it is a simple pilot); the count is
/// printed and kept from getting worse.
#[test]
fn most_levels_can_be_landed_by_a_simple_pilot() {
    let mut landed = Vec::new();
    let mut not = Vec::new();
    for n in 1..=MAX_START_LEVEL {
        let mut r = start_at(n);
        r.autopilot(4000);
        if r.app.scene == Scene::Landed {
            landed.push(n);
        } else {
            not.push((n, r.app.ending));
        }
    }
    eprintln!("landed: {landed:?}");
    eprintln!("not landed: {not:?}");
    assert!(
        landed.len() >= 15,
        "only {} of {MAX_START_LEVEL}",
        landed.len()
    );
}

// ---- Each way a flight ends badly -------------------------------------------

/// No input at all on level 1: the craft drifts sideways and falls.
#[test]
fn falling_without_the_engine_breaks_the_craft() {
    let mut r = start_at(1);
    until_down(&mut r, &Input::default(), 600);
    assert_eq!(r.app.scene, Scene::Broken);
    assert!(
        matches!(r.app.ending, Ending::Crash(Why::MissedPad | Why::TooFast)),
        "{:?}",
        r.app.ending
    );
    assert!(r.app.craft.touch_vy > SAFE_DOWN);
    assert_eq!(r.app.lives, LIVES - 1);
    // The pieces fly: they are on show at first.
    assert!(r.app.bits.live() > 10);
}

#[test]
fn coming_down_beside_the_pads_is_a_miss() {
    let mut r = start_at(1);
    // Slow and upright, but over rough ground left of the first pad.
    over_pad(&mut r, 3, 0, 2, 0);
    let p = *r
        .app
        .world
        .pads()
        .iter()
        .max_by_key(|p| p.w)
        .expect("a pad");
    let off = i32::from(p.w) / 2 + 6;
    r.app.craft.x -= off * PX;
    assert!(r.app.world.pad_at(r.app.craft.x / PX - 4).is_none());
    until_down(&mut r, &hold(button::B), 60);
    assert_eq!(r.app.ending, Ending::Crash(Why::MissedPad));
}

#[test]
fn a_fast_touchdown_on_a_pad_breaks_the_craft() {
    let mut r = start_at(1);
    // Over the pad, upright, engine off, from 20 pixels up.
    over_pad(&mut r, 20, 0, 0, 0);
    until_down(&mut r, &hold(button::B), 120);
    assert_eq!(r.app.ending, Ending::Crash(Why::TooFast));
    assert!(r.app.craft.touch_vy > SAFE_DOWN);
}

#[test]
fn a_leaning_touchdown_breaks_the_craft() {
    let mut r = start_at(1);
    over_pad(&mut r, 2, 0, 3, SAFE_LEAN / STEP + 4);
    until_down(&mut r, &Input::default(), 60);
    assert_eq!(r.app.ending, Ending::Crash(Why::Tilted));
}

#[test]
fn a_sliding_touchdown_breaks_the_craft() {
    let mut r = start_at(1);
    over_pad(&mut r, 2, 7, 3, 0);
    const { assert!(7 * PX / 60 > SAFE_SIDE) };
    until_down(&mut r, &hold(button::B), 60);
    assert_eq!(r.app.ending, Ending::Crash(Why::Sliding));
}

#[test]
fn flying_off_the_side_loses_the_craft() {
    let mut r = start_at(1);
    // Full engine, leaning hard to one side, until it leaves the world.
    let away = if r.app.craft.x / PX < 80 {
        button::LEFT
    } else {
        button::RIGHT
    };
    until_down(&mut r, &hold(button::A | away), 900);
    assert_eq!(r.app.scene, Scene::Broken);
    assert_eq!(r.app.ending, Ending::Lost);
    assert!(r.events.contains(&Event::Lost { level: 1 }));
    // Nothing breaks into pieces when it is lost.
    assert_eq!(r.app.bits.live(), 0);
}

#[test]
fn a_try_again_starts_the_same_level_afresh() {
    let mut r = start_at(2);
    until_down(&mut r, &Input::default(), 600);
    assert_eq!(r.app.scene, Scene::Broken);
    // A is ignored until the pieces have flown a while.
    r.tap(button::A);
    assert_eq!(r.app.scene, Scene::Broken);
    r.idle(BROKEN_WAIT / 2);
    // When the pieces lie still, the card just waits for the player.
    let mut waited = 0;
    while !r.app.still() {
        r.idle(1);
        waited += 1;
        assert!(waited < 200, "the pieces never settle");
    }
    eprintln!("pieces settled {waited} calls after the card could be closed");
    r.tap(button::A);
    assert_eq!(r.app.scene, Scene::Ready);
    assert_eq!(r.app.level, 2);
    assert_eq!(r.app.craft.fuel, r.app.world.params.fuel);
    assert_eq!(r.app.bits.live(), 0);
}

/// Three crashes end the game; the score stays as the best; any key goes
/// back to the title.
#[test]
fn three_crashes_end_the_game_and_keep_the_best() {
    let mut r = Run::new();
    r.play();
    // Land level 1 for some points first.
    r.autopilot(3000);
    assert_eq!(r.app.scene, Scene::Landed);
    let points = r.app.score;
    r.idle(LANDED_WAIT);
    r.tap(button::A);
    for life in (0..LIVES).rev() {
        while r.app.scene == Scene::Ready {
            r.call(&Input::default());
        }
        until_down(&mut r, &Input::default(), 900);
        assert_eq!(r.app.lives, life);
        if life > 0 {
            r.idle(BROKEN_WAIT / 2);
            r.tap(button::A);
        }
    }
    // No pause and no try-again with no craft left: START does nothing.
    r.tap(button::START);
    assert_eq!(r.app.pause, None);
    r.idle(BROKEN_WAIT / 2 + 2);
    assert_eq!(r.app.scene, Scene::Over);
    assert!(r.events.contains(&Event::Over {
        score: points,
        best: points
    }));
    assert_eq!(r.app.best, points);
    r.idle(BROKEN_WAIT / 6 + 1);
    for _ in 0..200 {
        if r.app.still() {
            break;
        }
        r.idle(1);
    }
    assert!(r.app.still(), "{} bits still move", r.app.bits.moving());
    r.tap(button::A);
    assert_eq!(r.app.scene, Scene::Title);
    assert_eq!(r.app.best, points);

    // A new game starts from nothing, with all craft.
    r.play();
    assert_eq!((r.app.score, r.app.lives, r.app.level), (0, LIVES, 1));
}

// ---- The game's own pause, and the console's ----------------------------------

#[test]
fn start_pauses_the_flight_and_nothing_moves() {
    let mut r = start_at(3);
    r.idle(10);
    r.tap(button::START);
    assert_eq!(r.app.pause, Some(0));
    assert!(r.app.still());
    let (craft, fuel, t) = (r.app.craft, r.app.craft.fuel, r.app.flight_t);
    r.idle(1);
    for _ in 0..60 {
        // Even with the engine and the stick held, nothing moves.
        let c = r.call(&hold(button::LEFT));
        assert_eq!(c.changed, 0, "the paused picture stays the same");
    }
    assert_eq!(r.app.craft, craft);
    assert_eq!((r.app.craft.fuel, r.app.flight_t), (fuel, t));
    // B closes it, and the flight goes on.
    r.tap(button::B);
    assert_eq!(r.app.pause, None);
    r.idle(5);
    assert_ne!(r.app.craft, craft);
}

#[test]
fn the_pause_menu_resumes_restarts_or_leaves() {
    let mut r = start_at(1);
    r.idle(20);
    let fuel_then = r.app.craft.fuel;

    // RESUME (the first line) with A.
    r.tap(button::START);
    r.tap(button::A);
    assert_eq!((r.app.pause, r.app.scene), (None, Scene::Fly));

    // RESTART LEVEL: down once, A. Same level, full fuel, the card again.
    r.call(&hold(button::A));
    r.call(&hold(button::A));
    assert!(r.app.craft.fuel < fuel_then);
    r.tap(button::START);
    r.tap(button::DOWN);
    assert_eq!(r.app.pause, Some(1));
    r.tap(button::A);
    assert_eq!(r.app.scene, Scene::Ready);
    assert_eq!(r.app.craft.fuel, r.app.world.params.fuel);
    assert_eq!(r.app.lives, LIVES, "a restart costs no craft");

    // The menu wraps: up from the first line is TITLE.
    r.tap(button::START);
    r.tap(button::UP);
    assert_eq!(r.app.pause, Some(2));
    r.tap(button::A);
    assert_eq!(r.app.scene, Scene::Title);
}

#[test]
fn the_pause_menu_answers_taps() {
    let mut r = start_at(1);
    r.tap(button::START);
    // Line 3, TITLE, at the middle of the box.
    let y = 34 + 20 + 2 * MENU_STEP;
    tap_at(&mut r, 80, y);
    assert_eq!(r.app.scene, Scene::Title);
    // A tap outside the box does nothing.
    let mut r = start_at(1);
    r.tap(button::START);
    tap_at(&mut r, 5, y);
    assert_eq!(r.app.pause, Some(0));
}

/// While the console's own menu is open the game is not called and its
/// time does not count. A long gap between calls (a slow card, a
/// debugger) costs at most a few ticks, never a jump.
#[test]
fn a_long_gap_between_calls_runs_only_a_few_ticks() {
    let mut r = start_at(1);
    r.idle(10);
    let before = r.app.flight_t;
    r.t += 10_000_000; // ten seconds missing
    r.call(&Input::default());
    assert_eq!(u64::from(r.app.flight_t - before), MAX_CATCH_UP);
    // Then on as normal: two ticks a call at 30 a second.
    let before = r.app.flight_t;
    r.call(&Input::default());
    assert_eq!(r.app.flight_t - before, 2);
}

#[test]
fn time_never_runs_backwards() {
    let mut r = start_at(1);
    r.idle(10);
    let before = r.app.craft;
    r.t = 0; // a clock that went back: no ticks until it catches up
    r.call(&Input::default());
    assert_eq!(r.app.craft, before);
}

// ---- The title menu ---------------------------------------------------------

#[test]
fn the_game_starts_at_level_1_still_stick_and_60_pictures_a_second() {
    let app = App::new();
    assert_eq!(app.scene, Scene::Title);
    assert_eq!(app.menu, 0);
    assert_eq!(app.opts.level, 1);
    assert_eq!(app.opts.view, View::Still);
    assert_eq!(app.opts.steer, Steer::Stick);
    assert!(app.opts.fast);
}

#[test]
fn the_title_menu_sets_every_option() {
    let mut r = Run::new();
    r.idle(3);
    assert_eq!(r.app.menu, 0);
    // LEVEL: right goes up, left from 1 wraps to the top.
    r.tap(button::DOWN);
    assert_eq!(r.app.menu, 1);
    r.tap(button::RIGHT);
    assert_eq!(r.app.opts.level, 2);
    r.tap(button::LEFT);
    r.tap(button::LEFT);
    assert_eq!(r.app.opts.level, MAX_START_LEVEL);
    r.tap(button::RIGHT);
    assert_eq!(r.app.opts.level, 1);
    // VIEW, STEER, RATE: left, right or A flip them.
    r.tap(button::DOWN);
    r.tap(button::A);
    assert_eq!(r.app.opts.view, View::Follow);
    r.tap(button::DOWN);
    r.tap(button::RIGHT);
    assert_eq!(r.app.opts.steer, Steer::Tilt);
    r.tap(button::DOWN);
    r.tap(button::LEFT);
    assert!(r.app.opts.fast);
    r.tap(button::LEFT);
    assert!(!r.app.opts.fast);
    // The menu wraps from the last line to the first.
    r.tap(button::DOWN);
    r.tap(button::DOWN);
    assert_eq!(r.app.menu, 0);
    // START plays from any line.
    r.tap(button::DOWN);
    r.tap(button::START);
    assert_eq!(r.app.scene, Scene::Ready);
    assert_eq!(r.app.opts.view, View::Follow);
}

#[test]
fn the_left_stick_steps_through_the_title_menu() {
    let mut r = Run::new();
    let stick = |x: i16, y: i16| Input {
        left: (x, y),
        ..Input::default()
    };
    // Held down: one step, not one a call.
    for _ in 0..10 {
        r.call(&stick(0, 30000));
    }
    assert_eq!(r.app.menu, 1);
    r.call(&stick(0, 0));
    r.call(&stick(0, 30000));
    assert_eq!(r.app.menu, 2);
    r.call(&stick(0, 0));
    r.call(&stick(0, -30000));
    assert_eq!(r.app.menu, 1);
    r.call(&stick(0, 0));
    r.call(&stick(30000, 0));
    assert_eq!(r.app.opts.level, 2);
    // A small push is not a step.
    r.call(&stick(0, 0));
    r.call(&stick(0, 9000));
    assert_eq!(r.app.menu, 1);
}

#[test]
fn the_title_menu_answers_taps() {
    let mut r = Run::new();
    let line = |k: i16| MENU_Y + k * MENU_STEP + 2;
    // A tap on LEVEL raises it.
    tap_at(&mut r, MENU_X + 10, line(1));
    assert_eq!((r.app.menu, r.app.opts.level), (1, 2));
    // A tap on the picture's right side (the craft) does nothing.
    tap_at(&mut r, 150, line(1));
    assert_eq!(r.app.opts.level, 2);
    // A tap on PLAY starts level 2; a tap skips the card.
    tap_at(&mut r, MENU_X + 10, line(0));
    assert_eq!((r.app.scene, r.app.level), (Scene::Ready, 2));
    tap_at(&mut r, 80, 60);
    assert_eq!(r.app.scene, Scene::Fly);
}

#[test]
fn end_on_the_title_leaves_the_game() {
    let mut r = Run::new();
    r.tap(button::UP);
    assert_eq!(r.app.menu, 5);
    r.call(&press(button::A));
    assert!(r.done);
    assert_eq!(r.events.last(), Some(&Event::Quit));
}

// ---- Calm screens --------------------------------------------------------------

#[test]
fn only_settled_cards_and_pauses_are_still() {
    let mut r = Run::new();
    assert!(!r.app.still(), "the title's flame flickers");
    r.play();
    assert!(!r.app.still());
    r.autopilot(3000);
    assert_eq!(r.app.scene, Scene::Landed);
    assert!(!r.app.still(), "dust is still settling");
    r.idle(LANDED_WAIT);
    assert!(r.app.still());
    // Still means still: no pixel changes from call to call.
    for _ in 0..10 {
        assert_eq!(r.call(&Input::default()).changed, 0);
    }
}

#[test]
fn the_game_runs_the_same_at_30_and_60_pictures_a_second() {
    // Physics is tied to game time, not to calls: after the same number
    // of physics ticks the craft is in the same place.
    let fly = |fast: bool| {
        let mut r = Run::new();
        r.app.opts.fast = fast;
        r.tap(button::A);
        assert_eq!(r.app.scene, Scene::Ready);
        // A skips the card; the same input from the first flight tick.
        r.call(&Input {
            pressed: button::A,
            ..hold(button::A | button::RIGHT)
        });
        assert_eq!(r.app.scene, Scene::Fly);
        let mut seen = std::collections::BTreeMap::new();
        seen.insert(r.app.flight_t, r.app.craft);
        while r.app.flight_t < 120 {
            r.call(&hold(button::A | button::RIGHT));
            seen.insert(r.app.flight_t, r.app.craft);
        }
        seen
    };
    let (slow, fast) = (fly(false), fly(true));
    let mut same = 0;
    for (t, c) in &slow {
        if let Some(d) = fast.get(t) {
            assert_eq!(c, d, "tick {t}");
            same += 1;
        }
    }
    assert!(same > 50);
    // The run helper's periods really are 1/30 s and 1/60 s.
    assert_eq!((PERIOD_30 * 30 + 500) / 1000, 1000);
}

#[test]
fn the_same_inputs_give_the_same_pictures() {
    let run = || {
        let mut r = Run::new();
        r.app.opts.view = View::Follow;
        r.play();
        r.autopilot(3000);
        r.idle(20);
        (r.print(), r.events.clone(), r.app.score)
    };
    let a = run();
    assert_eq!(a, run());
    // And a fresh App draws the same title.
    let (mut f1, mut f2) = (console_video::Frame::new(), console_video::Frame::new());
    App::new().draw(&mut f1);
    App::new().draw(&mut f2);
    assert!(f1.0 == f2.0);
}
