//! Every control, one at a time: what it does to the craft compared with
//! doing nothing. Each test starts the same flight (level 1) so the
//! results can be compared exactly.

mod common;

use common::{BOARD_TRIGGER, Pilot, Run, hold};
use game_kit::{Touch, button};
use lander::app::{Event, Input, NOTE_TICKS, Scene, Steer, View, tilt_lean};
use lander::craft::{Craft, FULL};
use lander::fixed::STEP;

/// Level 1, 10 calls of `inp` (20 physics ticks), then the craft.
fn after(inp: Input) -> Craft {
    after_n(inp, 10, |_| {})
}

fn after_n(inp: Input, calls: u32, setup: impl FnOnce(&mut Run)) -> Craft {
    let mut r = Run::new();
    setup(&mut r);
    r.play();
    for _ in 0..calls {
        r.call(&inp);
    }
    assert_eq!(r.app.scene, Scene::Fly);
    r.app.craft
}

fn triggers(l: u16, r: u16) -> Input {
    Input {
        triggers: (l, r),
        ..Input::default()
    }
}

fn left(x: i16, y: i16) -> Input {
    Input {
        left: (x, y),
        ..Input::default()
    }
}

#[test]
fn a_and_up_and_a_full_trigger_fire_the_engine_at_full_power() {
    let idle = after(Input::default());
    let a = after(hold(button::A));
    assert!(a.vy < idle.vy && a.fuel < idle.fuel);
    assert_eq!(after(hold(button::UP)), a);
    // A Switch-mode trigger reads 0 or full: full is the same as A.
    assert_eq!(after(triggers(0, FULL as u16)), a);
    // The board's 1023 scales to the same full value.
    let board_full = Pilot {
        trigger: BOARD_TRIGGER as u16,
        ..Pilot::default()
    };
    assert_eq!(after(board_full.input()), a);
}

#[test]
fn the_right_trigger_is_a_throttle() {
    let idle = after(Input::default());
    let quarter = after(triggers(0, FULL as u16 / 4));
    let half = after(triggers(0, FULL as u16 / 2));
    let full = after(triggers(0, FULL as u16));
    assert!(idle.vy > quarter.vy && quarter.vy > half.vy && half.vy > full.vy);
    assert!(idle.fuel > quarter.fuel && quarter.fuel > half.fuel && half.fuel > full.fuel);
}

#[test]
fn the_left_trigger_gives_half_power_for_on_off_triggers() {
    let half = after(triggers(0, FULL as u16 / 2));
    assert_eq!(after(triggers(FULL as u16, 0)), half);
    // Both held: the stronger wins.
    assert_eq!(
        after(triggers(FULL as u16, FULL as u16)),
        after(hold(button::A))
    );
}

#[test]
fn the_left_stick_up_is_a_throttle_too() {
    let up = after(left(0, -32767));
    assert_eq!(up, after(hold(button::A)));
    // Inside the dead zone it does nothing; down does nothing.
    assert_eq!(after(left(0, -3000)), after(Input::default()));
    assert_eq!(after(left(0, 32767)), after(Input::default()));
}

#[test]
fn sticks_dpad_and_shoulders_turn_the_craft() {
    let idle = after(Input::default());
    assert_eq!(idle.angle, 0);
    let stick_r = after(left(32767, 0));
    let stick_l = after(left(-32767, 0));
    assert!(stick_r.angle > 0 && stick_l.angle == -stick_r.angle);
    // A half push turns about half as fast.
    let stick_half = after(left(18000, 0));
    assert!(stick_half.angle > 0 && stick_half.angle < stick_r.angle * 2 / 3);
    // The right stick, the d-pad: the same full turn.
    let right_stick = after(Input {
        right: (32767, 0),
        ..Input::default()
    });
    assert_eq!(right_stick.angle, stick_r.angle);
    assert_eq!(after(hold(button::RIGHT)).angle, stick_r.angle);
    assert_eq!(after(hold(button::LEFT)).angle, stick_l.angle);
    // The shoulders: a slow, fine turn (a quarter).
    let r = after(hold(button::R)).angle;
    let l = after(hold(button::L)).angle;
    assert!(r > 0 && r < stick_r.angle / 3 && l == -r);
    // The engine pushes the way the craft leans.
    let lean_r = after_n(hold(button::RIGHT | button::A), 20, |_| {});
    let lean_l = after_n(hold(button::LEFT | button::A), 20, |_| {});
    assert!(lean_r.vx > lean_l.vx);
}

#[test]
fn the_craft_leans_a_quarter_turn_at_most() {
    let c = after_n(hold(button::RIGHT), 60, |_| {});
    assert_eq!(c.angle, 64 * STEP);
}

#[test]
fn b_brings_the_craft_back_upright() {
    let mut r = Run::new();
    r.play();
    for _ in 0..8 {
        r.call(&hold(button::RIGHT));
    }
    assert!(r.app.craft.angle > 10 * STEP);
    for _ in 0..30 {
        r.call(&hold(button::B));
    }
    assert_eq!(r.app.craft.angle, 0);
}

#[test]
fn tilt_steering_follows_the_pad_only_when_chosen_and_sent() {
    let tilted = Input {
        tilt: Some(-2048), // right side down, 30 degrees
        ..Input::default()
    };
    let want = tilt_lean(-2048);
    assert!(want > 0);
    let tilt = |r: &mut Run| r.app.opts.steer = Steer::Tilt;
    let c = after_n(tilted, 30, tilt);
    assert_eq!(c.angle, want);
    // Stick mode ignores the motion.
    assert_eq!(after_n(tilted, 30, |_| {}).angle, 0);
    // Tilt mode without motion from the pad: the stick still steers.
    let c = after_n(left(32767, 0), 10, tilt);
    assert_eq!(c.angle, after(left(32767, 0)).angle);
    // Small wobbles count as level.
    assert_eq!(tilt_lean(150), 0);
    // The stick wins over the tilt while it is pushed.
    let both = Input {
        left: (-32767, 0),
        ..tilted
    };
    assert!(after_n(both, 10, tilt).angle < 0);
}

#[test]
fn a_finger_leans_the_craft_towards_it_and_fires() {
    let mut r = Run::new();
    r.play();
    let (sx, sy) = r.app.to_screen(r.app.craft.x, r.app.craft.y);
    let fuel = r.app.craft.fuel;
    // A finger up and to the right of the craft.
    let finger = Input {
        touch: Some(Touch {
            x: (sx + 30) as i16,
            y: (sy - 30) as i16,
            down: true,
            started: false,
            ended: false,
        }),
        ..Input::default()
    };
    for _ in 0..15 {
        r.call(&finger);
    }
    let c = r.app.craft;
    assert!(c.fuel < fuel);
    // Leaning right, towards 45 degrees (32 steps).
    assert!(
        c.angle > 20 * STEP && c.angle <= 32 * STEP,
        "{}",
        c.angle / STEP
    );
    // Lifted: the engine stops.
    let fuel = c.fuel;
    r.call(&Input::default());
    assert_eq!(r.app.craft.fuel, fuel);
}

#[test]
fn select_switches_the_view_in_flight() {
    let mut r = Run::new();
    r.play();
    r.tap(button::SELECT);
    assert_eq!(r.app.opts.view, View::Follow);
    assert!(r.events.contains(&Event::View(View::Follow)));
    assert_eq!(r.app.note.map(|n| n.0), Some("VIEW: FOLLOW"));
    r.idle(NOTE_TICKS / 2 + 1);
    assert_eq!(r.app.note, None);
    r.tap(button::SELECT);
    assert_eq!(r.app.opts.view, View::Still);
}

#[test]
fn the_engine_stops_when_the_fuel_runs_out() {
    let mut r = Run::new();
    r.play();
    r.app.craft.fuel = 64 * 3; // three ticks at full power
    r.call(&hold(button::A));
    r.call(&hold(button::A));
    assert_eq!(r.app.craft.fuel, 0);
    assert_eq!(r.app.note.map(|n| n.0), Some("NO FUEL"));
    // Now A does nothing: no flame, and the craft speeds up downwards.
    let vy = r.app.craft.vy;
    r.call(&hold(button::A));
    assert_eq!(r.app.craft.burn, 0);
    assert!(r.app.craft.vy > vy);
}
