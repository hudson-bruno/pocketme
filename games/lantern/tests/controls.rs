//! Every control, one at a time: the title menu, the game's pause menu,
//! the map, the detail key, the sticks, triggers and shoulders, the
//! finger, and the frame rate and video hint the game asks for.

mod common;

use common::Driver;
use game_kit::{Touch, button, hint, rate};
use lantern::Lantern;
use lantern::app::{App, Event, Keys, Scene};
use lantern::hud::{self, FLASH_BOX, MAP_BOX, MENU_STEP, MENU_Y, PAUSE_BOX, PAUSE_Y, STATUS_Y};
use lantern::render::Detail;
use lantern::world::TICK_US;

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

fn pad_of(k: Keys) -> lantern::world::Pad {
    App::new().controls(&k)
}

#[test]
fn the_defaults_are_level_1_heavy_and_60_pictures_a_second() {
    let app = App::new();
    assert_eq!(app.scene, Scene::Title);
    assert_eq!(app.opts.level, 0);
    assert_eq!(app.opts.detail, Detail::Heavy);
    assert!(app.opts.fast);
    assert_eq!(app.menu, 0);
}

#[test]
fn the_title_menu_changes_level_detail_and_rate_and_leaves() {
    let mut d = Driver::new();
    d.tap(button::DOWN);
    assert_eq!(d.app.menu, 1);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.level, 1);
    d.tap(button::LEFT);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.level, 3, "LEFT from level 1 wraps to the last");
    d.tap(button::A);
    assert_eq!(d.app.opts.level, 0, "A steps forward too");

    d.tap(button::DOWN);
    assert_eq!(d.app.menu, 2);
    // From HEAVY (the start) RIGHT wraps to LIGHT.
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.detail, Detail::Light);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.detail, Detail::Normal);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.detail, Detail::Heavy);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.detail, Detail::Normal);

    d.tap(button::DOWN);
    assert_eq!(d.app.menu, 3);
    d.tap(button::RIGHT);
    assert!(!d.app.opts.fast);
    d.tap(button::A);
    assert!(d.app.opts.fast);

    // UP from the first line wraps to LEAVE; A there ends the game.
    d.app.menu = 0;
    d.tap(button::UP);
    assert_eq!(d.app.menu, 4);
    assert!(!d.hold(button::A), "LEAVE ends the game");
    assert!(d.events.contains(&Event::Quit));
}

#[test]
fn the_left_stick_steps_the_title_menu_once_per_push() {
    let mut d = Driver::new();
    let stick = |y: i16| Keys {
        left: (0, y),
        ..Keys::default()
    };
    d.call_keys(stick(30_000));
    assert_eq!(d.app.menu, 1);
    // Held there: no more steps.
    d.call_keys(stick(30_000));
    d.call_keys(stick(30_000));
    assert_eq!(d.app.menu, 1);
    d.call_keys(stick(0));
    d.call_keys(stick(30_000));
    assert_eq!(d.app.menu, 2);
    d.call_keys(stick(-30_000));
    assert_eq!(d.app.menu, 1);
}

#[test]
fn start_on_the_title_plays_and_a_tap_picks_a_line() {
    let mut d = Driver::new();
    d.app.menu = 3;
    d.tap(button::START);
    assert_eq!(d.app.scene, Scene::Play);
    assert!(d.events.contains(&Event::Start {
        level: 1,
        detail: Detail::Heavy
    }));

    let mut d = Driver::new();
    // The DETAIL line (the third) by finger: picks it and steps it.
    let y = MENU_Y + 2 * MENU_STEP + 2;
    d.touch(60, y);
    assert_eq!(d.app.menu, 2);
    assert_eq!(d.app.opts.detail, Detail::Light, "HEAVY steps on to LIGHT");
    // The PLAY line.
    d.touch(60, MENU_Y + 2);
    assert_eq!(d.app.scene, Scene::Play);
    // Off the card: nothing.
    assert_eq!(hud::title_line_at(5, MENU_Y + 2), None);
}

#[test]
fn the_pause_menu_resumes_restarts_and_goes_to_the_title() {
    let mut d = Driver::new();
    d.play_from_title();
    for _ in 0..10 {
        d.hold(button::UP);
    }
    d.hold(0);
    let (x, y, t) = (d.app.world.x, d.app.world.y, d.app.world.ticks);
    assert!(y != 0 || x != 0);

    d.tap(button::START);
    assert_eq!(d.app.pause, Some(0));
    // The world waits while paused, whatever is held.
    for _ in 0..20 {
        d.hold(button::R | button::Y);
    }
    d.hold(0);
    assert_eq!((d.app.world.x, d.app.world.y, d.app.world.ticks), (x, y, t));
    // RESUME.
    d.tap(button::A);
    assert_eq!(d.app.pause, None);
    assert_eq!(d.app.scene, Scene::Play);

    // START and B close it too.
    d.tap(button::START);
    d.tap(button::START);
    assert_eq!(d.app.pause, None);
    d.tap(button::START);
    d.tap(button::B);
    assert_eq!(d.app.pause, None);

    // RESTART LEVEL: back to the start, one more try counted.
    let start = lantern::world::World::new(0);
    d.tap(button::START);
    d.tap(button::DOWN);
    assert_eq!(d.app.pause, Some(1));
    d.tap(button::A);
    assert_eq!(d.app.pause, None);
    assert_eq!(d.app.scene, Scene::Play);
    assert_eq!(d.app.run.retries, 1);
    // One call of ticks may have run since (nothing held: no move).
    assert_eq!((d.app.world.x, d.app.world.y), (start.x, start.y));

    // TITLE, by finger on its line.
    d.tap(button::START);
    d.touch(60, PAUSE_Y + 2 * MENU_STEP + 2);
    assert_eq!(d.app.scene, Scene::Title);
    assert_eq!(d.app.pause, None);
}

#[test]
fn select_and_the_map_box_open_the_map_and_the_world_waits() {
    let mut d = Driver::new();
    d.play_from_title();
    d.idle(5);
    d.tap(button::SELECT);
    assert_eq!(d.app.scene, Scene::Map);
    let t = d.app.world.ticks;
    d.idle(30);
    assert_eq!(d.app.world.ticks, t);
    d.tap(button::SELECT);
    assert_eq!(d.app.scene, Scene::Play);

    d.touch(MAP_BOX.0 + 5, STATUS_Y + 5);
    assert_eq!(d.app.scene, Scene::Map);
    // Any tap on the map goes back.
    d.touch(80, 50);
    assert_eq!(d.app.scene, Scene::Play);
    // START on the map: back to play, paused.
    d.tap(button::SELECT);
    d.tap(button::START);
    assert_eq!(d.app.scene, Scene::Play);
    assert_eq!(d.app.pause, Some(0));
    d.tap(button::START);

    d.touch(PAUSE_BOX.0 + 4, STATUS_Y + 5);
    assert_eq!(d.app.pause, Some(0));
}

#[test]
fn x_cycles_the_detail_while_playing_with_a_note() {
    let mut d = Driver::new();
    d.play_from_title();
    let mut seen = Vec::new();
    for _ in 0..3 {
        d.tap(button::X);
        seen.push(d.app.opts.detail);
        assert!(d.app.note.is_some());
        assert!(d.events.contains(&Event::Detail(d.app.opts.detail)));
    }
    assert_eq!(seen, [Detail::Light, Detail::Normal, Detail::Heavy]);
    // The note goes after a while (75 ticks).
    d.idle(40);
    assert!(d.app.note.is_none());
}

#[test]
fn sticks_triggers_and_shoulders_move_the_player() {
    // Twin sticks. The left stick walks and steps sideways; the middle
    // is ignored.
    let p = pad_of(Keys {
        left: (0, -32_767),
        ..Keys::default()
    });
    assert_eq!((p.forward, p.turn, p.strafe), (256, 0, 0));
    let p = pad_of(Keys {
        left: (3_000, -3_000),
        ..Keys::default()
    });
    assert_eq!(
        (p.forward, p.turn, p.strafe),
        (0, 0, 0),
        "inside the dead zone"
    );
    let p = pad_of(Keys {
        left: (-32_768, 32_767),
        ..Keys::default()
    });
    assert_eq!((p.forward, p.turn, p.strafe), (-256, 0, -256));
    // The right stick turns, and only turns.
    let p = pad_of(Keys {
        right: (32_767, -32_767),
        ..Keys::default()
    });
    assert_eq!((p.forward, p.turn, p.strafe), (0, 256, 0));
    // Both at once: walk forward and to the right while turning left.
    let p = pad_of(Keys {
        left: (32_767, -32_767),
        right: (-32_768, 0),
        ..Keys::default()
    });
    assert_eq!((p.forward, p.turn, p.strafe), (256, -256, 256));
    // Triggers: side steps as far as pressed.
    let half = pad_of(Keys {
        triggers: (0, 16_384),
        ..Keys::default()
    });
    assert!((120..=136).contains(&half.strafe), "{}", half.strafe);
    let left = pad_of(Keys {
        triggers: (32_767, 0),
        ..Keys::default()
    });
    assert_eq!(left.strafe, -256);
    // Shoulders: full side steps.
    assert_eq!(
        pad_of(Keys {
            held: button::L,
            ..Keys::default()
        })
        .strafe,
        -256
    );
    assert_eq!(
        pad_of(Keys {
            held: button::R,
            ..Keys::default()
        })
        .strafe,
        256
    );
    // The d-pad wins over the sticks: down backs off, left turns.
    let p = pad_of(Keys {
        held: button::DOWN | button::LEFT,
        left: (0, -32_767),
        right: (32_767, 0),
        ..Keys::default()
    });
    assert_eq!((p.forward, p.turn), (-256, -256));
}

#[test]
fn a_finger_on_the_view_steers() {
    for detail in Detail::ALL {
        let v = detail.view();
        let mut app = App::new();
        app.opts.detail = detail;
        let at = |fx: i32, fy: i32| {
            // fx, fy in 1/6 of the view.
            let x = (v.x0 + v.w * fx / 6) as i16;
            let y = (v.y0 + v.h * fy / 6) as i16;
            let p = app.controls(&finger(x, y));
            (p.forward, p.turn)
        };
        assert_eq!(at(3, 1), (256, 0), "{detail:?}: top middle walks");
        assert_eq!(at(3, 5), (-256, 0), "{detail:?}: bottom backs off");
        assert_eq!(at(3, 3), (0, 0), "{detail:?}: the middle stands");
        let edge = |x: i32| {
            app.controls(&finger(x as i16, (v.y0 + v.h / 2) as i16))
                .turn
        };
        let (left, right) = (edge(v.x0), edge(v.x0 + v.w - 1));
        assert!(left <= -200 && right >= 200, "{detail:?}: {left} {right}");
        // The status line steers nothing.
        let p = app.controls(&finger(20, STATUS_Y + 4));
        assert_eq!((p.forward, p.turn), (0, 0));
    }

    // In play: a finger held at the top walks the player forward.
    let mut d = Driver::new();
    d.play_from_title();
    let before = (d.app.world.x, d.app.world.y);
    let v = d.app.opts.detail.view();
    let mut k = finger((v.x0 + v.w / 2) as i16, (v.y0 + 4) as i16);
    for _ in 0..15 {
        d.call_keys(k);
        if let Some(t) = &mut k.touch {
            t.started = false;
        }
    }
    let after = (d.app.world.x, d.app.world.y);
    assert_ne!(before, after);
}

#[test]
fn taps_on_the_middle_use_and_on_the_flash_box_flash() {
    let mut d = Driver::new();
    d.call_us = u64::from(TICK_US); // one tick a call
    d.play_from_title();
    let v = d.app.opts.detail.view();
    d.call_keys(finger((v.x0 + v.w / 2) as i16, (v.y0 + v.h / 2) as i16));
    assert!(d.app.last_pad.act, "the middle of the view uses");
    d.hold(0);
    assert!(!d.app.last_pad.act);
    // Off the middle (still on the view): no use.
    d.call_keys(finger((v.x0 + 2) as i16, (v.y0 + v.h / 2) as i16));
    assert!(!d.app.last_pad.act);

    d.call_keys(finger(FLASH_BOX.0 + 6, STATUS_Y + 5));
    assert!(d.app.last_pad.flash);
    assert!(d.app.world.cool > 0, "the lamp flashed");
}

/// The owner's rule: A (and the tap in the middle) with nothing to use
/// ahead does nothing; B, Y and the FLASH box flash.
#[test]
fn a_with_nothing_ahead_does_nothing_and_b_y_and_the_flash_box_flash() {
    use lantern::fixed::centre;
    let mut d = Driver::new();
    d.call_us = u64::from(TICK_US); // one tick a call
    d.play_from_title();
    // Level 1, at (1, 1) looking east: open floor ahead.
    d.app.world.x = centre(1);
    d.app.world.y = centre(1);
    d.app.world.angle = 0;
    assert_eq!(d.app.world.at(2, 1), lantern::world::cell::FLOOR);
    let flashed = |d: &Driver| d.app.world.tally.flashes;

    let oil = d.app.world.oil;
    d.tap(button::A);
    d.idle(2);
    assert_eq!(flashed(&d), 0, "A did not flash");
    assert_eq!(d.app.world.cool, 0);
    assert!(oil - d.app.world.oil <= 4, "A cost no oil");

    let v = d.app.opts.detail.view();
    d.call_keys(finger((v.x0 + v.w / 2) as i16, (v.y0 + v.h / 2) as i16));
    assert!(d.app.last_pad.act, "the middle of the view uses");
    d.idle(2);
    assert_eq!(flashed(&d), 0, "the use tap did not flash");

    for (n, press) in [button::B, button::Y].into_iter().enumerate() {
        d.tap(press);
        d.idle(1);
        assert_eq!(flashed(&d), n as u16 + 1, "button {press:#x} flashes");
        d.idle(60); // the lamp cools down
    }
    d.call_keys(finger(FLASH_BOX.0 + 6, STATUS_Y + 5));
    d.idle(1);
    assert_eq!(flashed(&d), 3, "the FLASH box flashes");
}

#[test]
fn presses_are_kept_when_calls_come_faster_than_ticks() {
    let mut d = Driver::new();
    d.play_from_title();
    d.call_us = 4_000; // 250 calls a second: most run no tick
    // Line the clock up just after a tick, so the press's call runs none.
    while d.now_us % u64::from(TICK_US) > 4_000 {
        d.hold(0);
    }
    let oil = d.app.world.oil;
    d.hold(button::B);
    d.hold(0);
    for _ in 0..6 {
        d.hold(0);
    }
    assert!(d.app.world.oil < oil, "the flash was not lost");
    assert!(d.app.world.cool > 0);
}

#[test]
fn the_game_asks_for_the_rate_and_hint_it_needs() {
    let mut d = Driver::new();
    assert_eq!(
        Lantern::wanted(&d.app),
        (rate::ON_DEMAND, hint::MOSTLY_STILL),
        "the title waits for input"
    );
    d.play_from_title();
    assert_eq!(Lantern::wanted(&d.app), (rate::FPS60, hint::SCROLLS));
    d.app.opts.fast = false;
    assert_eq!(Lantern::wanted(&d.app), (rate::FPS30, hint::SCROLLS));
    d.app.opts.fast = true;
    assert_eq!(Lantern::wanted(&d.app), (rate::FPS60, hint::SCROLLS));
    d.tap(button::START);
    assert_eq!(
        Lantern::wanted(&d.app),
        (rate::ON_DEMAND, hint::MOSTLY_STILL)
    );
    d.tap(button::START);
    d.tap(button::SELECT);
    assert_eq!(
        Lantern::wanted(&d.app),
        (rate::ON_DEMAND, hint::MOSTLY_STILL)
    );
    d.tap(button::SELECT);
    // A note on the pause menu keeps the calls coming until it goes.
    d.tap(button::X);
    d.tap(button::START);
    assert_eq!(Lantern::wanted(&d.app).0, rate::FPS60);
    d.idle(40);
    assert_eq!(Lantern::wanted(&d.app).0, rate::ON_DEMAND);
}
