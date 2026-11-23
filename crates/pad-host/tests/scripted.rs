//! The pad host against a scripted controller and gamepad. The gamepad's
//! SDP answers, reports and the outputs it was sent are the owner's live
//! logs (see `common/fixtures.rs`); the HCI and L2CAP steps around them
//! are written from the specs, in the order the logs show them.

mod common;

use board_api::{PadState, btn};
use common::fixtures::{self, Session};
use common::*;
use pad_host::{Battery, Mode, PadEvent, PadHost, map};
use pad_proto::hci::op;

fn i16le(r: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([r[at], r[at + 1]])
}

/// A saved pad's whole reconnect in Switch mode, to the end of the
/// start-up talk. Returns the pad and (control, interrupt) channels.
fn switch_connected(s: &mut Sim) -> (SwitchPad, u16, u16) {
    s.until_waiting();
    let (ctrl, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    (pad, ctrl, intr)
}

/// From "waiting" to buttons, motion and battery, with the live log of
/// round 5 (live3, the link that worked) as the source of truth: the four
/// output reports must be the logged ones byte for byte, the pad's logged
/// answers must finish the start-up talk, and its logged full reports
/// must give the motion and battery.
#[test]
fn switch_reconnect_from_waiting_to_buttons_motion_and_battery() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    assert_eq!(s.host.phase(), "waiting");
    assert_eq!(s.host.saved_pads(), 1);
    assert!(s.log_has("pairings read (read only): found=1"));
    let (ctrl, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let _ = ctrl;
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);

    // The outputs, as logged.
    let logged: Vec<Vec<u8>> = fixtures::SWITCH_WORKS
        .outputs
        .iter()
        .map(|o| hex(o.strip_prefix("output:").unwrap()))
        .collect();
    assert_eq!(logged.len(), 4);
    assert!(pad.outputs.len() >= 4, "{}", s.tail_log());
    for (k, want) in logged.iter().enumerate() {
        assert_eq!(&pad.outputs[k][..want.len()], &want[..], "output {k}");
    }
    assert_eq!(pad.outputs.len(), 4, "nothing more once the talk is done");
    assert!(pad.full && pad.motion);

    // Connected, in Switch mode, with the name the pad gave.
    let ev = s.take_events();
    let PadEvent::Connected { mode, name } = ev[0] else {
        panic!("{ev:?}")
    };
    assert_eq!(mode, Mode::Switch);
    assert_eq!(name.as_str(), "Pro Controller");
    assert_eq!(s.host.mode(), Mode::Switch);
    // Battery: report byte 2 is 0x80 in the log: full, not charging.
    let full = Battery {
        percent: 100,
        charging: false,
        level: 4,
    };
    assert!(ev.contains(&PadEvent::Battery(full)), "{ev:?}");
    assert_eq!(s.host.battery(), Some(full));

    // Motion: the newest of the three samples of the last report, turned
    // into the console's axes and units.
    let last = pad_last_report(&mut s, &mut pad, intr);
    let m = s.host.motion().expect("motion in Switch mode");
    let (ax, ay, az) = (i16le(&last, 37), i16le(&last, 39), i16le(&last, 41));
    let (gx, gy, gz) = (i16le(&last, 43), i16le(&last, 45), i16le(&last, 47));
    assert_eq!(m.accel, [-ay, ax, az]);
    let g = |v: i16| (v as i32 * 112 / 100) as i16;
    assert_eq!(m.gyro, [-g(gy), g(gx), g(gz)]);
    // Held still (the log's pad was tilted): about 1 g in all.
    let g2: i64 = m.accel.iter().map(|&a| i64::from(a) * i64::from(a)).sum();
    assert!((3500 * 3500..4700 * 4700).contains(&g2), "{m:?}");

    // Sticks at rest: inside the dead zone.
    let st = s.host.read();
    assert!(st.connected);
    assert_eq!(st.buttons, 0);
    assert_eq!((st.left.x, st.left.y, st.right.x, st.right.y), (0, 0, 0, 0));
    assert_eq!(st.lt, 0);

    // A (byte 3 bit 3) and ZL (byte 5 bit 7).
    pad.buttons = [0x08, 0, 0x80];
    s.switch_runs(&mut pad, intr, 50);
    let st = s.host.read();
    assert_eq!(st.buttons, btn::EAST | btn::ZL);
    assert_eq!(st.lt, board_api::input::TRIGGER_MAX, "ZL is a full trigger");
    assert_eq!(s.host.take_pressed(), btn::EAST | btn::ZL);
    assert_eq!(s.host.take_pressed(), 0, "seen once");

    // The owner's end test reads these lines.
    for line in [
        "PAD-HOST pairings read (read only)",
        "PAD-HOST stage: Waiting for a saved pad",
        "PAD-HOST connected mode=switch layout=",
        "PAD-HOST switch start-up:",
        "PAD-HOST motion on",
        "PAD-HOST battery percent=100 level=4 charging=0",
    ] {
        assert!(s.log_has(line), "no line {line:?}\n{}", s.tail_log());
    }
    assert_nothing_forbidden(&s);
    s.assert_key_never_printed();
    s.assert_no_full_address();
}

/// Sends one more report and returns it.
fn pad_last_report(s: &mut Sim, pad: &mut SwitchPad, intr: u16) -> Vec<u8> {
    let r = pad.next_report();
    s.report(intr, &r);
    s.advance(10);
    r
}

/// live3: the first link opened HID and SDP and then the pad hung up
/// (0x13) before any report. The host comes back to waiting, says why,
/// and the second link (the next fixture) works.
#[test]
fn a_reconnect_that_fails_once_then_works() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.reconnect(&fixtures::SWITCH_FAILS);
    s.advance(200);
    s.feed(&ev_disconnected(
        H,
        fixtures::SWITCH_FAILS.end_reason.unwrap(),
    ));
    s.advance(1_100);
    assert!(s.host.waiting(), "{}", s.tail_log());
    assert!(!s.host.connected());
    assert!(
        s.take_events().is_empty(),
        "no Connected, so no Disconnected"
    );
    assert!(s.log_has("PAD-HOST gone reason=0x13"), "{}", s.tail_log());

    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
    assert!(s.host.motion().is_some());
    assert_eq!(pad.outputs.len(), 4);
    assert_nothing_forbidden(&s);
}

/// live3: the pad's first link after it was switched on ended (0x13)
/// before any report, and it called again within the second. The host
/// must take that second call at once, not only after its pause.
#[test]
fn the_pad_calling_again_at_once_after_a_failed_start_is_taken() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.reconnect(&fixtures::SWITCH_FAILS);
    s.advance(200);
    s.feed(&ev_disconnected(
        H,
        fixtures::SWITCH_FAILS.end_reason.unwrap(),
    ));
    s.advance(30);
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    assert_eq!(pad.outputs.len(), 4);
    assert_nothing_forbidden(&s);
}

/// The console reads the radio between frames, not at once as the test
/// firmware did: every packet from the pad here waits 40 ms (longer than
/// the slowest frame measured) before the host sees it. The reconnect,
/// the start-up talk, motion and battery must all still work, with no
/// try given up.
#[test]
fn a_reconnect_works_when_every_packet_waits_a_frame() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.feed_delay_ms = 40;
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    assert_eq!(pad.outputs.len(), 4, "{}", s.tail_log());
    assert!(s.host.motion().is_some());
    assert!(s.host.battery().is_some());
    assert_eq!(s.host.counters().failed, [0; 4], "{}", s.tail_log());
    assert_nothing_forbidden(&s);
}

/// A call from an address with no saved key is refused ("unacceptable
/// address", 0x0F) and its address is not logged.
#[test]
fn an_unknown_address_is_refused() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.cmds.clear();
    assert!(!s.pad_calls(STRANGER, PAD_COD));
    assert_eq!(
        s.take_cmd(op::REJECT_CONNECTION_REQUEST),
        cat(&[&STRANGER.0, &[0x0F]])
    );
    // A phone, too.
    s.feed(&ev_conn_request(STRANGER, 0x5A020C));
    s.take_cmd(op::REJECT_CONNECTION_REQUEST);
    s.advance(2_000);
    assert!(s.host.waiting());
    assert!(!s.sent(op::ACCEPT_CONNECTION_REQUEST));
    assert!(s.take_events().is_empty());
    assert_nothing_forbidden(&s);
    s.assert_no_full_address();
}

/// No pairings saved at all: every call is refused.
#[test]
fn with_nothing_saved_every_call_is_refused() {
    let mut s = Sim::with_flash(Ram::blank());
    s.until_waiting();
    assert_eq!(s.host.saved_pads(), 0);
    assert!(s.log_has("found=0 blank=1"), "{}", s.tail_log());
    s.cmds.clear();
    assert!(!s.pad_calls(PAD, PAD_COD));
    s.take_cmd(op::REJECT_CONNECTION_REQUEST);
    assert_nothing_forbidden(&s);
}

/// The saved pad calls but starts a new pairing (it lost our key): the
/// pairing is refused, the link ends with "authentication failure", the
/// key stays, and the host waits again.
#[test]
fn a_pairing_attempt_is_refused() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    assert!(s.pad_calls(PAD, PAD_COD));
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_addr(0x31, PAD)); // IO capability request: pairing
    assert_eq!(
        s.take_cmd(op::IO_CAPABILITY_REQUEST_NEGATIVE_REPLY),
        cat(&[&PAD.0, &[0x18]])
    );
    assert_eq!(
        s.take_cmd(op::DISCONNECT),
        cat(&[&H.to_le_bytes(), &[0x05]])
    );
    s.feed(&ev_addr(0x16, PAD)); // PIN code request: also refused
    s.take_cmd(op::PIN_CODE_REQUEST_NEGATIVE_REPLY);
    s.feed(&ev_link_key(PAD, [0x77; 16])); // a new key: ignored
    s.advance(1_100);
    assert!(s.host.waiting(), "{}", s.tail_log());
    assert_eq!(s.host.saved_pads(), 1);
    assert!(s.take_events().is_empty());
    assert!(!s.log_has("key change"), "the key table never changes");
    assert_nothing_forbidden(&s);
    s.assert_key_never_printed();
}

fn pad_goes(reason: u8) {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (mut pad, _, intr) = switch_connected(&mut s);
    pad.buttons = [0x08, 0, 0];
    s.switch_runs(&mut pad, intr, 30);
    s.take_events();
    s.wakes.clear();
    s.feed(&ev_disconnected(H, reason));
    let wake = s.tick();
    assert!(wake, "a pad that goes wakes the console");
    assert_eq!(s.take_events(), vec![PadEvent::Disconnected { reason }]);
    assert_eq!(s.host.read(), PadState::NONE);
    assert_eq!(s.host.motion(), None);
    assert_eq!(s.host.battery(), None);
    assert_eq!(s.host.take_pressed(), 0, "the latch goes with the pad");
    assert!(s.log_has(&format!("PAD-HOST gone reason={reason:#04x}")));
    s.advance(1_100);
    assert!(s.host.waiting(), "{}", s.tail_log());
    // And it comes back.
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
    assert_eq!(
        pad.outputs.len(),
        4,
        "the start-up talk again, from the start"
    );
    assert_nothing_forbidden(&s);
}

/// The pad is switched off: it ends the link ("remote user ended", 0x13).
#[test]
fn the_pad_switches_off() {
    pad_goes(0x13);
}

/// The pad goes out of range: the controller gives up on the link
/// (supervision timeout, 0x08), as at the end of live3.
#[test]
fn the_pad_goes_out_of_range() {
    pad_goes(fixtures::SWITCH_WORKS.end_reason.unwrap());
}

/// The other three modes, from their logged sessions: the mode is
/// recognised, the reports are decoded, and in the Xbox modes the battery
/// question is asked once (its answer here is made up: no answer was ever
/// logged).
fn other_mode(f: &Session, want: Mode) -> Sim {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    let (ctrl, intr) = s.reconnect(f);
    let mut pressed = 0;
    for r in f.reports {
        s.report(intr, &hex(r));
        s.advance(10);
        pressed |= s.host.take_pressed();
    }
    assert!(s.host.connected(), "{}", s.tail_log());
    assert_eq!(s.host.mode(), want, "{}", s.tail_log());
    let ev = s.take_events();
    assert!(
        matches!(ev[0], PadEvent::Connected { mode, name } if mode == want && name.as_str() == f.name),
        "{ev:?}"
    );
    assert_eq!(s.host.motion(), None, "motion only in Switch mode");
    assert!(pressed != 0, "the logged session pressed buttons");
    assert!(s.log_has(&format!("connected mode={}", want.word())));
    let asks: Vec<Vec<u8>> = s.drain_data(PAD_CTRL);
    if want == Mode::DInput {
        assert!(asks.is_empty(), "{asks:?}");
    } else {
        // GET_REPORT, input, report 0x04: once.
        assert_eq!(asks, vec![vec![0x41, 0x04]], "{}", s.tail_log());
        s.control(ctrl, &[0xA1, 0x04, 0x13]);
        s.advance(10);
        let b = Battery {
            percent: 100,
            charging: true,
            level: 3,
        };
        assert_eq!(s.host.battery(), Some(b));
        assert_eq!(s.take_events(), vec![PadEvent::Battery(b)]);
    }
    assert_nothing_forbidden(&s);
    s.assert_no_full_address();
    s
}

#[test]
fn xinput_mode_is_decoded() {
    other_mode(&fixtures::XINPUT, Mode::XInput);
}

#[test]
fn android_mode_is_decoded() {
    other_mode(&fixtures::ANDROID, Mode::Android);
}

#[test]
fn dinput_mode_is_decoded() {
    other_mode(&fixtures::DINPUT, Mode::DInput);
}

/// A press that is down in one report and up in the next, both between
/// two frames, is seen once by `take_pressed`, and not again.
#[test]
fn a_short_press_is_seen_once() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (mut pad, _, intr) = switch_connected(&mut s);
    s.host.take_pressed();
    pad.buttons = [0x08, 0, 0];
    let down = pad.next_report();
    pad.buttons = [0, 0, 0];
    let up = pad.next_report();
    s.report(intr, &down);
    s.report(intr, &up);
    assert_eq!(s.host.read().buttons, 0, "it is up again");
    assert_eq!(s.host.take_pressed(), btn::EAST);
    s.advance(10);
    assert_eq!(s.host.take_pressed(), 0);
    // Also when a tick comes between the reports and the frame.
    s.report(intr, &down);
    s.report(intr, &up);
    s.advance(20);
    assert_eq!(s.host.take_pressed(), btn::EAST);
    assert_eq!(s.host.take_pressed(), 0);
}

/// A pad lying still sends about 100 reports a second, each a little
/// different (the logged reports: stick noise, motion, counter). None of
/// it is a change: the console may sleep.
#[test]
fn a_resting_pad_does_not_wake_the_console() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (mut pad, _, intr) = switch_connected(&mut s);
    s.wakes.clear();
    let from = s.now;
    let before = s.host.stats().reports;
    s.switch_runs(&mut pad, intr, 2_000);
    let sent = s.host.stats().reports - before;
    assert!(sent >= 190, "{sent} reports");
    assert!(s.wakes.is_empty(), "woke at {:?} (from {from})", s.wakes);
    // A button does wake it.
    pad.buttons = [0x08, 0, 0];
    s.switch_runs(&mut pad, intr, 20);
    assert!(!s.wakes.is_empty());
    // Moving a stick out of the dead zone does too (the rule itself is
    // tested in map.rs).
    let rest = s.host.read();
    let mut moved = rest;
    moved.left.x = 9000;
    assert!(map::changed(&rest, &moved));
}

/// The pad calls and then says nothing at all: the host gives up after
/// its timeouts and waits again, ready for the next call.
#[test]
fn a_silent_pad_is_dropped_and_the_host_waits() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    assert!(s.pad_calls(PAD, PAD_COD));
    s.advance(60_000);
    assert!(s.host.waiting(), "{}", s.tail_log());
    assert!(s.sent(op::DISCONNECT));
    assert!(s.log_has("timeout"), "{}", s.tail_log());
    // The next call works.
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
}

/// HID opens but the pad never sends a report and ignores every output.
/// The link stays (an Xbox-mode pad may be silent for minutes: it sends
/// only on change), nothing is shown to games, the start-up talk tries a
/// bounded number of times and stops, and when the pad hangs up the host
/// waits again.
#[test]
fn a_pad_that_never_reports_is_not_shown_and_the_talk_gives_up() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    pad.answers_outputs = false;
    s.advance(60_000);
    s.switch_answers(&mut pad, 0);
    let tried = pad.outputs.len();
    assert!((1..=40).contains(&tried), "{tried} outputs");
    s.advance(60_000);
    s.switch_answers(&mut pad, 0);
    assert_eq!(pad.outputs.len(), tried, "the talk stopped");
    assert_eq!(s.host.phase(), "connected");
    assert!(!s.host.connected());
    assert_eq!(s.host.read(), PadState::NONE);
    assert!(s.take_events().is_empty());
    s.feed(&ev_disconnected(H, 0x13));
    s.advance(1_100);
    assert!(s.host.waiting(), "{}", s.tail_log());
}

/// The controller never confirms our Disconnect (a controller fault): the
/// host drops the link itself after a while and waits again; a late
/// confirmation is ignored.
#[test]
fn a_missing_disconnection_complete_does_not_hang_the_host() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.confirm_disconnect = false;
    assert!(s.pad_calls(PAD, PAD_COD));
    s.feed(&ev_addr(0x17, PAD));
    s.take_cmd(op::LINK_KEY_REQUEST_REPLY);
    s.feed(&ev_addr(0x31, PAD));
    s.take_cmd(op::DISCONNECT);
    s.advance(4_000);
    assert!(!s.host.waiting(), "still waiting for the controller");
    s.advance(3_000);
    assert!(s.host.waiting(), "{}", s.tail_log());
    assert!(s.log_has("no disconnection-complete"), "{}", s.tail_log());
    s.feed(&ev_disconnected(H, 0x16));
    s.advance(100);
    assert!(s.host.waiting());
    s.confirm_disconnect = true;
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
}

/// The link key never shows in a log line, an event's Debug, the load
/// result, the counters, or a saved pairing's Debug.
#[test]
fn the_link_key_never_reaches_any_output() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (mut pad, _, intr) = switch_connected(&mut s);
    pad.buttons = [0x08, 0, 0];
    s.switch_runs(&mut pad, intr, 30);
    s.feed(&ev_disconnected(H, 0x08));
    s.advance(1_100);
    s.assert_key_never_printed();
    s.assert_no_full_address();
    let mut texts = Vec::new();
    for e in s.take_events() {
        texts.push(format!("{e:?}"));
    }
    texts.push(format!("{:?}", s.host.stats()));
    texts.push(format!("{:?}", s.host.read()));
    let mut host = PadHost::new();
    let mut view = ReadOnly {
        ram: &s.ram,
        reads: 0,
    };
    let mut rec = Rec::default();
    let loaded = host.load_pairings(&mut view, region(), &mut rec).unwrap();
    assert_eq!(loaded.keys, 1);
    texts.push(format!("{loaded:?}"));
    texts.extend(rec.logs.iter().cloned());
    let p = pad_store::Pairing {
        addr: PAD.0,
        key: KEY,
        key_type: 4,
    };
    texts.push(format!("{p:?}"));
    for t in &texts {
        assert_no_key(t);
        assert_no_address(t);
    }
}

/// Loading reads only: the area's CRC is the same after, and it is the
/// one the log line gives.
#[test]
fn loading_the_pairings_only_reads() {
    let ram = Ram::with(&[(PAD, KEY)]);
    let before = ram.0.clone();
    let mut host = PadHost::new();
    let mut view = ReadOnly {
        ram: &ram,
        reads: 0,
    };
    let mut rec = Rec::default();
    let loaded = host.load_pairings(&mut view, region(), &mut rec).unwrap();
    assert!(view.reads > 0);
    assert_eq!(ram.0, before);
    assert_eq!(loaded.keys, 1);
    assert_eq!(loaded.report.count, 1);
    let crc = format!("crc32={:#010x}", area_crc(&ram));
    assert!(rec.logs[0].contains(&crc), "{} vs {crc}", rec.logs[0]);
}

/// The host is built at compile time: a board keeps it in a static and
/// never builds its 19.5 KB on a task's stack.
#[test]
fn the_host_can_live_in_a_static() {
    static FRESH: PadHost = PadHost::new();
    assert!(!FRESH.connected());
    assert!(!FRESH.waiting(), "not started yet");
    assert_eq!(FRESH.saved_pads(), 0);
    assert_eq!(FRESH.read(), PadState::NONE);
}
