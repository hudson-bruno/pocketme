//! Every control, one at a time: the defaults, the title menu (buttons,
//! stick and finger), the game's own pause menu, the driving controls
//! on the pad (stick, triggers, d-pad, A B X R), the keys that change
//! the look while racing (SELECT, Y), the finger zones while racing,
//! and the frame rate and video hint the game asks for.

mod common;

use common::Driver;
use game_kit::{Touch, button, hint, rate};
use hairpin::Hairpin;
use hairpin::app::{App, CARD_WAIT, DEAD_ZONE, Event, Keys, Scene};
use hairpin::hud::{self, MENU_LINE, MENU_Y, PAUSE_X, Zone};
use hairpin::race::{COUNTDOWN, Pad};
use hairpin::render::{CX, Cam, Detail, TOP};

fn finger(x: i16, y: i16) -> Keys {
    Keys {
        touch: Some(Touch {
            x,
            y,
            down: true,
            started: true,
            ended: false,
        }),
        ..Keys::default()
    }
}

fn pad_of(k: Keys) -> Pad {
    App::new().controls(&k)
}

/// The middle of menu line `i`, at column `x`.
fn line_y(i: u8) -> i16 {
    MENU_Y + i16::from(i) * MENU_LINE + 2
}

/// From the title into the race, past the countdown.
fn racing(d: &mut Driver) {
    d.play_from_title();
    d.idle(COUNTDOWN / 2 + 2);
    assert!(!d.app.race.counting());
}

#[test]
fn the_defaults_are_course_1_heavy_chase_and_60_pictures_a_second() {
    let app = App::new();
    assert_eq!(app.scene, Scene::Title);
    assert_eq!(app.opts.course, 0);
    assert_eq!(app.opts.detail, Detail::Heavy);
    assert_eq!(app.opts.cam, Cam::Chase);
    assert!(app.opts.fast);
    assert_eq!(app.menu, 0);
    assert!(app.pause.is_none());
}

#[test]
fn the_title_menu_changes_every_option_and_leaves() {
    let mut d = Driver::new();
    d.tap(button::DOWN);
    assert_eq!(d.app.menu, 1);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.course, 1);
    assert_eq!(d.app.race.course, 1, "the course behind the menu follows");
    d.tap(button::LEFT);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.course, 2, "LEFT from course 1 wraps to the last");
    d.tap(button::A);
    assert_eq!(d.app.opts.course, 0, "A steps forward too");

    d.tap(button::DOWN);
    assert_eq!(d.app.menu, 2);
    for want in [Detail::Light, Detail::Normal, Detail::Heavy] {
        d.tap(button::RIGHT);
        assert_eq!(d.app.opts.detail, want);
    }
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.detail, Detail::Normal);
    d.tap(button::RIGHT);

    d.tap(button::DOWN);
    assert_eq!(d.app.menu, 3);
    d.tap(button::RIGHT);
    assert!(!d.app.opts.fast);
    d.tap(button::A);
    assert!(d.app.opts.fast);

    d.tap(button::DOWN);
    assert_eq!(d.app.menu, 4);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.cam, Cam::Nose);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.cam, Cam::Chase);

    // UP from the first line wraps to LEAVE; A there ends the game.
    d.app.menu = 0;
    d.tap(button::UP);
    assert_eq!(d.app.menu, 5);
    assert!(!d.hold(button::A), "LEAVE ends the game");
    assert!(d.events.contains(&Event::Quit));
}

#[test]
fn the_stick_steps_the_title_menu_once_per_push() {
    let mut d = Driver::new();
    let stick = |x: i16, y: i16| Keys {
        left: (x, y),
        ..Keys::default()
    };
    d.call_keys(stick(0, 30_000));
    assert_eq!(d.app.menu, 1, "down");
    d.call_keys(stick(0, 30_000));
    assert_eq!(d.app.menu, 1, "held: no second step");
    d.call_keys(stick(0, 0));
    d.call_keys(stick(0, 30_000));
    assert_eq!(d.app.menu, 2, "pushed again");
    d.call_keys(stick(30_000, 0));
    assert_eq!(d.app.opts.detail, Detail::Light, "right steps the value");
    d.call_keys(stick(-30_000, 0));
    assert_eq!(d.app.opts.detail, Detail::Heavy, "left steps it back");
    d.call_keys(stick(0, -30_000));
    assert_eq!(d.app.menu, 1, "up");
    // A small push is not a step.
    d.call_keys(stick(0, 0));
    d.call_keys(stick(0, 9_000));
    assert_eq!(d.app.menu, 1);
}

#[test]
fn a_finger_works_the_title_menu() {
    let mut d = Driver::new();
    // A tap on the right half of COURSE steps it on, on the left half back.
    d.touch(120, line_y(1));
    assert_eq!(d.app.menu, 1);
    assert_eq!(d.app.opts.course, 1);
    d.touch(40, line_y(1));
    assert_eq!(d.app.opts.course, 0);
    d.touch(40, line_y(1));
    assert_eq!(d.app.opts.course, 2, "back from the first wraps");
    d.touch(120, line_y(2));
    assert_eq!(d.app.opts.detail, Detail::Light);
    d.touch(40, line_y(2));
    assert_eq!(d.app.opts.detail, Detail::Heavy);
    d.touch(120, line_y(3));
    assert!(!d.app.opts.fast);
    d.touch(120, line_y(3));
    assert!(d.app.opts.fast);
    d.touch(120, line_y(4));
    assert_eq!(d.app.opts.cam, Cam::Nose);
    d.touch(120, line_y(4));
    // A tap outside the menu does nothing.
    let before = d.app.opts;
    d.touch(4, line_y(2));
    d.touch(80, 115);
    assert_eq!(d.app.opts, before);
    assert_eq!(d.app.scene, Scene::Title);
    // A tap on PLAY starts the chosen course.
    d.touch(80, line_y(0));
    assert_eq!(d.app.scene, Scene::Race);
    assert_eq!(d.app.race.course, 2);
}

#[test]
fn start_on_the_title_plays() {
    let mut d = Driver::new();
    d.tap(button::DOWN);
    d.tap(button::DOWN);
    d.tap(button::START);
    assert_eq!(d.app.scene, Scene::Race);
}

#[test]
fn a_finger_can_leave_from_the_title() {
    let mut d = Driver::new();
    d.finger(80, line_y(5), true);
    assert_eq!(d.app.scene, Scene::Title);
    let mut d = Driver::new();
    assert!(!d.call_keys(finger(80, line_y(5))), "LEAVE ends the game");
}

#[test]
fn the_pad_drives() {
    // Nothing held: nothing wished.
    assert_eq!(pad_of(Keys::default()), Pad::default());
    // A, UP: full throttle. B, DOWN, L: full brake.
    for b in [button::A, button::UP] {
        let p = pad_of(Keys {
            held: b,
            ..Keys::default()
        });
        assert_eq!((p.throttle, p.brake), (256, 0));
    }
    for b in [button::B, button::DOWN, button::L] {
        let p = pad_of(Keys {
            held: b,
            ..Keys::default()
        });
        assert_eq!((p.throttle, p.brake), (0, 256));
    }
    // X, R: boost.
    for b in [button::X, button::R] {
        assert!(
            pad_of(Keys {
                held: b,
                ..Keys::default()
            })
            .boost
        );
    }
    // The d-pad steers fully.
    let p = pad_of(Keys {
        held: button::LEFT,
        ..Keys::default()
    });
    assert_eq!(p.steer, -256);
    let p = pad_of(Keys {
        held: button::RIGHT,
        ..Keys::default()
    });
    assert_eq!(p.steer, 256);
    // The stick steers by how far it is pushed, past a dead zone.
    let steer = |x: i16| {
        pad_of(Keys {
            left: (x, 0),
            ..Keys::default()
        })
        .steer
    };
    assert_eq!(steer((DEAD_ZONE - 1) as i16), 0);
    assert_eq!(steer(-(DEAD_ZONE - 1) as i16), 0);
    assert_eq!(steer(32_767), 256);
    assert_eq!(steer(-32_767), -256);
    let half = steer(((DEAD_ZONE + 32_767) / 2) as i16);
    assert!((120..=136).contains(&half), "half way: {half}");
    assert!(steer(-20_000) < 0 && steer(20_000) > 0);
    // The right stick steers too, when the left one is in the middle;
    // the left one wins when both are pushed.
    let both = |l: i16, r: i16| {
        pad_of(Keys {
            left: (l, 0),
            right: (r, 0),
            ..Keys::default()
        })
        .steer
    };
    assert_eq!(both(0, 32_767), 256);
    assert_eq!(both(0, -32_767), -256);
    assert_eq!(both(-32_767, 32_767), -256);
    // The triggers: right is throttle, left is brake, by how far.
    let p = pad_of(Keys {
        triggers: (0, 32_767),
        ..Keys::default()
    });
    assert_eq!((p.throttle, p.brake), (256, 0));
    let p = pad_of(Keys {
        triggers: (16_384, 0),
        ..Keys::default()
    });
    assert_eq!(p.throttle, 0);
    assert!((120..=136).contains(&p.brake), "half brake: {}", p.brake);
}

#[test]
fn a_finger_drives() {
    let d = Detail::Light;
    // Anywhere in the middle: drive; the further from the middle, the
    // harder the turn.
    assert_eq!(hud::zone_at(d, CX as i16, 60), Zone::Drive(0));
    assert_eq!(hud::zone_at(d, CX as i16 + 3, 60), Zone::Drive(0));
    let Zone::Drive(a) = hud::zone_at(d, CX as i16 + 20, 60) else {
        panic!()
    };
    let Zone::Drive(b) = hud::zone_at(d, CX as i16 + 50, 60) else {
        panic!()
    };
    assert!(0 < a && a < b && b <= 256, "{a} {b}");
    assert_eq!(hud::zone_at(d, 150, 60), Zone::Drive(256));
    assert_eq!(hud::zone_at(d, 5, 60), Zone::Drive(-256));
    // The two boxes at the bottom, in every detail.
    for detail in Detail::ALL {
        let (brake, boost) = hud::boxes(detail);
        let mid = |r: hud::Rect| (r.0 + r.2 as i16 / 2, r.1 + r.3 as i16 / 2);
        let (x, y) = mid(brake);
        assert_eq!(hud::zone_at(detail, x, y), Zone::Brake, "{detail:?}");
        let (x, y) = mid(boost);
        assert_eq!(hud::zone_at(detail, x, y), Zone::Boost, "{detail:?}");
    }
    // The pause box in the top bar.
    assert_eq!(hud::zone_at(d, PAUSE_X + 5, 3), Zone::Pause);
    assert_ne!(hud::zone_at(d, 20, 3), Zone::Pause);

    let (brake, boost) = hud::boxes(d);
    let p = pad_of(finger(CX as i16 + 50, 60));
    assert_eq!(p.throttle, 256);
    assert!(p.steer > 200);
    let p = pad_of(finger(brake.0 + 4, brake.1 + 4));
    assert_eq!((p.throttle, p.brake), (0, 256));
    let p = pad_of(finger(boost.0 + 4, boost.1 + 4));
    assert!(p.boost);
    assert_eq!(p.throttle, 256);
}

#[test]
fn start_pauses_and_the_race_stands_still() {
    let mut d = Driver::new();
    racing(&mut d);
    d.hold(button::A);
    d.idle(1);
    d.hold(button::A);
    let tick = d.app.race.tick;
    let prog = d.app.race.karts[1].prog;
    d.tap(button::START);
    assert_eq!(d.app.pause, Some(0));
    let frozen = d.app.race.tick;
    for _ in 0..30 {
        d.hold(button::A | button::RIGHT);
        d.hold(0);
    }
    assert_eq!(d.app.pause, None, "A on RESUME goes on");
    d.tap(button::START);
    let frozen2 = d.app.race.tick;
    d.idle(40);
    assert_eq!(d.app.race.tick, frozen2, "nothing moves while paused");
    assert!(frozen >= tick);
    assert!(d.app.race.karts[1].prog > prog);
    // START again goes on.
    d.tap(button::START);
    assert_eq!(d.app.pause, None);
    d.idle(2);
    assert!(d.app.race.tick > frozen2);
    // B leaves the pause too.
    d.tap(button::START);
    d.tap(button::B);
    assert_eq!(d.app.pause, None);
}

#[test]
fn the_pause_menu_changes_the_look_restarts_and_leaves() {
    let mut d = Driver::new();
    racing(&mut d);
    d.tap(button::START);
    d.tap(button::DOWN);
    assert_eq!(d.app.pause, Some(1));
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.detail, Detail::Light);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.detail, Detail::Heavy);
    d.tap(button::DOWN);
    d.tap(button::A);
    assert!(!d.app.opts.fast, "RATE in the pause menu");
    d.tap(button::A);
    assert!(d.app.opts.fast);
    d.tap(button::DOWN);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.cam, Cam::Nose);
    d.tap(button::DOWN);
    assert_eq!(d.app.pause, Some(4));
    let ticks = d.app.race.tick;
    assert!(ticks > COUNTDOWN);
    d.tap(button::A);
    assert_eq!(d.app.pause, None, "RESTART");
    assert_eq!(d.app.scene, Scene::Race);
    assert!(d.app.race.tick < 4, "the race starts again from the grid");
    // TITLE, by finger this time.
    d.tap(button::START);
    d.touch(80, line_y(5));
    assert_eq!(d.app.scene, Scene::Title);
    assert_eq!(d.app.pause, None);
}

#[test]
fn a_finger_pauses_and_works_the_pause_menu() {
    let mut d = Driver::new();
    racing(&mut d);
    d.touch(PAUSE_X + 8, (TOP / 2) as i16);
    assert_eq!(d.app.pause, Some(0));
    d.touch(120, line_y(1));
    assert_eq!(d.app.opts.detail, Detail::Light);
    d.touch(40, line_y(1));
    assert_eq!(d.app.opts.detail, Detail::Heavy);
    d.touch(120, line_y(3));
    assert_eq!(d.app.opts.cam, Cam::Nose);
    d.touch(80, line_y(0));
    assert_eq!(d.app.pause, None, "RESUME");
    // A finger in the view drives; it does not pause.
    d.touch(80, 60);
    assert_eq!(d.app.pause, None);
}

#[test]
fn select_and_y_change_the_look_while_racing() {
    let mut d = Driver::new();
    racing(&mut d);
    d.tap(button::SELECT);
    assert_eq!(d.app.opts.detail, Detail::Light);
    d.tap(button::SELECT);
    assert_eq!(d.app.opts.detail, Detail::Normal);
    d.tap(button::SELECT);
    assert_eq!(d.app.opts.detail, Detail::Heavy);
    assert!(d.events.contains(&Event::Detail(Detail::Normal)));
    d.tap(button::Y);
    assert_eq!(d.app.opts.cam, Cam::Nose);
    d.tap(button::Y);
    assert_eq!(d.app.opts.cam, Cam::Chase);
    assert!(d.app.note.is_some(), "a note says what changed");
}

#[test]
fn the_finish_card_takes_a_finger() {
    let mut d = Driver::new();
    d.race_course(0);
    let trip = common::autopilot(&mut d, 30 * 360);
    assert!(trip.finished);
    // The card waits a moment before it takes a press.
    d.touch(80, 76 + 2);
    assert_eq!(d.app.scene, Scene::Finish);
    d.idle(CARD_WAIT / 2 + 2);
    // TITLE by finger.
    d.touch(80, 76 + MENU_LINE + 2);
    assert_eq!(d.app.scene, Scene::Title);
}

#[test]
fn the_game_asks_for_the_rate_and_hint_it_needs() {
    let mut d = Driver::new();
    // The title is still: called only on input.
    assert_eq!(
        Hairpin::wanted(&d.app),
        (rate::ON_DEMAND, hint::MOSTLY_STILL)
    );
    // A note on the title moves (it goes away by itself).
    d.tap(button::DOWN);
    d.tap(button::DOWN);
    d.tap(button::RIGHT);
    assert!(d.app.note.is_some());
    assert_eq!(Hairpin::wanted(&d.app).0, rate::FPS60);
    d.idle(60);
    assert_eq!(Hairpin::wanted(&d.app).0, rate::ON_DEMAND);
    d.tap(button::LEFT);
    d.idle(60);
    d.app.menu = 0;
    d.tap(button::A);
    assert_eq!(Hairpin::wanted(&d.app), (rate::FPS60, hint::SCROLLS));
    d.app.opts.fast = false;
    assert_eq!(Hairpin::wanted(&d.app), (rate::FPS30, hint::SCROLLS));
    d.app.opts.fast = true;
    // Paused: still once the note is gone.
    d.tap(button::START);
    d.idle(60);
    assert_eq!(
        Hairpin::wanted(&d.app),
        (rate::ON_DEMAND, hint::MOSTLY_STILL)
    );
}
