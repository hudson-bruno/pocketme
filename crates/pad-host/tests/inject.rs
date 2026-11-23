//! Injected input (testing without a pad), the stats counters and the
//! one-word state, against the scripted controller and gamepad.

mod common;

use board_api::{PadState, btn};
use common::fixtures;
use common::*;
use pad_host::inject::{self, SwitchInput};
use pad_host::{Mode, PadEvent};

/// A Switch report with `buttons` held (and gravity on z, so motion is
/// on).
fn sw(buttons: u32) -> Vec<u8> {
    inject::switch_report(&SwitchInput {
        buttons,
        accel: [0, 0, 4096],
        battery: 4,
        ..Default::default()
    })
    .to_vec()
}

fn inject(s: &mut Sim, r: &[u8]) {
    assert!(s.host.inject_report(s.now, r, &mut s.radio));
}

/// An injected Switch pad is seen by a game exactly like a real one: it
/// connects (event, wake), its buttons, sticks and motion read through,
/// and it goes (event, NONE).
#[test]
fn an_injected_switch_pad_reads_like_a_real_one() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    assert_eq!(s.host.state(), "waiting");
    assert!(s.host.inject_begin(s.now, Mode::Switch, &mut s.radio));
    assert!(s.host.injecting());
    assert_eq!(s.host.read(), PadState::NONE, "nothing before a report");
    let r = inject::switch_report(&SwitchInput {
        buttons: btn::EAST | btn::UP,
        left: (32767, 0),
        accel: [100, 200, 4096],
        gyro: [1000, -2000, 50],
        battery: 3,
        charging: true,
        ..Default::default()
    });
    inject(&mut s, &r);
    assert!(s.tick(), "a new pad wakes the console");
    assert_eq!(s.host.state(), "ready");
    assert_eq!(s.host.mode(), Mode::Switch);
    let p = s.host.read();
    assert!(p.connected);
    assert_eq!(p.buttons, btn::EAST | btn::UP);
    assert_eq!(p.left.x, 32767);
    assert_eq!(p.left.y, 0);
    let m = s.host.motion().expect("motion");
    assert_eq!(m.accel, [-200, 100, 4096]);
    assert_eq!(m.gyro, [2240, 1120, 56]);
    assert_eq!(
        s.host.battery().map(|b| (b.level, b.charging)),
        Some((3, true))
    );
    let ev = s.take_events();
    assert!(
        matches!(
            ev[0],
            PadEvent::Connected {
                mode: Mode::Switch,
                ..
            }
        ),
        "{ev:?}"
    );
    assert_eq!(s.host.take_pressed(), btn::EAST | btn::UP);
    // Nothing went to the radio.
    let sent = s.radio.out.len();
    inject(&mut s, &sw(0));
    s.tick();
    assert_eq!(s.radio.out.len(), sent);
    let ev = s.take_events();
    assert!(
        matches!(ev[..], [PadEvent::Battery(b)] if b.level == 4),
        "{ev:?}"
    );
    // It goes.
    s.host.inject_end(s.now, 0x13, &mut s.radio);
    assert!(!s.host.injecting());
    assert!(s.tick());
    assert_eq!(s.host.read(), PadState::NONE);
    assert_eq!(s.host.motion(), None);
    assert_eq!(
        s.take_events(),
        vec![PadEvent::Disconnected { reason: 0x13 }]
    );
    assert_eq!(s.host.state(), "waiting");
    assert!(!s.host.inject_report(s.now, &sw(0), &mut s.radio));
    let c = s.host.counters();
    assert_eq!(
        (c.injected, c.attempts, c.ready, c.lost),
        (2, 0, 0, 0),
        "{c:?}"
    );
    s.assert_no_full_address();
}

/// A press down in one injected report and up in the next, both before
/// the console looks, is seen once.
#[test]
fn a_short_injected_press_counts_once() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.host.inject_begin(s.now, Mode::Switch, &mut s.radio);
    inject(&mut s, &sw(0));
    s.tick();
    s.host.take_pressed();
    inject(&mut s, &sw(btn::HOME));
    inject(&mut s, &sw(0));
    assert_eq!(s.host.read().buttons, 0);
    assert!(
        s.tick(),
        "the press wakes the console although it is up again"
    );
    assert_eq!(s.host.take_pressed(), btn::HOME);
    assert_eq!(s.host.take_pressed(), 0);
}

/// 100 reports a second of a pad lying still (counter, stick wobble,
/// motion noise from `inject::resting`) never wake the console; a
/// button does.
#[test]
fn resting_injected_reports_do_not_wake() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.host.inject_begin(s.now, Mode::Switch, &mut s.radio);
    let base = sw(0);
    inject(&mut s, &base);
    s.tick();
    s.wakes.clear();
    let mut out = [0_u8; 64];
    for n in 1..=300 {
        let len = inject::resting(&base, n, &mut out);
        inject(&mut s, &out[..len]);
        s.now += 10;
        s.tick();
    }
    assert!(s.wakes.is_empty(), "woke at {:?}", s.wakes);
    assert_eq!(s.host.counters().gap_max_ms, 10);
    inject(&mut s, &sw(btn::SOUTH));
    assert!(s.tick());
}

/// The other modes the GuliKit has that need no descriptor.
#[test]
fn xinput_and_android_can_be_injected_dinput_cannot() {
    for (mode, want) in [(Mode::XInput, Mode::XInput), (Mode::Android, Mode::Android)] {
        let mut s = Sim::new(&[(PAD, KEY)]);
        s.until_waiting();
        assert!(s.host.inject_begin(s.now, mode, &mut s.radio));
        // An Xbox report 0x01 with A (bit 0 of byte 14 in pad-view's
        // reading) is not needed here: the mode is chosen by the name.
        assert_eq!(s.host.mode(), want);
        assert_eq!(s.host.motion(), None);
    }
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    assert!(!s.host.inject_begin(s.now, Mode::DInput, &mut s.radio));
    assert!(!s.host.injecting());
}

/// Injection never gets in a real pad's way: refused while a pad is
/// linked, and ended at once when a saved pad calls (which then
/// connects as always).
#[test]
fn a_real_pad_wins_over_injection() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    assert!(s.host.inject_begin(s.now, Mode::Switch, &mut s.radio));
    inject(&mut s, &sw(btn::SOUTH));
    s.tick();
    s.take_events();
    assert!(s.pad_calls(PAD, PAD_COD));
    assert!(!s.host.injecting(), "{}", s.tail_log());
    assert!(s.log_has("injection ends"));
    assert_eq!(
        s.take_events(),
        vec![PadEvent::Disconnected { reason: 0x16 }]
    );
    assert_eq!(s.host.read(), PadState::NONE);
    assert!(
        !s.host.inject_begin(s.now, Mode::Switch, &mut s.radio),
        "linked"
    );
    s.pad_uses_saved_key(PAD);
    let (_, intr) = s.pad_connects_hid(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
    assert!(!s.host.inject_begin(s.now, Mode::Switch, &mut s.radio));
}

/// The one-word state through a whole reconnect, and the counters.
#[test]
fn state_words_and_counters_through_a_reconnect() {
    assert_eq!(pad_host::PadHost::new().state(), "off", "not started");
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    assert_eq!(s.host.state(), "waiting");
    assert!(s.pad_calls(PAD, PAD_COD));
    assert_eq!(s.host.state(), "connecting");
    s.pad_uses_saved_key(PAD);
    let (_, intr) = s.pad_connects_hid(&fixtures::SWITCH_WORKS);
    assert_eq!(s.host.state(), "starting");
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert_eq!(s.host.state(), "ready");
    let c = s.host.counters();
    assert_eq!((c.attempts, c.ready, c.lost, c.failed), (1, 1, 0, [0; 4]));
    assert!(c.reports > 100 && c.undecoded == 0, "{c:?}");
    assert!(c.gap_max_ms > 0 && c.gap_max_ms < 100, "{c:?}");
    s.feed(&ev_disconnected(H, 0x13));
    s.tick();
    assert_eq!(s.host.state(), "waiting");
    assert_eq!(s.host.counters().lost, 1);

    // A call that never gets its key: failed while connecting (security).
    s.advance(1_100);
    assert!(s.pad_calls(PAD, PAD_COD));
    s.advance(60_000);
    assert!(s.host.waiting(), "{}", s.tail_log());
    let c = s.host.counters();
    assert_eq!(c.attempts, 2);
    assert_eq!(c.failed, [0, 1, 0, 0], "{c:?}");

    // A stranger is refused while waiting: not an attempt.
    assert!(!s.pad_calls(STRANGER, PAD_COD));
    assert_eq!(s.host.counters().attempts, 2);
    s.host.reset_counters();
    assert_eq!(s.host.counters(), pad_host::Counters::default());
}

/// The board can check what it sends against the saved pads: their
/// addresses, oldest first, and nothing past them.
#[test]
fn the_saved_addresses_are_given_to_the_board() {
    let s = Sim::new(&[(PAD, KEY)]);
    assert_eq!(s.host.saved_address(0), Some(PAD.0));
    assert_eq!(s.host.saved_address(1), None);
    assert_eq!(s.host.saved_address(9), None);
    let none = Sim::new(&[]);
    assert_eq!(none.host.saved_address(0), None);
}

/// The start-up log of the reconnect-only host never speaks of calling
/// anybody (round 6: it said "any gamepad that answers is called").
#[test]
fn the_start_log_never_says_it_calls() {
    let s = Sim::new(&[(PAD, KEY)]);
    assert!(s.log_has("the saved pads are the list"), "{}", s.tail_log());
    assert!(!s.log_has("is called"), "{}", s.tail_log());
}
