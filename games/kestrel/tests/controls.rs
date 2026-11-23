//! The controls: every screen with the pad and with touch alone, every
//! setting changed while flying, the rate the game asks the console for,
//! and a touch-only player that finishes all three missions.

mod common;

use common::{Driver, Plan, plan};
use game_kit::{Touch, button, hint, rate};
use kestrel::Kestrel;
use kestrel::app::{CARD_WAIT, Event, Keys, MAX_CATCH_UP, Scene};
use kestrel::course::MISSIONS;
use kestrel::hud::{self, BOX_W, MENU_STEP, PAUSE_Y, TITLE_Y};
use kestrel::scene::{self, BOTTOM_Y, Detail, Eye};
use kestrel::world::State;

/// A finger held at (x, y); `first` for the call it lands.
fn finger(d: &mut Driver, x: i16, y: i16, first: bool) -> bool {
    d.held = 0;
    d.call_keys(Keys {
        touch: Some(Touch {
            x,
            y,
            down: true,
            started: first,
            ended: false,
        }),
        ..Keys::default()
    })
}

fn title_line(n: u8) -> (i16, i16) {
    (40, TITLE_Y + i16::from(n) * MENU_STEP)
}

fn pause_line(n: u8) -> (i16, i16) {
    (50, PAUSE_Y + i16::from(n) * MENU_STEP)
}

/// The middle of bottom box `n` (0 = roll left .. 4 = roll right).
fn bottom_box(n: i32) -> (i16, i16) {
    ((n * BOX_W + BOX_W / 2) as i16, (BOTTOM_Y + 6) as i16)
}

fn flying() -> Driver {
    let mut d = Driver::new();
    d.play_from_title();
    d
}

#[test]
fn the_defaults() {
    let d = Driver::new();
    let o = d.app.opts;
    assert_eq!(d.app.scene, Scene::Title);
    assert_eq!(o.mission, 0);
    assert_eq!(o.detail, Detail::High);
    assert_eq!(o.eye, Eye::Chase);
    assert!(o.fast);
    assert_eq!(Kestrel::wanted(&d.app), (rate::FPS60, hint::MOSTLY_STILL));
}

#[test]
fn the_title_menu_with_the_pad() {
    let mut d = Driver::new();
    d.tap(button::DOWN);
    assert_eq!(d.app.menu, 1);
    d.tap(button::RIGHT);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.mission, 2);
    assert_eq!(d.app.world.mission, 2, "the title shows the chosen mission");
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.mission, 0, "wraps round");
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.mission, 2);
    d.tap(button::DOWN);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.detail, Detail::Low, "RIGHT from HIGH wraps");
    d.tap(button::DOWN);
    d.tap(button::A);
    assert_eq!(d.app.opts.eye, Eye::Wide);
    d.tap(button::DOWN);
    d.tap(button::A);
    assert!(!d.app.opts.fast);
    d.tap(button::UP);
    d.tap(button::UP);
    d.tap(button::UP);
    d.tap(button::UP);
    d.tap(button::UP);
    assert_eq!(d.app.menu, 5, "UP from the top wraps to LEAVE");
    // START plays from any line, at the chosen mission.
    d.tap(button::START);
    assert_eq!(d.app.scene, Scene::Brief);
    assert_eq!(d.app.world.mission, 2);
    assert!(d.events.contains(&Event::Start {
        mission: 3,
        detail: Detail::Low,
        eye: Eye::Wide
    }));
}

#[test]
fn the_title_menu_with_the_stick_steps_once_per_push() {
    let mut d = Driver::new();
    for _ in 0..10 {
        d.hold_stick(0, (0, 30_000));
    }
    assert_eq!(d.app.menu, 1, "a held stick steps once");
    d.hold_stick(0, (0, 0));
    d.hold_stick(0, (0, 30_000));
    assert_eq!(d.app.menu, 2);
    d.hold_stick(0, (0, 0));
    d.hold_stick(0, (30_000, 0));
    assert_eq!(d.app.opts.detail, Detail::Low);
    d.hold_stick(0, (0, 0));
    d.hold_stick(0, (0, -30_000));
    d.hold_stick(0, (0, 0));
    d.hold_stick(0, (0, -30_000));
    assert_eq!(d.app.menu, 0);
    // A small push is nothing.
    d.hold_stick(0, (0, 12_000));
    assert_eq!(d.app.menu, 0);
}

#[test]
fn the_title_menu_with_touch_alone() {
    let mut d = Driver::new();
    let (x, y) = title_line(1);
    d.touch(x, y);
    assert_eq!(d.app.opts.mission, 1);
    let (x, y) = title_line(2);
    d.touch(x, y);
    d.touch(x, y);
    assert_eq!(d.app.opts.detail, Detail::Mid);
    let (x, y) = title_line(3);
    d.touch(x, y);
    assert_eq!(d.app.opts.eye, Eye::Wide);
    let (x, y) = title_line(4);
    d.touch(x, y);
    assert!(!d.app.opts.fast);
    // A tap beside the menu does nothing.
    d.touch(2, 30);
    assert_eq!(d.app.scene, Scene::Title);
    let (x, y) = title_line(0);
    d.touch(x, y);
    assert_eq!(d.app.scene, Scene::Brief);
    assert_eq!(d.app.world.mission, 1);
    // The briefing: a tap goes on once it is ready.
    d.wait_card();
    d.touch(80, 60);
    assert_eq!(d.app.scene, Scene::Play);
}

#[test]
fn leave_on_the_title_ends_the_game() {
    let mut d = Driver::new();
    let (x, y) = title_line(5);
    let down = Touch {
        x,
        y,
        down: true,
        started: true,
        ended: false,
    };
    let going = d.call_keys(Keys {
        touch: Some(down),
        ..Keys::default()
    });
    assert!(!going);
    assert_eq!(d.events.last(), Some(&Event::Quit));
    // With the pad: LEAVE is UP from PLAY, then A.
    let mut d = Driver::new();
    d.tap(button::UP);
    assert!(!d.hold(button::A));
}

#[test]
fn the_briefing_waits_then_a_starts_and_b_goes_back() {
    let mut d = Driver::new();
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Brief);
    d.tap(button::A);
    assert_eq!(
        d.app.scene,
        Scene::Brief,
        "too soon: the card takes no press yet"
    );
    d.tap(button::B);
    assert_eq!(d.app.scene, Scene::Title);
    d.tap(button::A);
    d.wait_card();
    d.tap(button::START);
    assert_eq!(d.app.scene, Scene::Play);
}

#[test]
fn start_pauses_and_the_world_stands_still() {
    let mut d = flying();
    d.idle(10);
    d.tap(button::START);
    assert_eq!(d.app.pause, Some(0));
    let t = d.app.world.ticks;
    let ship = d.app.world.ship;
    d.idle(100);
    assert_eq!(d.app.world.ticks, t);
    assert_eq!(d.app.world.ship, ship);
    assert_eq!(Kestrel::wanted(&d.app).0, rate::ON_DEMAND);
    d.tap(button::START);
    assert_eq!(d.app.pause, None);
    d.idle(5);
    assert!(d.app.world.ticks > t);
    // B and a tap beside the card resume too; so does RESUME.
    for how in 0..3 {
        d.tap(button::START);
        assert!(d.app.pause.is_some());
        match how {
            0 => d.tap(button::B),
            1 => d.touch(4, 50),
            _ => {
                let (x, y) = pause_line(0);
                d.touch(x, y);
            }
        }
        assert_eq!(d.app.pause, None, "way {how}");
    }
    // The II box pauses.
    let (x, y) = bottom_box(2);
    d.touch(x, y);
    assert_eq!(d.app.pause, Some(0));
}

#[test]
fn a_tap_on_the_pause_card_off_its_lines_does_not_resume() {
    let mut d = flying();
    d.tap(button::START);
    let (cx, cy, w, h) = hud::PAUSE_CARD;
    // The PAUSED heading, the card's top edge, the strip under the last
    // line, the bottom corners.
    for (x, y) in [
        (80, cy + 6),
        (80, cy),
        (80, cy + h as i16 - 2),
        (cx, cy + h as i16 - 1),
        (cx + w as i16 - 1, cy + h as i16 - 1),
    ] {
        assert_eq!(hud::pause_line_at(x, y), None, "({x}, {y}) is on no line");
        d.touch(x, y);
        assert_eq!(d.app.pause, Some(0), "a tap at ({x}, {y}) resumed");
    }
    // Just beside the card still resumes.
    d.touch(cx - 1, PAUSE_Y + 4);
    assert_eq!(d.app.pause, None);
}

/// Draws the app into a fresh frame.
fn picture(d: &mut Driver) -> Box<game_kit::Frame> {
    let mut f = Box::new(game_kit::Frame::new());
    d.app.draw(&mut f);
    f
}

#[test]
fn a_note_never_covers_the_pause_card() {
    for detail in Detail::ALL {
        let mut d = flying();
        d.app.opts.detail = detail;
        d.idle(3);
        d.tap(button::START);
        // VIEW steps by the pad: a note comes up, the detail stays.
        d.tap(button::DOWN);
        d.tap(button::DOWN);
        d.tap(button::RIGHT);
        assert!(d.app.note.is_some(), "{detail:?}: no note");
        assert_eq!(d.app.opts.detail, detail);
        let with = picture(&mut d);
        let note = d.app.note.take();
        let without = picture(&mut d);
        d.app.note = note;
        let (cx, cy, w, h) = hud::PAUSE_CARD;
        let mut differs = 0;
        for y in 0..120i16 {
            for x in 0..160i16 {
                let i = y as usize * 160 + x as usize;
                if with.0[i] == without.0[i] {
                    continue;
                }
                differs += 1;
                let on_card = (cx..cx + w as i16).contains(&x) && (cy..cy + h as i16).contains(&y);
                assert!(
                    !on_card,
                    "{detail:?}: the note changes ({x}, {y}) on the card"
                );
                assert!(
                    y >= scene::TOP_H as i16,
                    "{detail:?}: the note is on the top bar"
                );
            }
        }
        assert!(differs > 0, "{detail:?}: the note is not drawn");
    }
}

#[test]
fn the_pause_menu_changes_every_setting() {
    let mut d = flying();
    d.tap(button::START);
    d.tap(button::DOWN);
    d.tap(button::RIGHT);
    assert_eq!(d.app.opts.detail, Detail::Low);
    d.tap(button::LEFT);
    d.tap(button::LEFT);
    assert_eq!(d.app.opts.detail, Detail::Mid);
    d.tap(button::DOWN);
    d.tap(button::A);
    assert_eq!(d.app.opts.eye, Eye::Wide);
    d.tap(button::DOWN);
    d.tap(button::RIGHT);
    assert!(!d.app.opts.fast);
    assert!(
        d.app.pause.is_some(),
        "changing a setting keeps the menu open"
    );
    // By touch too.
    let (x, y) = pause_line(1);
    d.touch(x, y);
    assert_eq!(d.app.opts.detail, Detail::High);
    let (x, y) = pause_line(2);
    d.touch(x, y);
    assert_eq!(d.app.opts.eye, Eye::Nose);
    let (x, y) = pause_line(3);
    d.touch(x, y);
    assert!(d.app.opts.fast);
    assert!(d.events.iter().any(|e| matches!(e, Event::Settings { .. })));
    // RESTART: the same mission from its briefing, counted as a retry.
    d.idle(5);
    let (x, y) = pause_line(4);
    d.touch(x, y);
    assert_eq!(d.app.scene, Scene::Brief);
    assert_eq!(d.app.run.retries, 1);
    assert_eq!(d.app.world.ticks, 0);
    // TITLE.
    d.launch();
    d.tap(button::START);
    for _ in 0..5 {
        d.tap(button::DOWN);
    }
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Title);
}

#[test]
fn x_y_and_select_change_settings_while_flying() {
    let mut d = flying();
    let t = d.app.world.ticks;
    d.tap(button::X);
    assert_eq!(d.app.opts.detail, Detail::Low);
    assert_eq!(d.app.note.map(|n| n.0), Some("DETAIL: LOW"));
    d.tap(button::X);
    d.tap(button::X);
    assert_eq!(d.app.opts.detail, Detail::High);
    d.tap(button::Y);
    assert_eq!(d.app.opts.eye, Eye::Wide);
    d.tap(button::SELECT);
    assert!(!d.app.opts.fast);
    assert_eq!(Kestrel::wanted(&d.app), (rate::FPS30, hint::MOSTLY_STILL));
    assert!(d.app.world.ticks > t, "flying goes on");
    assert_eq!(d.app.pause, None);
    let settings = d
        .events
        .iter()
        .filter(|e| matches!(e, Event::Settings { .. }))
        .count();
    assert_eq!(settings, 5);
}

#[test]
fn the_pad_steers_and_sets_the_speed() {
    let d = flying();
    let pad_of = |k: Keys| d.app.controls(&k);
    let held = |b: u32| Keys {
        held: b,
        ..Keys::default()
    };
    assert_eq!(pad_of(Keys::default()).steer, (0, 0));
    assert_eq!(pad_of(held(button::RIGHT)).steer, (256, 0));
    assert_eq!(pad_of(held(button::LEFT)).steer, (-256, 0));
    assert_eq!(pad_of(held(button::UP)).steer, (0, 256));
    assert_eq!(pad_of(held(button::DOWN)).steer, (0, -256));
    // The stick: up reads minus; a small push is nothing.
    let stick = |x: i16, y: i16| Keys {
        left: (x, y),
        ..Keys::default()
    };
    assert_eq!(pad_of(stick(32_767, -32_767)).steer, (256, 256));
    assert_eq!(pad_of(stick(3_000, -3_000)).steer, (0, 0));
    let half = pad_of(stick(18_000, 0)).steer.0;
    assert!((100..160).contains(&half), "{half}");
    // Speed: A and the right trigger boost, B and the left one brake,
    // the right stick does both.
    assert_eq!(pad_of(held(button::A)).throttle, 256);
    assert_eq!(pad_of(held(button::B)).throttle, -256);
    let trig = |l: u16, r: u16| Keys {
        triggers: (l, r),
        ..Keys::default()
    };
    assert_eq!(pad_of(trig(0, 32_767)).throttle, 256);
    assert_eq!(pad_of(trig(32_767, 0)).throttle, -256);
    let right = |y: i16| Keys {
        right: (0, y),
        ..Keys::default()
    };
    assert_eq!(pad_of(right(-32_767)).throttle, 256);
    assert_eq!(pad_of(right(32_767)).throttle, -256);
}

#[test]
fn l_r_and_a_right_stick_flick_roll() {
    let mut d = flying();
    d.tap(button::R);
    d.idle(2);
    assert_eq!(d.app.world.tally.rolls, 1);
    assert_eq!(d.app.world.ship.roll_dir, 1);
    d.idle(40);
    d.tap(button::L);
    d.idle(2);
    assert_eq!(d.app.world.tally.rolls, 2);
    assert_eq!(d.app.world.ship.roll_dir, -1);
    d.idle(40);
    d.call_keys(Keys {
        right: (32_000, 0),
        ..Keys::default()
    });
    d.idle(2);
    assert_eq!(d.app.world.tally.rolls, 3);
    assert_eq!(d.app.world.ship.roll_dir, 1);
}

#[test]
fn the_touch_boxes_roll_and_set_the_speed() {
    let mut d = flying();
    let (x, y) = bottom_box(0);
    d.touch(x, y);
    d.idle(1);
    assert_eq!(d.app.world.tally.rolls, 1);
    assert_eq!(d.app.world.ship.roll_dir, -1);
    d.idle(40);
    let (x, y) = bottom_box(4);
    d.touch(x, y);
    d.idle(1);
    assert_eq!(d.app.world.ship.roll_dir, 1);
    // FAST, then FAST again back to cruise; SLOW, then FAST.
    let (fx, fy) = bottom_box(3);
    let (sx, sy) = bottom_box(1);
    d.touch(fx, fy);
    d.idle(1);
    assert_eq!(d.app.last_pad.throttle, 256);
    d.touch(fx, fy);
    d.idle(1);
    assert_eq!(d.app.last_pad.throttle, 0);
    d.touch(sx, sy);
    d.idle(1);
    assert_eq!(d.app.last_pad.throttle, -256);
    d.touch(fx, fy);
    d.idle(1);
    assert_eq!(d.app.last_pad.throttle, 256);
    // A pad button wins over the boxes while held.
    d.hold(button::B);
    d.hold(button::B);
    assert_eq!(d.app.last_pad.throttle, -256);
}

#[test]
fn a_finger_on_the_view_steers_the_craft_towards_it() {
    for detail in Detail::ALL {
        for eye in Eye::ALL {
            let mut d = Driver::new();
            d.app.opts.detail = detail;
            d.app.opts.eye = eye;
            d.play_from_title();
            d.idle(3);
            let (px, py) = scene::ship_on_screen(&d.app.world, detail, eye);
            let (px, py) = (px as i16, py as i16);
            let mut steer_at = |x: i16, y: i16| {
                finger(&mut d, x, y, true);
                finger(&mut d, x, y, false);
                d.app.last_pad.steer
            };
            let r = steer_at(px + 30, py);
            assert!(
                r.0 == 256 && r.1.abs() < 40,
                "{detail:?} {eye:?}: right {r:?}"
            );
            let l = steer_at(px - 6, py);
            assert!(
                l.0 < -30 && l.0 > -100,
                "{detail:?} {eye:?}: a little left {l:?}"
            );
            let u = steer_at(px, py - 20);
            assert!(u.1 > 150, "{detail:?} {eye:?}: up {u:?}");
            let dn = steer_at(px, py + 20);
            assert!(dn.1 < -150, "{detail:?} {eye:?}: down {dn:?}");
            // Lifting the finger stops the steering.
            d.idle(1);
            assert_eq!(d.app.last_pad.steer, (0, 0));
        }
    }
}

#[test]
fn the_rate_the_game_asks_for() {
    let mut d = Driver::new();
    assert_eq!(Kestrel::wanted(&d.app).0, rate::FPS60);
    d.tap(button::A);
    assert_eq!(
        Kestrel::wanted(&d.app).0,
        rate::FPS60,
        "the card is waiting"
    );
    d.wait_card();
    assert_eq!(
        Kestrel::wanted(&d.app).0,
        rate::ON_DEMAND,
        "a ready card is still"
    );
    d.tap(button::A);
    assert_eq!(Kestrel::wanted(&d.app).0, rate::FPS60);
    d.app.opts.fast = false;
    assert_eq!(Kestrel::wanted(&d.app).0, rate::FPS30);
    d.tap(button::START);
    d.idle(100);
    assert_eq!(Kestrel::wanted(&d.app).0, rate::ON_DEMAND);
    for _ in 0..3 {
        assert_eq!(Kestrel::wanted(&d.app).1, hint::MOSTLY_STILL);
    }
}

#[test]
fn a_long_gap_between_calls_is_not_caught_up() {
    let mut d = flying();
    let t = d.app.world.ticks;
    d.now_us += 5_000_000;
    d.idle(1);
    assert!(
        u64::from(d.app.world.ticks - t) <= MAX_CATCH_UP,
        "{} ticks in one call",
        d.app.world.ticks - t
    );
}

/// Ends the mission now by running the clock out.
fn fail(d: &mut Driver) {
    d.app.world.clock = 1;
    d.idle(1);
    assert_eq!(d.app.scene, Scene::Result);
    assert_eq!(d.app.world.state, State::OutOfTime);
}

#[test]
fn the_failed_card_by_pad_and_by_touch() {
    let mut d = flying();
    fail(&mut d);
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Result, "too soon");
    d.wait_card();
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::Brief);
    assert_eq!(d.app.run.retries, 1);
    d.launch();
    fail(&mut d);
    d.wait_card();
    d.tap(button::B);
    assert_eq!(d.app.scene, Scene::Title);
    // Touch: TRY AGAIN and TITLE boxes; a tap elsewhere does nothing.
    d.play_from_title();
    fail(&mut d);
    d.wait_card();
    d.touch(5, 20);
    assert_eq!(d.app.scene, Scene::Result);
    d.touch(50, hud::FAIL_BOXES_Y + 4);
    assert_eq!(d.app.scene, Scene::Brief);
    d.launch();
    fail(&mut d);
    d.wait_card();
    d.touch(110, hud::FAIL_BOXES_Y + 4);
    assert_eq!(d.app.scene, Scene::Title);
}

/// The autopilot's wishes as a finger: on the view, `steer` away from
/// the craft, or a tap on a speed box when the speed should change.
fn touch_call(d: &mut Driver, p: &Plan, down: &mut bool) {
    let want = p.speed;
    if want != d.app.touch_throttle {
        // FAST toggles fast on (or off when on), SLOW the same for slow.
        let b = match (want, d.app.touch_throttle) {
            (1, _) | (0, 1) => 3,
            _ => 1,
        };
        let (x, y) = bottom_box(b);
        finger(d, x, y, true);
        *down = false;
        return;
    }
    let (detail, eye) = (d.app.opts.detail, d.app.opts.eye);
    let (px, py) = scene::ship_on_screen(&d.app.world, detail, eye);
    let x = px + p.steer.0 * 24 / 256;
    let y = py - p.steer.1 * 24 / 256;
    let a = if detail == Detail::Low {
        (0, 10, 160, BOTTOM_Y)
    } else {
        let r = detail.area();
        (r.x0, r.y0, r.x1, r.y1)
    };
    let x = x.clamp(a.0, a.2 - 1) as i16;
    let y = y.clamp(a.1, a.3 - 1) as i16;
    finger(d, x, y, !*down);
    *down = true;
}

#[test]
fn touch_alone_finishes_all_three_missions() {
    let mut out = String::new();
    for (detail, eye) in [(Detail::Low, Eye::Chase), (Detail::Mid, Eye::Nose)] {
        let mut d = Driver::new();
        // Settings by touch.
        while d.app.opts.detail != detail {
            let (x, y) = title_line(2);
            d.touch(x, y);
        }
        while d.app.opts.eye != eye {
            let (x, y) = title_line(3);
            d.touch(x, y);
        }
        let (x, y) = title_line(0);
        d.touch(x, y);
        for m in 0..MISSIONS.len() {
            assert_eq!(d.app.scene, Scene::Brief);
            d.wait_card();
            d.touch(80, 60);
            assert_eq!(d.app.scene, Scene::Play);
            let mut down = false;
            let mut calls = 0;
            while d.app.scene == Scene::Play && calls < 4000 {
                let p = plan(&d.app.world);
                touch_call(&mut d, &p, &mut down);
                calls += 1;
            }
            let w = &d.app.world;
            out += &format!(
                "touch only, {detail:?} {eye:?}, mission {}: {:?} in {calls} calls, gates {}/{}, hits {}, time left {} s, score {}\n",
                m + 1,
                w.state,
                w.tally.passed,
                w.m().gates,
                w.tally.hits,
                w.clock_s10() / 10,
                w.score
            );
            assert_eq!(w.state, State::Delivered, "{out}");
            d.wait_card();
            d.touch(80, 60);
        }
        assert_eq!(d.app.scene, Scene::End);
        d.touch(80, 60);
        assert_eq!(d.app.scene, Scene::Title);
    }
    print!("{out}");
    let dir = common::evidence("");
    std::fs::write(dir.join("touch-only.txt"), &out).unwrap();
}

#[test]
fn the_end_card_goes_back_to_the_title() {
    let mut d = Driver::new();
    d.app.opts.mission = 2;
    d.app.world.reset(2);
    d.play_from_title();
    // Deliver mission 3 by putting the craft just before the dock.
    let w = &mut d.app.world;
    w.next_gate = w.m().gates;
    let g = w.m().gate(w.next_gate, w.ticks);
    w.ship.x = g.x << 4;
    w.ship.y = g.y << 4;
    w.ship.z = (g.z << 4) - 16;
    d.idle(1);
    assert_eq!(d.app.scene, Scene::Result);
    assert_eq!(d.app.world.state, State::Delivered);
    d.wait_card();
    d.tap(button::A);
    assert_eq!(d.app.scene, Scene::End);
    assert!(d.events.iter().any(|e| matches!(e, Event::End { .. })));
    d.idle(CARD_WAIT);
    d.tap(button::START);
    assert_eq!(d.app.scene, Scene::Title);
}
