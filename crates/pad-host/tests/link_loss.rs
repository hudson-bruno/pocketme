//! A pad switched off, and switched on again (round 6, the owner's test:
//! the console took 20 s to notice, and the pad could not come back
//! before that). Against the scripted controller and the logged pad:
//!
//! - the supervision timeout: written once we are the link's central,
//!   from the pad's own SDP wish, kept above any sniff interval, refused
//!   without harm;
//! - the silence guard: a streaming pad (Switch mode, full reports) that
//!   says nothing for [`SILENCE_MS`] is gone at once; never for an
//!   injected pad, a mode that sends only on change, the start-up talk,
//!   or a busy loop;
//! - the stale link: from the moment the console says "gone", a new call
//!   from the pad is a normal reconnect, whether the old link's
//!   Disconnection Complete comes at once, late, after the new call, or
//!   never; and the linked pad calling again drops the old link.

mod common;

use board_api::{PadState, btn};
use common::fixtures::{self, Session};
use common::*;
use pad_host::inject::{self, SwitchInput};
use pad_host::{Mode, PadEvent, SILENCE_MS, STALL_MS};
use pad_proto::hci::op;
use pad_proto::host::{LSTO_DEFAULT, LSTO_MAX, LSTO_MIN, supervision_slots};

const H2: u16 = 0x0081;

/// The pad calls, the role change comes (as in every logged call: after
/// the link is up, before the key), the rest of the reconnect, and the
/// start-up talk until full reports with motion flow.
fn switch_central(s: &mut Sim) -> (SwitchPad, u16) {
    s.until_waiting();
    s.pad_name = Some(fixtures::SWITCH_WORKS.name.to_string());
    assert!(s.pad_calls(PAD, fixtures::SWITCH_WORKS.cod));
    s.feed(&ev_role(0, PAD, 0));
    s.pad_uses_saved_key(PAD);
    let (_, intr) = s.pad_connects_hid(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    assert!(pad.full && pad.motion);
    (pad, intr)
}

/// A pad of mode `f` connected (no role change), its logged reports sent.
fn other_mode(s: &mut Sim, f: &Session) -> u16 {
    s.until_waiting();
    let (_, intr) = s.reconnect(f);
    for r in f.reports {
        s.report(intr, &hex(r));
        s.advance(10);
    }
    assert!(s.host.connected(), "{}", s.tail_log());
    intr
}

fn disconnects(s: &Sim, handle: u16) -> usize {
    s.history
        .iter()
        .filter(|(o, p)| *o == op::DISCONNECT && u16le(p, 0) == handle)
        .count()
}

// ---- supervision timeout ------------------------------------------------

#[test]
fn the_supervision_value_is_the_pads_wish_kept_between_1_and_5_s() {
    assert_eq!(LSTO_DEFAULT, 3200, "2 s");
    assert_eq!(LSTO_MIN, 1600, "1 s");
    assert_eq!(LSTO_MAX, 8000, "5 s");
    // The GuliKit's wish (SDP 0x020C = 0x0C80): taken as it is.
    assert_eq!(supervision_slots(Some(0x0C80), 0), 0x0C80);
    assert_eq!(supervision_slots(Some(4000), 0), 4000);
    // Nothing, or 0 ("no timeout"): 2 s.
    assert_eq!(supervision_slots(None, 0), 3200);
    assert_eq!(supervision_slots(Some(0), 0), 3200);
    // Too short or too long: kept in range.
    assert_eq!(supervision_slots(Some(1), 0), 1600);
    assert_eq!(supervision_slots(Some(0x7D00), 0), 8000);
    assert_eq!(supervision_slots(Some(0xFFFF), 0), 8000);
    // Sniff: three intervals at least, even above 5 s.
    assert_eq!(supervision_slots(Some(0x0C80), 800), 3200, "0.5 s sniff");
    assert_eq!(supervision_slots(Some(0x0C80), 2048), 6144, "1.28 s sniff");
    assert_eq!(supervision_slots(None, 4000), 12000);
    assert_eq!(supervision_slots(None, 0xFFFE), 0xFFFF);
}

#[test]
fn the_supervision_timeout_is_written_once_we_are_central() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    assert!(s.pad_calls(PAD, PAD_COD));
    assert!(lsto_written(&s).is_empty(), "peripheral: not ours to write");
    s.feed(&ev_role(0, PAD, 0));
    // Before SDP: the 2 s default, at once.
    assert_eq!(lsto_written(&s), vec![(H, 3200)]);
    assert!(s.log_has(
        "supervision timeout: 3200 slots (2000 ms) for handle=0x080 (the pad asked nothing (yet)"
    ));
    s.pad_uses_saved_key(PAD);
    let (_, intr) = s.pad_connects_hid(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    // The pad asked for the same in SDP: not written again.
    assert_eq!(lsto_written(&s), vec![(H, 3200)]);
    assert!(s.host.connected());
    assert_nothing_forbidden(&s);
}

#[test]
fn a_link_where_the_role_never_changes_gets_no_supervision_timeout() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    other_mode(&mut s, &fixtures::SWITCH_WORKS);
    s.advance(5_000);
    assert!(lsto_written(&s).is_empty());
    // A role change to peripheral (or a failed one) changes nothing.
    s.feed(&ev_role(0, PAD, 1));
    s.feed(&ev_role(0x0C, PAD, 0));
    s.advance(100);
    assert!(lsto_written(&s).is_empty());
}

#[test]
fn a_role_change_before_connection_complete_counts() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.feed(&ev_conn_request(PAD, PAD_COD));
    s.take_cmd(op::ACCEPT_CONNECTION_REQUEST);
    s.feed(&ev_role(0, PAD, 0));
    assert!(lsto_written(&s).is_empty(), "no link yet");
    s.feed(&ev_conn_complete(0, H, PAD));
    assert_eq!(lsto_written(&s), vec![(H, 3200)]);
}

#[test]
fn a_role_change_from_another_device_is_ignored() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    s.feed(&ev_conn_request(PAD, PAD_COD));
    s.take_cmd(op::ACCEPT_CONNECTION_REQUEST);
    s.feed(&ev_role(0, STRANGER, 0));
    s.feed(&ev_conn_complete(0, H, PAD));
    s.feed(&ev_role(0, STRANGER, 0));
    assert!(lsto_written(&s).is_empty());
}

#[test]
fn the_timeout_stays_longer_than_a_sniff_interval() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (_pad, _) = switch_central(&mut s);
    assert_eq!(lsto_written(&s), vec![(H, 3200)]);
    // A short sniff interval (0.5 s): 3 x 800 < 3200, nothing to do.
    s.feed(&ev_mode(H, 2, 800));
    assert_eq!(lsto_written(&s).len(), 1);
    // A long one (1.28 s): the timeout grows to 3 intervals.
    s.feed(&ev_mode(H, 2, 2048));
    assert_eq!(lsto_written(&s).last(), Some(&(H, 6144)));
    // Active again: back to the pad's 2 s.
    s.feed(&ev_mode(H, 0, 0));
    assert_eq!(lsto_written(&s).last(), Some(&(H, 3200)));
    // A mode change on another handle changes nothing.
    s.feed(&ev_mode(H2, 2, 4000));
    assert_eq!(lsto_written(&s).len(), 3);
}

#[test]
fn a_refused_supervision_timeout_does_no_harm() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.refuse = Some((op::WRITE_LINK_SUPERVISION_TIMEOUT, 0x0C));
    let (mut pad, intr) = switch_central(&mut s);
    assert_eq!(lsto_written(&s).len(), 1, "tried once, no retry");
    assert!(s.log_has("supervision timeout not taken (status 0x0c)"));
    s.switch_runs(&mut pad, intr, 2_000);
    assert_eq!(lsto_written(&s).len(), 1);
    assert!(s.host.connected());
    assert!(!s.sent(op::DISCONNECT));
}

// ---- silence guard ------------------------------------------------------

#[test]
fn a_switched_off_pad_is_gone_after_one_second_of_silence() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (mut pad, intr) = switch_central(&mut s);
    assert!(s.log_has(&format!(
        "full reports flow: silence guard on ({SILENCE_MS} ms)"
    )));
    pad.buttons = [0x08, 0, 0];
    s.switch_runs(&mut pad, intr, 30);
    s.take_events();
    assert_ne!(s.host.read().buttons, 0);
    // Switched off: nothing more comes.
    let off = s.now;
    s.advance(SILENCE_MS - 20);
    assert!(s.host.connected(), "not yet");
    assert_eq!(disconnects(&s, H), 0);
    s.advance(30);
    assert!(!s.host.connected(), "{}", s.tail_log());
    let took = s.now - off;
    assert!((SILENCE_MS..=SILENCE_MS + 20).contains(&took), "{took} ms");
    assert_eq!(
        s.take_events(),
        vec![PadEvent::Disconnected { reason: 0x08 }]
    );
    assert_eq!(s.host.read(), PadState::NONE);
    assert_eq!(s.host.take_pressed(), 0, "buttons released");
    assert_eq!(s.host.motion(), None);
    assert_eq!(disconnects(&s, H), 1, "the host ends the link");
    assert!(s.host.waiting(), "ready for the next call at once");
    assert!(s.log_has("silent for 1000 ms"));
    assert_eq!(s.host.counters().silent, 1);
    assert_eq!(s.host.counters().lost, 1);
    // The controller confirmed (the scripted one does at once): calm.
    assert!(s.log_has("old link closed by the controller"));
    // And the pad comes back as usual.
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
    assert_eq!(s.host.counters().attempts, 2);
    assert_nothing_forbidden(&s);
}

#[test]
fn gaps_like_the_real_pads_never_trip_the_guard() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (mut pad, intr) = switch_central(&mut s);
    // The longest real gap was 366 ms; here gaps of 900 ms, 30 times.
    for _ in 0..30 {
        let r = pad.next_report();
        s.report(intr, &r);
        s.advance(900);
    }
    assert!(s.host.connected(), "{}", s.tail_log());
    assert_eq!(disconnects(&s, H), 0);
    assert!(
        s.take_events()
            .iter()
            .all(|e| !matches!(e, PadEvent::Disconnected { .. }))
    );
}

/// A streaming pad that goes back to the simple reports (0x3F, sent only
/// on change) is no longer watched: quiet then is not a pad gone.
#[test]
fn a_pad_back_on_simple_reports_is_not_watched() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (_pad, intr) = switch_central(&mut s);
    s.report(intr, &hex("3f00000f007a007f007e0083"));
    s.advance(10);
    assert!(s.log_has("simple reports again: silence guard off"));
    s.advance(20_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    assert_eq!(disconnects(&s, H), 0);
}

#[test]
fn modes_that_send_only_on_change_stay_connected_when_quiet() {
    for f in [&fixtures::XINPUT, &fixtures::ANDROID, &fixtures::DINPUT] {
        let mut s = Sim::new(&[(PAD, KEY)]);
        other_mode(&mut s, f);
        s.advance(60_000);
        assert!(s.host.connected(), "{}: {}", f.name, s.tail_log());
        assert!(!s.sent(op::DISCONNECT), "{}", f.name);
    }
}

#[test]
fn the_start_up_talk_before_full_reports_is_not_watched() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    // The pad sends its simple reports (0x3f) and ignores the talk: no
    // full reports ever come.
    pad.answers_outputs = false;
    for _ in 0..5 {
        let r = pad.next_report();
        s.report(intr, &r);
        s.advance(10);
    }
    assert!(s.host.connected());
    s.advance(20_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    assert!(!s.sent(op::DISCONNECT));
    assert!(!s.log_has("silence guard on"));
}

#[test]
fn an_injected_pad_is_never_dropped_for_silence() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.until_waiting();
    assert!(s.host.inject_begin(s.now, Mode::Switch, &mut s.radio));
    let r = inject::switch_report(&SwitchInput {
        buttons: btn::SOUTH,
        ..SwitchInput::default()
    });
    assert!(s.host.inject_report(s.now, &r, &mut s.radio));
    s.tick();
    assert!(s.host.connected());
    s.advance(30_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    assert!(s.host.injecting());
    assert!(!s.log_has("silent for"));
    assert!(
        !s.take_events()
            .iter()
            .any(|e| matches!(e, PadEvent::Disconnected { .. }))
    );
}

#[test]
fn a_busy_loop_is_not_taken_for_a_silent_pad() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (mut pad, intr) = switch_central(&mut s);
    // The loop is busy for 1.5 s (no tick); the reports that came meanwhile
    // are handed over before the next tick: still here.
    s.now += 1_500;
    let r = pad.next_report();
    s.report(intr, &r);
    s.tick();
    assert!(s.host.connected());
    // Busy again for 1.5 s, and this time nothing came: the late tick
    // does not count the time it was not looking ...
    s.now += 1_500;
    s.tick();
    assert!(s.host.connected(), "a late look is not a silent pad");
    const { assert!(STALL_MS < SILENCE_MS) };
    // ... but if the pad stays silent, it is gone a second later.
    s.advance(SILENCE_MS + 20);
    assert!(!s.host.connected(), "{}", s.tail_log());
}

// ---- the stale link -----------------------------------------------------

/// The guard drops the link; the controller has not closed the old one
/// yet (its Disconnection Complete is late); the pad, switched on again,
/// calls: the call is taken and the pad works, on a new handle.
#[test]
fn after_the_guard_the_pad_is_taken_back_before_the_old_link_closes() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.confirm_disconnect = false;
    let (_, _) = switch_central(&mut s);
    s.advance(SILENCE_MS + 20);
    assert!(s.host.waiting());
    assert_eq!(disconnects(&s, H), 1);
    s.take_events();
    // Data that still comes on the old handle: dropped, quietly.
    let errs = s
        .radio
        .logs
        .iter()
        .filter(|l| l.starts_with("PAD-ERR"))
        .count();
    s.pad_frame(H, PAD_INTR, &[0xA1, 0x30]);
    // The pad calls again on a new handle, the old link still open.
    s.handle = H2;
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    s.feed(&ev_role(0, PAD, 0));
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    assert_eq!(lsto_written(&s).last(), Some(&(H2, 3200)));
    // Now the old link's end comes, late: the new link stays.
    s.feed(&ev_disconnected(H, 0x08));
    s.switch_runs(&mut pad, intr, 500);
    assert!(s.host.connected(), "{}", s.tail_log());
    assert!(s.log_has("old link closed by the controller"));
    assert!(
        !s.take_events()
            .iter()
            .any(|e| matches!(e, PadEvent::Disconnected { .. }))
    );
    let errs_after = s
        .radio
        .logs
        .iter()
        .filter(|l| l.starts_with("PAD-ERR"))
        .count();
    assert_eq!(errs, errs_after, "{}", s.tail_log());
    assert_eq!(disconnects(&s, H2), 0, "never the new link");
    assert_nothing_forbidden(&s);
}

/// The old link's end comes between the new call and its Connection
/// Complete.
#[test]
fn an_old_links_end_between_the_new_call_and_its_completion_changes_nothing() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.confirm_disconnect = false;
    switch_central(&mut s);
    s.advance(SILENCE_MS + 20);
    s.feed(&ev_conn_request(PAD, PAD_COD));
    s.take_cmd(op::ACCEPT_CONNECTION_REQUEST);
    s.feed(&ev_disconnected(H, 0x16));
    assert_eq!(s.host.phase(), "accepting", "{}", s.tail_log());
    s.feed(&ev_conn_complete(0, H2, PAD));
    s.handle = H2;
    s.advance(20);
    s.pad_answers_info();
    s.pad_uses_saved_key(PAD);
    let (_, intr) = s.pad_connects_hid(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
}

/// The controller never says the old link closed: the host forgets it
/// after a while and the pad comes back anyway (here even on the same
/// handle number, which a controller gives out only once the old link is
/// gone).
#[test]
fn an_old_link_that_never_closes_does_not_block_the_pad() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.confirm_disconnect = false;
    switch_central(&mut s);
    s.advance(SILENCE_MS + 20);
    assert!(s.host.waiting());
    s.advance(12_000);
    let said = |s: &Sim| {
        s.radio
            .logs
            .iter()
            .filter(|l| l.contains("no disconnection-complete for the old link"))
            .count()
    };
    assert_eq!(said(&s), 1, "{}", s.tail_log());
    s.advance(5_000);
    assert_eq!(said(&s), 1, "forgotten: said once");
    assert!(s.host.waiting());
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected());
    // Taken again at once, before the host forgot the old link.
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.confirm_disconnect = false;
    switch_central(&mut s);
    s.advance(SILENCE_MS + 20);
    s.advance(300);
    let (_, intr) = s.reconnect(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
}

/// The pad we are linked to calls again (it was switched off and on
/// before anyone noticed, and the controller passed the call on): the old
/// link is dropped, the console sees the pad go and come, and the new
/// call is taken. Never two links, never stuck.
#[test]
fn the_linked_pad_calling_again_replaces_the_old_link() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    s.confirm_disconnect = false;
    let (_pad, _) = switch_central(&mut s);
    s.take_events();
    s.cmds.clear();
    s.feed(&ev_conn_request(PAD, PAD_COD));
    assert_eq!(disconnects(&s, H), 1, "the old link is ended");
    let accept = s.take_cmd(op::ACCEPT_CONNECTION_REQUEST);
    assert_eq!(&accept[..6], &PAD.0);
    assert!(!s.sent(op::REJECT_CONNECTION_REQUEST));
    s.tick();
    assert_eq!(
        s.take_events(),
        vec![PadEvent::Disconnected { reason: 0x16 }]
    );
    assert_eq!(s.host.read(), PadState::NONE);
    assert!(s.log_has("the linked pad calls again"));
    s.feed(&ev_conn_complete(0, H2, PAD));
    s.handle = H2;
    s.advance(20);
    s.pad_answers_info();
    s.pad_uses_saved_key(PAD);
    let (_, intr) = s.pad_connects_hid(&fixtures::SWITCH_WORKS);
    let mut pad = SwitchPad::new(&fixtures::SWITCH_WORKS);
    s.switch_runs(&mut pad, intr, 3_000);
    assert!(s.host.connected(), "{}", s.tail_log());
    s.feed(&ev_disconnected(H, 0x16));
    s.switch_runs(&mut pad, intr, 100);
    assert!(s.host.connected());
    let c = s.host.counters();
    assert_eq!((c.attempts, c.ready, c.lost), (2, 2, 1), "{c:?}");
    assert_nothing_forbidden(&s);
}

/// A call from a stranger while linked is still refused, and the link
/// stays.
#[test]
fn a_strangers_call_while_linked_changes_nothing() {
    let mut s = Sim::new(&[(PAD, KEY)]);
    let (mut pad, intr) = switch_central(&mut s);
    s.feed(&ev_conn_request(STRANGER, PAD_COD));
    assert!(s.sent(op::REJECT_CONNECTION_REQUEST));
    assert!(!s.sent(op::DISCONNECT));
    s.switch_runs(&mut pad, intr, 100);
    assert!(s.host.connected());
}
