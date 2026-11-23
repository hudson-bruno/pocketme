//! The title's two backdrops (RACE: a made-up race filmed; SHIP: the
//! player's ship turning on the grid). Settings keep their values across
//! the OPTIONS page and back, a race started from PLAY is the same race
//! whatever ran behind the menu before it, the title is the same picture
//! for the same keys, and ten minutes of it never panic, never write a
//! log line and never put the camera in the road.

mod common;

use camber::app::{App, Backdrop, Keys, Page, Scene};
use camber::race::{CatchUp, Class, Mode};
use camber::scene::{Detail, ViewKind};
use camber::track::{FM, ROOF, TRACKS};
use camber::tv::Shot;
use common::Driver;
use game_kit::{Touch, button};

/// One call of a script: buttons held (pressed when not held the call
/// before), or a tap at a point of the screen.
#[derive(Clone, Copy)]
enum Step {
    Hold(u32),
    Tap(i16, i16),
}

fn play(d: &mut Driver, s: Step) -> bool {
    match s {
        Step::Hold(b) => d.hold(b, (0, 0)),
        Step::Tap(x, y) => {
            d.held = 0;
            d.call(Keys {
                touch: Some(Touch {
                    x,
                    y,
                    down: true,
                    started: true,
                    ended: false,
                }),
                ..Keys::default()
            })
        }
    }
}

/// Press and let go: two calls.
fn press(b: u32) -> [Step; 2] {
    [Step::Hold(b), Step::Hold(0)]
}

/// The y of the middle of the title's row `k`.
fn row_y(k: u8) -> i16 {
    camber::app::ROWS_Y + i16::from(k) * camber::app::ROW + camber::app::ROW / 2
}

/// A tap on the title's row `k`, then a call with nothing.
fn tap_row(k: u8) -> [Step; 2] {
    [Step::Tap(30, row_y(k)), Step::Hold(0)]
}

/// From the main page's PLAY line: OPTIONS, BACKDROP one to the right,
/// BACK, and up to PLAY again, by buttons.
fn ship_by_keys() -> Vec<Step> {
    let mut v = Vec::new();
    for _ in 0..4 {
        v.extend(press(button::DOWN));
    }
    v.extend(press(button::A));
    for _ in 0..4 {
        v.extend(press(button::DOWN));
    }
    v.extend(press(button::RIGHT));
    v.extend(press(button::B));
    for _ in 0..4 {
        v.extend(press(button::UP));
    }
    v
}

#[test]
fn settings_keep_their_values_across_options_and_back() {
    // By buttons.
    let mut d = Driver::new();
    d.draw = false;
    for s in [
        press(button::DOWN),
        press(button::RIGHT), // TRACK 1
        press(button::DOWN),
        press(button::LEFT), // CLASS: one to the left
        press(button::DOWN),
        press(button::RIGHT), // MODE: TRIAL
        press(button::DOWN),
        press(button::A),     // OPTIONS
        press(button::RIGHT), // DETAIL
        press(button::DOWN),
        press(button::A), // RATE
        press(button::DOWN),
        press(button::RIGHT), // VIEW
        press(button::DOWN),
        press(button::RIGHT), // CATCH-UP
        press(button::DOWN),
        press(button::RIGHT), // BACKDROP
        press(button::B),
    ]
    .concat()
    {
        play(&mut d, s);
    }
    let keys = d.app.opts;
    assert_eq!((d.app.page, d.app.sel), (Page::Main, 4));
    assert_eq!(keys.track, 1);
    assert_ne!(keys.class, Class::Brisk);
    assert_eq!(keys.mode, Mode::Trial);
    assert_ne!(keys.detail, Detail::High);
    assert!(!keys.fast);
    assert_ne!(keys.view, ViewKind::Chase);
    assert_ne!(keys.catchup, CatchUp::Light);
    assert_eq!(keys.backdrop, Backdrop::Ship);
    // Into OPTIONS and out by its BACK line: nothing changes.
    for s in [press(button::A), press(button::UP), press(button::A)].concat() {
        play(&mut d, s);
    }
    assert_eq!((d.app.page, d.app.sel), (Page::Main, 4));
    assert_eq!(d.app.opts, keys);
    // The SHIP backdrop shows the chosen track and class.
    assert_eq!(d.app.race.track, 1);
    assert_eq!(d.app.race.class, keys.class);
    assert_eq!(d.app.race.mode, Mode::Trial);

    // The same by taps alone.
    let mut t = Driver::new();
    t.draw = false;
    for s in [
        tap_row(1),
        tap_row(2),
        tap_row(2),
        tap_row(3),
        tap_row(4), // OPTIONS
        tap_row(0),
        tap_row(1),
        tap_row(2),
        tap_row(3),
        tap_row(4),
        tap_row(5), // BACK
    ]
    .concat()
    {
        play(&mut t, s);
    }
    let taps = t.app.opts;
    assert_eq!((t.app.page, t.app.sel), (Page::Main, 4));
    assert_eq!(taps.track, 1);
    assert_eq!(taps.mode, Mode::Trial);
    assert!(!taps.fast);
    assert_eq!(taps.backdrop, Backdrop::Ship);
    for s in [tap_row(4), tap_row(5)].concat() {
        play(&mut t, s);
    }
    assert_eq!(t.app.opts, taps);
    // Back to RACE by a tap: the made-up race of the same track.
    for s in [tap_row(4), tap_row(4), tap_row(5)].concat() {
        play(&mut t, s);
    }
    assert_eq!(t.app.opts.backdrop, Backdrop::Race);
    assert_eq!((t.app.race.mode, t.app.race.track), (Mode::Race, 1));
}

/// Plays `script` on the title, then PLAY, then `race_calls` calls of the
/// computer's pilot through the keys; hands back the driver after each.
fn race_after(script: &[Step], mode: Mode) -> Driver {
    let mut d = Driver::new();
    d.app.opts.mode = mode;
    for &s in script {
        assert!(play(&mut d, s));
        assert_eq!(d.app.scene, Scene::Title);
    }
    assert_eq!((d.app.page, d.app.sel), (Page::Main, 0));
    play(&mut d, Step::Hold(button::A));
    assert_eq!(d.app.scene, Scene::Race);
    d
}

#[test]
fn play_starts_the_same_race_whatever_ran_behind_the_menu() {
    const WAIT: usize = 400;
    // RACE all along.
    let plain = vec![Step::Hold(0); WAIT];
    // SHIP, picked by buttons, left on for a while.
    let mut ship = ship_by_keys();
    // RACE, but the track changed and changed back (a new made-up race).
    let mut moved = Vec::new();
    moved.extend(press(button::DOWN));
    moved.extend(press(button::RIGHT));
    moved.extend([Step::Hold(0); 90]);
    moved.extend(press(button::LEFT));
    moved.extend(press(button::UP));
    for v in [&mut ship, &mut moved] {
        v.resize(WAIT, Step::Hold(0));
    }
    for mode in [Mode::Race, Mode::Trial] {
        let mut ds = [&plain, &ship, &moved].map(|s| race_after(s, mode));
        assert_eq!(ds[1].app.opts.backdrop, Backdrop::Ship);
        // Same number of pictures before PLAY: the same seed.
        assert!(ds.iter().all(|d| d.app.frames == ds[0].app.frames));
        for call in 0..30 * 70 {
            for d in ds.iter_mut() {
                d.auto();
            }
            let [a, b, c] = &ds;
            for (k, o) in [b, c].into_iter().enumerate() {
                assert!(
                    a.app.race == o.app.race,
                    "{mode:?} race {k} parts at call {call}"
                );
                assert_eq!(a.app.cam, o.app.cam, "{mode:?} camera {k} at call {call}");
                assert!(*a.frame == *o.frame, "{mode:?} picture {k} at call {call}");
                assert_eq!(a.logs, o.logs, "{mode:?} logs {k}");
            }
        }
        assert!(
            ds[0].logs.len() > 2,
            "{mode:?}: laps came round: {:?}",
            ds[0].logs
        );
    }
}

/// A title script with both backdrops, track and class changes, by
/// buttons and by taps.
fn title_script() -> Vec<Step> {
    let mut v = vec![Step::Hold(0); 200];
    v.extend(ship_by_keys());
    v.extend([Step::Hold(0); 200]);
    v.extend(tap_row(1)); // TRACK
    v.extend([Step::Hold(0); 150]);
    v.extend(tap_row(2)); // CLASS
    v.extend([Step::Hold(0); 150]);
    v.extend([tap_row(4), tap_row(4), tap_row(5)].concat()); // RACE
    v.extend([Step::Hold(0); 400]);
    v.extend(press(button::DOWN));
    v.extend(press(button::RIGHT));
    v.extend([Step::Hold(0); 400]);
    v
}

#[test]
fn the_title_is_the_same_picture_for_the_same_keys() {
    let script = title_script();
    let mut a = Driver::new();
    let mut b = Driver::new();
    assert!(*a.frame == *b.frame);
    let (mut changed, mut last) = (0u32, a.frame.clone());
    for (call, &s) in script.iter().enumerate() {
        play(&mut a, s);
        play(&mut b, s);
        assert!(*a.frame == *b.frame, "the pictures part at call {call}");
        assert_eq!(a.app.cam, b.app.cam);
        if *a.frame != *last {
            changed += 1;
            last = a.frame.clone();
        }
    }
    assert_eq!(a.app.scene, Scene::Title);
    // The backdrop moves: nearly every picture is a new one.
    assert!(changed as usize * 10 > script.len() * 9, "{changed}");
}

#[test]
fn ten_minutes_of_title_never_panic_log_or_put_the_camera_in_the_road() {
    let mut d = Driver::new();
    let mut sh = [0u32; 2];
    let mut calls = 0u32;
    // Every 40 s the next track, every 80 s the other class, every
    // 60 s the other backdrop.
    while calls < 30 * 60 * 10 {
        let o = d.app.opts;
        let want_ship = (calls / (30 * 60)) % 2 == 1;
        let want_track = ((calls / (30 * 40)) % TRACKS.len() as u32) as u8;
        let want_class = Class::ALL[((calls / (30 * 80)) % Class::ALL.len() as u32) as usize];
        if want_ship != (o.backdrop == Backdrop::Ship)
            || want_track != o.track
            || want_class != o.class
        {
            d.app.opts.backdrop = if want_ship {
                Backdrop::Ship
            } else {
                Backdrop::Race
            };
            d.app.opts.track = want_track;
            d.app.opts.class = want_class;
        }
        assert!(d.call(Keys::default()));
        calls += 1;
        let r = &d.app.race;
        let tr = r.tr();
        let cam = &d.app.cam;
        let seg = tr.seg(tr.index(cam.s));
        let half = tr.half(cam.s) * 256;
        let name = TRACKS[usize::from(r.track)].name;
        assert!(
            cam.h >= FM / 3,
            "{name}: a camera in the road at call {calls}"
        );
        if seg.kind & ROOF != 0 {
            assert!(
                cam.x.abs() < half && cam.h < FM * 5,
                "{name}: in a tunnel's wall at call {calls}"
            );
        }
        if d.app.opts.backdrop == Backdrop::Ship {
            sh[1] += 1;
            assert_eq!((r.mode, r.n), (Mode::Trial, 1));
            // Over the road beside the ship, behind it.
            assert!(cam.x.abs() < half, "{name}: the SHIP camera off the road");
            assert!(cam.s < r.ships[0].s);
        } else {
            sh[0] += 1;
            assert_eq!(r.mode, Mode::Race);
            if d.app.tv.shot == Shot::Side {
                assert!(cam.x.abs() > half + FM, "{name}: a side camera on the road");
            }
        }
    }
    println!("calls with RACE {} and with SHIP {}", sh[0], sh[1]);
    assert!(sh.iter().all(|&k| k > 30 * 60 * 4));
    assert!(d.logs.is_empty(), "{:?}", d.logs);
    assert_eq!(d.app.best, [[0; 3]; 3]);
    assert_eq!(d.time, 0);
    assert_eq!(d.app.ticks, 0);
}

#[test]
fn the_title_left_alone_keeps_its_options() {
    let mut a = App::new();
    let o = a.opts;
    for _ in 0..600 {
        a.update(&Keys {
            delta_us: 33_333,
            ..Keys::default()
        });
    }
    assert_eq!(a.opts, o);
    assert_eq!(a.scene, Scene::Title);
}
