//! The controls: the title menu, the pause menu and the results by keys
//! and by touch, the race controls (stick, pad, triggers, touch), and a
//! whole race flown by touch alone on every track.

mod common;

use camber::app::{
    Backdrop, COL_W, END_Y, Keys, LINE, Log, MAIN_LINES, OPT_LINES, PAUSE_BOX, PAUSE_LINES,
    PAUSE_Y, Page, ROW, ROWS_Y, Scene, control, line_at, row_at,
};
use camber::race::{Carve, CatchUp, Class, LAPS, Mode, Phase};
use camber::scene::{Detail, ViewKind};
use camber::track::TRACKS;
use common::Driver;
use game_kit::{Touch, button};

fn touch(x: i16, y: i16, started: bool) -> Keys {
    Keys {
        touch: Some(Touch {
            x,
            y,
            down: true,
            started,
            ended: false,
        }),
        ..Keys::default()
    }
}

/// A tap: a touch that starts, then the finger lifted.
fn tap_at(d: &mut Driver, x: i16, y: i16) -> bool {
    let going = d.call(touch(x, y, true));
    d.call(Keys::default());
    going
}

/// The y of the middle of menu line `k`.
fn line_y(top: i16, k: u8) -> i16 {
    top + i16::from(k) * LINE + 2
}

/// The y of the middle of the title's row `k`.
fn row_y(k: u8) -> i16 {
    ROWS_Y + i16::from(k) * ROW + ROW / 2
}

/// The x of a tap on the title's column.
const COL_X: i16 = 30;

#[test]
fn menu_lines_are_found_where_they_are_drawn() {
    for lines in [MAIN_LINES, OPT_LINES] {
        for k in 0..lines {
            assert_eq!(row_at(COL_X, row_y(k), lines), Some(k));
            // The whole row, from its top to its last line.
            assert_eq!(row_at(0, ROWS_Y + i16::from(k) * ROW, lines), Some(k));
            assert_eq!(
                row_at(COL_W - 1, ROWS_Y + i16::from(k + 1) * ROW - 1, lines),
                Some(k)
            );
        }
        assert_eq!(row_at(COL_X, ROWS_Y - 1, lines), None);
        assert_eq!(row_at(COL_X, ROWS_Y + i16::from(lines) * ROW, lines), None);
        // Right of the column is the backdrop.
        assert_eq!(row_at(COL_W, row_y(0), lines), None);
        // The last row still ends on the screen, above the best lap.
        assert!(ROWS_Y + i16::from(lines) * ROW <= 109);
    }
    // The column leaves more than half the picture to the backdrop.
    const { assert!(COL_W * 100 / 160 <= 45) };
    // The pause menu's lines.
    for k in 0..PAUSE_LINES {
        assert_eq!(line_at(line_y(PAUSE_Y, k), PAUSE_Y, PAUSE_LINES), Some(k));
    }
    assert_eq!(line_at(PAUSE_Y - 10, PAUSE_Y, PAUSE_LINES), None);
}

#[test]
fn the_title_menu_works_by_keys() {
    let mut d = Driver::new();
    assert_eq!(d.app.scene, Scene::Title);
    assert_eq!(d.app.page, Page::Main);
    // TRACK: right, right, left.
    d.tap(button::DOWN);
    assert_eq!(d.app.sel, 1);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.track, 1);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.track, 2);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.track, 1);
    // The made-up race behind the menu follows the track and class.
    assert_eq!(d.app.race.track, 1);
    // CLASS, MODE: every value comes round again.
    d.tap(button::DOWN);
    let c0 = d.app.opts.class;
    d.tap(button::RIGHT);
    assert_eq!(d.app.race.class, d.app.opts.class);
    for _ in 0..2 {
        d.tap(button::RIGHT);
    }
    assert_eq!(d.app.opts.class, c0);
    d.tap(button::DOWN);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.mode, Mode::Trial);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.mode, Mode::Race);
    // OPTIONS: A opens the page; left and right do nothing there.
    d.tap(button::DOWN);
    assert_eq!(d.app.sel, 4);
    d.tap(button::RIGHT);
    assert_eq!(d.app.page, Page::Main);
    d.tap(button::A);
    assert_eq!(d.app.page, Page::Options);
    assert_eq!(d.app.sel, 0);
    // DETAIL, RATE, VIEW.
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.detail, Detail::Low);
    d.tap(button::DOWN);
    d.tap(button::A);
    assert!(!d.app.opts.fast);
    d.tap(button::A);
    assert!(d.app.opts.fast);
    d.tap(button::DOWN);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.view, ViewKind::Nose);
    // CATCH-UP: LIGHT unless picked OFF; LEFT and RIGHT switch it.
    d.tap(button::DOWN);
    assert_eq!(d.app.opts.catchup, CatchUp::Light);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.catchup, CatchUp::Off);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.catchup, CatchUp::Light);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.catchup, CatchUp::Off);
    // BACKDROP: RACE unless picked SHIP; the backdrop follows at once.
    d.tap(button::DOWN);
    assert_eq!(d.app.opts.backdrop, Backdrop::Race);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.backdrop, Backdrop::Ship);
    assert_eq!((d.app.race.mode, d.app.race.n), (Mode::Trial, 1));
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.backdrop, Backdrop::Race);
    assert_eq!(d.app.race.mode, Mode::Race);
    d.tap(button::A);
    assert_eq!(d.app.opts.backdrop, Backdrop::Ship);
    // BACK, then round: DOWN from BACK is DETAIL, UP from DETAIL is BACK.
    d.tap(button::DOWN);
    assert_eq!(d.app.sel, OPT_LINES - 1);
    d.tap(button::DOWN);
    assert_eq!(d.app.sel, 0);
    d.tap(button::UP);
    assert_eq!(d.app.sel, OPT_LINES - 1);
    // A on BACK: the main page, on OPTIONS.
    d.tap(button::A);
    assert_eq!(d.app.page, Page::Main);
    assert_eq!(d.app.sel, 4);
    // B on OPTIONS goes back too; on the main page it does nothing.
    d.tap(button::A);
    assert_eq!(d.app.page, Page::Options);
    d.tap(button::B);
    assert_eq!(d.app.page, Page::Main);
    assert_eq!(d.app.sel, 4);
    d.tap(button::B);
    assert_eq!(d.app.page, Page::Main);
    assert_eq!(d.app.scene, Scene::Title);
    // UP from the top goes round to LEAVE; DOWN back to PLAY.
    for _ in 0..4 {
        d.tap(button::UP);
    }
    assert_eq!(d.app.sel, 0);
    d.tap(button::UP);
    assert_eq!(d.app.sel, MAIN_LINES - 1);
    d.tap(button::DOWN);
    assert_eq!(d.app.sel, 0);
    // START on PLAY starts, with the catch-up picked.
    d.tap(button::START);
    assert_eq!(d.app.scene, Scene::Race);
    assert_eq!(d.app.race.track, 1);
    assert_eq!(d.app.race.catchup, CatchUp::Off);
    assert_eq!(d.app.race.mode, Mode::Race);
    assert_eq!(d.app.opts.backdrop, Backdrop::Ship);
}

#[test]
fn leave_on_the_title_ends_the_game() {
    let mut d = Driver::new();
    d.tap(button::UP);
    let going = d.hold(button::A, (0, 0));
    assert!(!going);
    assert!(d.logs.contains(&Log::Leave));
}

#[test]
fn the_title_menu_works_by_touch() {
    let mut d = Driver::new();
    // A tap on TRACK steps the track; on CLASS the class; on MODE the mode.
    tap_at(&mut d, COL_X, row_y(1));
    assert_eq!(d.app.opts.track, 1);
    assert_eq!(d.app.sel, 1);
    tap_at(&mut d, COL_X, row_y(2));
    assert_eq!(d.app.opts.class, Class::Wild);
    tap_at(&mut d, COL_X, row_y(3));
    assert_eq!(d.app.opts.mode, Mode::Trial);
    tap_at(&mut d, COL_X, row_y(3));
    assert_eq!(d.app.opts.mode, Mode::Race);
    // Nothing happens for a tap off the menu: over the name, below the
    // rows, or right of the column on the backdrop.
    let before = (d.app.opts, d.app.sel, d.app.page);
    tap_at(&mut d, COL_X, 5);
    tap_at(&mut d, COL_X, 115);
    tap_at(&mut d, 120, row_y(0));
    tap_at(&mut d, 120, row_y(5));
    assert_eq!((d.app.opts, d.app.sel, d.app.page), before);
    assert_eq!(d.app.scene, Scene::Title);
    // OPTIONS, and every line there by taps alone, BACK included.
    tap_at(&mut d, COL_X, row_y(4));
    assert_eq!(d.app.page, Page::Options);
    tap_at(&mut d, COL_X, row_y(0));
    assert_eq!(d.app.opts.detail, Detail::Low);
    tap_at(&mut d, COL_X, row_y(1));
    assert!(!d.app.opts.fast);
    tap_at(&mut d, COL_X, row_y(2));
    assert_eq!(d.app.opts.view, ViewKind::Far);
    tap_at(&mut d, COL_X, row_y(3));
    assert_eq!(d.app.opts.catchup, CatchUp::Off);
    tap_at(&mut d, COL_X, row_y(4));
    assert_eq!(d.app.opts.backdrop, Backdrop::Ship);
    tap_at(&mut d, COL_X, row_y(4));
    assert_eq!(d.app.opts.backdrop, Backdrop::Race);
    tap_at(&mut d, COL_X, row_y(4));
    assert_eq!(d.app.opts.backdrop, Backdrop::Ship);
    // A tap below BACK (on the best lap) does nothing.
    tap_at(&mut d, COL_X, row_y(6));
    assert_eq!(d.app.page, Page::Options);
    tap_at(&mut d, COL_X, row_y(5));
    assert_eq!(d.app.page, Page::Main);
    // A tap on PLAY starts; a tap on LEAVE would end the game.
    tap_at(&mut d, COL_X, row_y(0));
    assert_eq!(d.app.scene, Scene::Race);
    assert_eq!(d.app.race.track, 1);
    assert_eq!(d.app.race.class, Class::Wild);
    assert_eq!(d.app.race.catchup, CatchUp::Off);
    assert_eq!(d.app.race.mode, Mode::Race);
    let mut d = Driver::new();
    assert!(!tap_at(&mut d, COL_X, row_y(MAIN_LINES - 1)));
    assert!(d.logs.contains(&Log::Leave));
}

#[test]
fn the_pause_menu_works_by_keys_and_by_touch() {
    let mut d = Driver::new();
    d.race(0, Class::Brisk, Mode::Race);
    for _ in 0..120 {
        d.auto();
    }
    // START pauses; the race stands still while paused.
    d.tap(button::START);
    assert_eq!(d.app.pause, Some(0));
    let tick = d.app.race.tick;
    for _ in 0..30 {
        d.hold(0, (0, 0));
    }
    assert_eq!(d.app.race.tick, tick);
    assert!(d.app.still());
    // DETAIL, RATE, VIEW can be changed here.
    d.tap(button::DOWN);
    let detail = d.app.opts.detail;
    d.tap(button::RIGHT);
    assert_ne!(d.app.opts.detail, detail);
    d.tap(button::DOWN);
    d.tap(button::A);
    assert!(!d.app.opts.fast);
    d.tap(button::DOWN);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.view, ViewKind::Far);
    // B resumes.
    d.tap(button::B);
    assert_eq!(d.app.pause, None);
    // A tap at the top pauses; a tap outside the panel resumes.
    tap_at(&mut d, 80, 4);
    assert!(d.app.pause.is_some());
    tap_at(&mut d, 4, 110);
    assert!(d.app.pause.is_none());
    // RESTART (by touch) starts the race again from the grid.
    tap_at(&mut d, 80, 4);
    tap_at(&mut d, 80, line_y(PAUSE_Y, 4));
    assert_eq!(d.app.pause, None);
    assert_eq!(d.app.race.phase, Phase::Count);
    // (The finger's lift is the first call of the new race.)
    assert!(d.app.race.tick <= 2, "tick {}", d.app.race.tick);
    // TITLE (by keys).
    d.tap(button::START);
    for _ in 0..5 {
        d.tap(button::DOWN);
    }
    assert_eq!(d.app.pause, Some(5));
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Title);
    // The pause panel is where the touch rule says.
    assert!(PAUSE_BOX.0 < 80 && PAUSE_BOX.1 < line_y(PAUSE_Y, 0));
}

#[test]
fn race_controls_from_every_input() {
    let k = |held: u32, stick: i16, trig: (u16, u16)| Keys {
        held,
        stick: (stick, 0),
        trig,
        ..Keys::default()
    };
    // Nothing held: nothing.
    let c = control(&k(0, 0, (0, 0)));
    assert!(!c.thrust && !c.brake_l && !c.brake_r && c.steer == 0);
    // A small push inside the dead zone does nothing; a full push steers fully.
    assert_eq!(control(&k(0, 5_000, (0, 0))).steer, 0);
    assert_eq!(control(&k(0, 32_767, (0, 0))).steer, 256);
    assert_eq!(control(&k(0, -32_768, (0, 0))).steer, -256);
    // The pad steers fully.
    assert_eq!(control(&k(button::LEFT, 0, (0, 0))).steer, -256);
    assert_eq!(control(&k(button::RIGHT, 0, (0, 0))).steer, 256);
    // A or the right trigger: thrust. L, R: the carve (no brake); B or
    // the left trigger: the brake (both air brakes).
    assert!(control(&k(button::A, 0, (0, 0))).thrust);
    assert!(control(&k(0, 0, (0, 1023))).thrust);
    let c = control(&k(button::L, 0, (0, 0)));
    assert!(!c.brake_l && !c.brake_r && c.carve == Carve::Left);
    let c = control(&k(button::R, 0, (0, 0)));
    assert!(!c.brake_l && !c.brake_r && c.carve == Carve::Right);
    let c = control(&k(button::B, 0, (0, 0)));
    assert!(c.brake_l && c.brake_r && c.carve == Carve::Off);
    let c = control(&k(0, 0, (1023, 0)));
    assert!(c.brake_l && c.brake_r);
    // Touch: a finger thrusts and steers towards itself; in the bottom
    // strip and far to one side it carves that way (tests/carve.rs).
    let c = control(&touch(80, 60, false));
    assert!(c.thrust && c.steer == 0 && c.carve == Carve::Off);
    let c = control(&touch(20, 60, false));
    assert!(c.thrust && c.steer == -256 && !c.brake_l && c.carve == Carve::Off);
    let c = control(&touch(150, 110, false));
    assert!(c.thrust && c.steer == 256 && !c.brake_r && c.carve == Carve::Right);
    // The top strip is the pause button, not the throttle.
    assert!(!control(&touch(80, 4, false)).thrust);
}

/// The player's ship flown by a finger alone: the finger goes where the
/// computer's pilot would steer, always thrusting (a finger down is
/// thrust), above the carve strip (a finger has no brake).
#[test]
fn a_race_flown_by_touch_alone_finishes_on_every_track() {
    for t in 0..TRACKS.len() as u8 {
        let mut d = Driver::new();
        d.draw = false;
        d.race(t, Class::Brisk, Mode::Race);
        let mut calls = 0;
        while !d.app.results() {
            let c = d.app.race.drive_plain(0);
            let x = (80 + c.steer * 48 / 256) as i16;
            let y = 60;
            d.call(touch(x, y, false));
            calls += 1;
            assert!(
                calls < 30 * 300,
                "{}: no finish by touch",
                TRACKS[usize::from(t)].name
            );
        }
        let sh = &d.app.race.ships[0];
        assert_eq!(sh.lap, LAPS + 1);
        eprintln!(
            "{} by touch: place {} time {}",
            TRACKS[usize::from(t)].name,
            d.app.race.place(0),
            sh.finish
        );
        // The results by touch: TITLE.
        d.draw = true;
        d.call(Keys::default());
        tap_at(&mut d, 80, line_y(END_Y, 1));
        assert_eq!(d.app.scene, Scene::Title);
    }
}

/// The console may hand a time step of up to u32::MAX microseconds after
/// a long gap; racing, the results and every scene must take it (once a
/// wrapped sum stopped the chip).
#[test]
fn the_longest_time_step_is_taken_in_every_scene() {
    let mut d = Driver::new();
    d.race(0, Class::Brisk, Mode::Race);
    for step in [u32::MAX, u32::MAX - 1, 1, u32::MAX, 0, u32::MAX] {
        d.call_us = step;
        assert!(d.call(Keys::default()));
    }
    // To the results: the autopilot finishes, then the longest steps again.
    d.call_us = 16_667;
    for _ in 0..60 * 400 {
        if d.app.race.phase == Phase::Done {
            break;
        }
        d.auto();
    }
    assert_eq!(d.app.race.phase, Phase::Done);
    for _ in 0..4 {
        d.call_us = u32::MAX;
        assert!(d.call(Keys::default()));
    }
    assert_eq!(d.app.scene, Scene::Race);
}

/// One time trial flown by the computer's pilot through the keys, to the
/// results and back to the title. `slow`: thrust on every other call
/// only, and no carve (with the thrust off the pilot would carve more
/// corners, and the slings would make up for the thrust). Gives the
/// player's best lap of that trial (ticks).
fn fly_trial(d: &mut Driver, track: u8, class: Class, slow: bool) -> u32 {
    d.race(track, class, Mode::Trial);
    let mut calls = 0u32;
    while !d.app.results() {
        let (mut held, stick) = common::keys_of_ctl(d.app.race.drive(0));
        if slow {
            held &= !(button::L | button::R);
            if calls % 2 == 1 {
                held &= !button::A;
            }
        }
        d.hold(held, stick);
        calls += 1;
        assert!(calls < 30 * 600, "no end in ten minutes");
    }
    let best = d.app.race.ships[0].best;
    assert!(best > 0);
    d.tap(button::DOWN);
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Title);
    best
}

/// The session's best lap table (review of 2026-11-22, finding 1): a
/// better lap replaces the best, a slower one leaves it, and each track
/// and class keeps its own.
#[test]
fn the_session_keeps_the_best_lap_for_each_track_and_class() {
    let mut d = Driver::new();
    d.draw = false;
    d.set(Detail::Low, ViewKind::Chase, false);
    assert_eq!(d.app.best, [[0; 3]; 3], "no best lap before any race");
    // A slow trial first: its best lap is the session's.
    let slow = fly_trial(&mut d, 0, Class::Brisk, true);
    assert_eq!(d.app.session_best(0, Class::Brisk), slow);
    // A faster one replaces it.
    let fast = fly_trial(&mut d, 0, Class::Brisk, false);
    assert!(fast < slow, "the test needs a faster trial: {fast} {slow}");
    assert_eq!(d.app.session_best(0, Class::Brisk), fast);
    // A slower one again leaves it.
    let again = fly_trial(&mut d, 0, Class::Brisk, true);
    assert!(
        again > fast,
        "the test needs a slower trial: {again} {fast}"
    );
    assert_eq!(d.app.session_best(0, Class::Brisk), fast);
    // Another class on the same track, and another track: their own.
    let other_class = fly_trial(&mut d, 0, Class::Steady, false);
    let other_track = fly_trial(&mut d, 1, Class::Brisk, true);
    assert_eq!(d.app.session_best(0, Class::Steady), other_class);
    assert_eq!(d.app.session_best(1, Class::Brisk), other_track);
    assert_eq!(d.app.session_best(0, Class::Brisk), fast);
    eprintln!(
        "best laps (ticks): slow {slow}, fast {fast}, slow again {again}, \
         other class {other_class}, other track {other_track}"
    );
    // Nothing else is filled in.
    for track in 0..TRACKS.len() as u8 {
        for class in Class::ALL {
            let set = matches!(
                (track, class),
                (0, Class::Brisk) | (0, Class::Steady) | (1, Class::Brisk)
            );
            assert_eq!(
                d.app.session_best(track, class) != 0,
                set,
                "track {track} {class:?}"
            );
        }
    }
}
